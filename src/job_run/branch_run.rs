//! A co-work branch's turn on Codex or OpenRouter (docs/runs.md, "The turns"). bmo-agency carries the turn as
//! one run of the road's program on a session thrown away; what is made of its answer here is the upstream
//! that took it, written on the job's row, the answer told to the cache siren, and the model and effort it
//! went out on named on the answer.

use job_agency::Reply;

use super::claude::turn::{Asked, ClaudeRoad};
use super::siren;
use crate::queue::routes::JOB_WORK_KEY;
use crate::{Refusal, run};

/// The road whose program reports each of a turn's requests.
pub const CODEX: &str = "codex";

/// The road whose program reports what it spent as a whole.
pub const OPENROUTER: &str = "openrouter";

/// Whether a branch's turn on the provider is one this module makes something of: the stand-in roads.
pub fn carries(provider: &str) -> bool {
  provider == CODEX || provider == OPENROUTER
}

/// What a branch's answer on a stand-in road is taken as: the upstream that took it written down, the siren
/// told, and the model and effort the turn went out on named.
pub async fn answered(road: &ClaudeRoad<'_>, asked: &Asked, reply: Reply) -> Result<Reply, Refusal> {
  let ground = road.ground;
  let mapping = &asked.attempt;

  if let Some(upstream) = &reply.upstream {
    run(
      ground.db,
      r#"UPDATE "jobs" SET "execution_upstream" = $1 WHERE "unit" = $2 AND "finished_at" IS NULL"#,
      vec![upstream.as_str().into(), road.unit.as_str().into()],
    )
    .await?;
  }

  let scope = format!("{JOB_WORK_KEY}:{}", road.worker());
  let effort = mapping.effort.clone().unwrap_or_default();
  let effort = if asked.provider == CODEX {
    siren::tell(
      ground.siren.as_ref(),
      &siren::codex_events(JOB_WORK_KEY, &scope, &road.cwd, &reply, &mapping.model),
    )
    .await;
    Some(effort)
  } else {
    siren::tell(
      ground.siren.as_ref(),
      &[siren::spend_event(JOB_WORK_KEY, &scope, &road.cwd, &reply, &mapping.model)],
    )
    .await;
    Some(effort).filter(|effort| !effort.is_empty())
  };

  Ok(Reply {
    model: Some(mapping.model.clone()),
    effort,
    ..reply
  })
}
