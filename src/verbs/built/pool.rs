//! The live picture of the co-worker pool the console draws as tiles (docs/api.md, "The co-worker pool
//! board"): a tile for every place a tier has on a provider that is on and lends co-workers, and one for
//! every worker's own turn. It is read from bmo-agency's pool and the queue's rows on every ask, and nothing
//! here writes.

use job_agency::{Agency, PoolRequest, PoolState};
use queue_rows::{todos, work_orders as rows};
use serde_json::Value;

use super::Built;
use crate::queue::units::{Units, agency_failed};
use crate::queue::workers::workers;
use crate::verbs::pool::{Bucket, LogPlace, Tile, TileState, Work, WorkKind, WorkerTile};
use crate::verbs::{Board, Done, PoolBoard, Target, TargetKind};
use crate::{Refusal, now_ms, one};

// The role and tier a worker's own job is read at where its row names none.
const JOB_ROLE: &str = "builder";

/// The number a request's owner carries after a prefix, as `work-order:<item>` and `search-run:<run>` do.
fn number_after(prefix: &str, owner: &str) -> Option<i64> {
  owner.strip_prefix(prefix)?.split(':').next()?.parse().ok()
}

/// Where in a work order a pool request's task stands: its stage's title, and the task.
fn order_task<'a>(order: &'a Value, token: &str) -> Option<(Option<&'a str>, &'a Value)> {
  let asks = |task: &&Value| task.get("poolRequest").and_then(Value::as_str) == Some(token);
  let list = |value: &'a Value, key: &str| value.get(key).and_then(Value::as_array).into_iter().flatten();

  for stage in list(order, "stages") {
    if let Some(task) = list(stage, "tasks").find(asks) {
      return Some((stage.get("title").and_then(Value::as_str), task));
    }
  }

  list(order, "tasks")
    .find(asks)
    .or_else(|| order.get("control").filter(asks))
    .map(|task| (None, task))
}

fn idle() -> Tile {
  Tile {
    state: TileState::Idle,
    work: None,
  }
}

fn unit_stop(log: Option<&LogPlace>) -> Option<Target> {
  log.map(|log| Target {
    kind: TargetKind::Unit,
    id: log.run.clone(),
  })
}

impl Built {
  async fn item_title(&self, id: i64) -> Result<Option<String>, Refusal> {
    Ok(
      todos::item(&self.db, id)
        .await?
        .map(|item| item.title)
        .filter(|title| !title.is_empty()),
    )
  }

  // The worker whose job stands in a unit, the latest where more than one row names it.
  async fn job_worker(&self, unit: &str) -> Result<Option<String>, Refusal> {
    let row = one(
      &self.db,
      r#"SELECT "worker" FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
      vec![unit.into()],
    )
    .await?;

    Ok(row.map(|row| row.try_get::<Option<String>>("", "worker")).transpose()?.flatten())
  }

  // What one request holds or waits for, named by its owner.
  async fn work_of(&self, request: &PoolRequest) -> Result<Work, Refusal> {
    let asking = &request.asking;
    let since = if request.state == PoolState::Waiting {
      Some(request.enqueued_at)
    } else {
      request.leased_at
    };
    let mut work = Work {
      kind: WorkKind::Other,
      role: asking.role.clone(),
      tier: asking.tier.clone(),
      provider: asking.provider.clone(),
      model: asking.model.clone(),
      effort: asking.effort.clone(),
      since,
      item: None,
      title: Some(asking.owner.clone()),
      detail: Some(asking.source.clone()),
      stop: None,
      log: None,
    };

    if let Some(item) = number_after("work-order:", &asking.owner) {
      let order = rows::read(&self.db, item).await?;
      let found = order.as_ref().and_then(|order| order_task(order, &request.token));
      let step = found.map(|(stage, task)| {
        let name = task.get("title").or_else(|| task.get("role")).and_then(Value::as_str);

        [stage, name]
          .into_iter()
          .flatten()
          .filter(|part| !part.is_empty())
          .collect::<Vec<_>>()
          .join(" · ")
      });
      let log = match &request.child {
        Some(child) => self.job_worker(child).await?.map(|agent| LogPlace {
          agent,
          run: child.clone(),
        }),
        None => None,
      };

      work.kind = if asking.turn { WorkKind::Turn } else { WorkKind::Cowork };
      work.item = Some(item);
      work.title = self.item_title(item).await?;
      work.detail = step.filter(|step| !step.is_empty());
      work.stop = unit_stop(log.as_ref());
      work.log = log;

      return Ok(work);
    }

    for (prefix, kind) in [("search-run:", WorkKind::Search), ("browse-run:", WorkKind::Browse)] {
      let Some(number) = number_after(prefix, &asking.owner) else {
        continue;
      };
      let run = self.ground.agency.run(number).await.map_err(agency_failed)?;
      let log = run.as_ref().map(|run| LogPlace {
        agent: run.agent.clone(),
        run: run.unit.clone(),
      });

      work.kind = kind;
      work.title = Some(number.to_string());
      work.detail = run.map(|run| run.depth.to_string());
      work.stop = unit_stop(log.as_ref());
      work.log = log;

      return Ok(work);
    }

    if asking.source == "native" || asking.owner.starts_with("native:") {
      work.kind = WorkKind::Native;
      work.detail = None;
    }

    Ok(work)
  }

  async fn tile_of(&self, request: &PoolRequest) -> Result<Tile, Refusal> {
    Ok(Tile {
      state: if request.state == PoolState::Active { TileState::Working } else { TileState::Held },
      work: Some(self.work_of(request).await?),
    })
  }

  // A worker's own open job, which takes no place in the pool but is its worker's one place all the same:
  // the latest where more than one row is open.
  async fn job_work(&self, worker: &str) -> Result<Option<Work>, Refusal> {
    let row = one(
      &self.db,
      r#"SELECT "unit", "todo_id", "purpose", "launched_at", "execution_mode", "execution_model",
           "execution_effort", "execution_tier"
         FROM "jobs" WHERE "worker" = $1 AND "finished_at" IS NULL ORDER BY "id" DESC LIMIT 1"#,
      vec![worker.into()],
    )
    .await?;
    let Some(row) = row else { return Ok(None) };
    let text = |name: &str| row.try_get::<Option<String>>("", name);
    let unit: String = row.try_get("", "unit")?;
    let item: Option<i64> = row.try_get("", "todo_id")?;
    // A job with no item stands under why it was launched.
    let title = match item {
      Some(item) => self.item_title(item).await?,
      None => text("purpose")?.filter(|purpose| !purpose.trim().is_empty()),
    };
    let log = LogPlace {
      agent: worker.to_owned(),
      run: unit.clone(),
    };

    Ok(Some(Work {
      kind: WorkKind::Turn,
      role: JOB_ROLE.to_owned(),
      tier: text("execution_tier")?.unwrap_or_default(),
      provider: text("execution_mode")?.unwrap_or_default(),
      model: text("execution_model")?.unwrap_or_default(),
      effort: text("execution_effort")?,
      since: row.try_get("", "launched_at")?,
      item,
      title,
      detail: None,
      stop: unit_stop(Some(&log)),
      log: Some(log),
    }))
  }
}

impl PoolBoard for Built {
  async fn pool_board(&self) -> Done<Board> {
    let units = self.units();
    let live = units.open_requests(None).await?;
    let tiers: Vec<String> = units
      .tiers()
      .await?
      .into_iter()
      .filter(|tier| tier.coworker)
      .map(|tier| tier.name)
      .collect();
    let mut buckets = vec![];

    for provider in units
      .providers()
      .await?
      .into_iter()
      .filter(|provider| provider.enabled && provider.coworker)
    {
      for tier in &tiers {
        let room = self.ground.agency.pool_room(&provider.name, tier).await.map_err(agency_failed)?;
        let capacity = u32::try_from(room.capacity).unwrap_or_default();
        let mine: Vec<&PoolRequest> = live
          .iter()
          .filter(|one| {
            !one.asking.turn && one.asking.provider == provider.name && &one.asking.tier == tier
          })
          .collect();

        if capacity == 0 && mine.is_empty() {
          continue;
        }

        let (mut tiles, mut waiting) = (vec![], vec![]);

        for request in mine {
          if request.state == PoolState::Waiting {
            waiting.push(self.work_of(request).await?);
          } else {
            tiles.push(self.tile_of(request).await?);
          }
        }

        while tiles.len() < capacity as usize {
          tiles.push(idle());
        }

        buckets.push(Bucket {
          provider: provider.name.clone(),
          tier: tier.clone(),
          capacity,
          tiles,
          waiting,
        });
      }
    }

    let mut tiles = vec![];

    for worker in workers(&self.db, &self.ground.source).await? {
      let mine: Vec<&PoolRequest> = live
        .iter()
        .filter(|one| one.asking.turn && one.asking.place_key.as_deref() == Some(worker.cwd.as_str()))
        .collect();
      let tile = match mine.iter().find(|one| one.state == PoolState::Active) {
        Some(request) => self.tile_of(request).await?,
        None => match self.job_work(&worker.name).await? {
          Some(work) => Tile {
            state: TileState::Working,
            work: Some(work),
          },
          None => idle(),
        },
      };
      let mut waiting = vec![];

      for request in mine.into_iter().filter(|one| one.state == PoolState::Waiting) {
        waiting.push(self.work_of(request).await?);
      }

      tiles.push(WorkerTile {
        name: worker.name,
        label: worker.label,
        tile,
        waiting,
      });
    }

    Ok(Board {
      read_at: now_ms(),
      buckets,
      workers: tiles,
    })
  }
}
