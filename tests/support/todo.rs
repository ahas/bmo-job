//! What the TODO verb's tests stand on (docs/verbs.md, "The TODO's edits and holds"; docs/rows.md): a server on
//! the ported verbs over a scratch database, and a stand reaching it as a shell's does, the verb run on that
//! stand by whoever the test says — a person, the agent's turn, or a job — each call read the way the command
//! line reads it, and each item read back through the server's own readings.

use std::path::Path;

use bmo_job::verbs::Built;
use bmo_job::verbs::stand::{JobProcess, Stand};
use bmo_job::verbs::todo::{Beside, ShellHands, TodoArgs, providers, rows_ground, rows_units, todo_verb};
use clap::Parser;
use job_base::Settings;
use queue_rows::Filer;
use serde_json::{Value, json};

use super::brain;
use super::server::{Served, host_text, settings_text};

/// The owner's sender and private chat, as the receiver stand-in names them.
pub const OWNER_SENDER: i64 = 5;
pub const OWNER_CHAT: i64 = 100;
/// A group room, and a member of it who is not the owner.
pub const ROOM: i64 = -1001;
pub const MEMBER: i64 = 777;
/// The agent the deployment's settings name.
pub const AGENT: &str = "agent";

/// The three parts the agent writes in its own words, which every filing out of a conversation carries.
pub const OWN_WORDS: [&str; 6] = [
  "--what",
  "what the work is",
  "--do",
  "what has to be done",
  "--conclusion",
  "that is what was settled",
];

#[derive(Parser)]
#[command(name = "todo", disable_help_flag = true)]
struct Line {
  #[command(flatten)]
  args: TodoArgs,
}

/// What one call came to: 0 with the answers it gave, 1 with the code and sentence it was refused with, 2
/// where its words did not read.
#[derive(Debug)]
pub struct Ran {
  pub code: i32,
  pub answers: Vec<Value>,
  pub refused: String,
  pub said: String,
}

impl Ran {
  /// The one answer of a call that gave one.
  pub fn answer(&self) -> &Value {
    assert_eq!(self.code, 0, "refused {}: {}", self.refused, self.said);
    &self.answers[0]
  }
}

/// Who calls: a person, the agent's turn, or a job by its run's name, a middle step of a work order or not.
#[derive(Clone, Copy, Default)]
pub struct As<'a> {
  pub job: Option<&'a str>,
  pub step: bool,
  pub agent: Option<&'a str>,
}

/// A word standing on an item.
#[derive(Debug)]
pub struct Note {
  pub id: i64,
  pub author: String,
  pub author_id: Option<i64>,
  pub text: String,
  pub job: Option<i64>,
  pub delivered: bool,
  pub kind: Option<String>,
}

/// The server, the stand a shell reaches it from, and bmo-brain stood in for the stand.
pub struct Bench {
  pub served: Served<Built>,
  pub brain: brain::Stub,
  pub stand: Stand,
}

impl Bench {
  pub async fn new() -> Self {
    Self::worded(None).await
  }

  /// The bench whose deployment reads the owner's wording document from `wording`, where one is given.
  pub async fn worded(wording: Option<&str>) -> Self {
    let served = Served::<Built>::ported().await;
    let brain = brain::Stub::start().await;
    let scratch = served.scratch.path();
    let url = format!("sqlite://{}?mode=rwc", scratch.join("job.db").display());
    let text =
      (settings_text(&url, &served.agency.address(), &served.gate.address(), &served.receiver.address())
        + &host_text(scratch, &served.testbed.program()))
        .replace("address = \"http://127.0.0.1:1/\"", &format!("address = \"{}\"", served.base))
        .replace("brain = \"http://127.0.0.1:1/\"", &format!("brain = \"{}\"", brain.address()));
    let text = match wording {
      None => text,
      Some(wording) => {
        let document = scratch.join("wording.md");

        std::fs::write(&document, wording).expect("the wording document is written");
        text.replace(
          "owner_chat = 100\n",
          &format!(
            "owner_chat = 100\nwording = {}\n",
            serde_json::to_string(&document.display().to_string()).unwrap()
          ),
        )
      }
    };

    brain.answer("GET /memories", 200, json!({ "answered": true, "memories": [] }));
    brain.answer("POST /memory/add", 200, json!({ "answered": true, "lines": [] }));

    for (route, body) in [
      ("GET /executions", json!([])),
      (
        "GET /providers",
        json!([{ "name": "claude-code", "enabled": true, "registered": false, "coworker": true, "lines": ["worker"] }]),
      ),
      ("GET /tiers", json!([])),
      ("GET /pool/roles", json!([])),
      ("GET /pool/requests", json!([])),
      ("GET /machine/room", json!({ "held": [], "jobs": 0, "browser_jobs": 0, "browsers": 0 })),
      ("DELETE /executions/{id}/cancel", json!({ "dropped": [] })),
      ("GET /agents/{agent}/rest", json!({ "resting": false })),
      ("GET /nodes", json!([])),
    ] {
      served.agency.answer(route, 200, body);
    }

    for route in ["POST /executions/{id}/cancel", "POST /workers/{worker}/bench", "POST /turn-records"] {
      served.agency.answer_empty(route, 204);
    }

    served
      .receiver
      .answer("GET /owner", 200, json!({ "sender": OWNER_SENDER, "chat": OWNER_CHAT }));
    served.receiver.answer("POST /instructions", 201, json!({ "id": 1 }));
    served.testbed.answer("pending-list", "[]", "", 0);

    let settings = Settings::read(&text).expect("the bench's settings read");
    let stand = Stand::of(&settings).await.expect("the stand is built");

    Self {
      served,
      brain,
      stand,
    }
  }

  /// A call as a person at a shell types it.
  pub async fn todo(&self, args: &[&str]) -> Ran {
    self.todo_as(As::default(), args).await
  }

  /// A call as the agent's own turn makes it.
  pub async fn agent_todo(&self, args: &[&str]) -> Ran {
    self
      .todo_as(
        As {
          agent: Some(AGENT),
          ..As::default()
        },
        args,
      )
      .await
  }

  /// A call as the job running as `run` makes it.
  pub async fn job_todo(&self, run: &str, args: &[&str]) -> Ran {
    self
      .todo_as(
        As {
          job: Some(run),
          ..As::default()
        },
        args,
      )
      .await
  }

  /// A call by whoever the test says.
  pub async fn todo_as(&self, who: As<'_>, args: &[&str]) -> Ran {
    let line = match Line::try_parse_from(std::iter::once("todo").chain(args.iter().copied())) {
      Ok(line) => line,
      Err(refused) => {
        return Ran {
          code: 2,
          answers: vec![],
          refused: String::new(),
          said: refused.to_string(),
        };
      }
    };
    let process = JobProcess {
      run_name: who.job.map(str::to_owned),
      in_job: who.job.is_some(),
      work_order_step: who.step,
    };
    let filer = Filer {
      agent: who.agent,
      label: None,
    };
    let person = (who.job.is_none() && who.agent.is_none()).then_some("person");
    let units = rows_units(&self.stand);
    let hands = ShellHands { stand: &self.stand };
    let providers = providers(&self.stand).await.expect("the providers read");
    let ground = rows_ground(&self.stand, &process, filer, person, &units, &hands, &providers);
    let beside = Beside {
      source: &self.stand.ground.source,
      testbed: &self.stand.ground.testbed,
    };

    match todo_verb(&ground, &beside, &line.args, who.step).await {
      Ok(answers) => Ran {
        code: 0,
        answers: answers
          .iter()
          .map(|answer| serde_json::to_value(answer).expect("an answer writes"))
          .collect(),
        refused: String::new(),
        said: String::new(),
      },
      Err(refusal) => Ran {
        code: 1,
        answers: vec![],
        refused: refusal.code.to_owned(),
        said: refusal.error,
      },
    }
  }

  /// A statement run on the database.
  pub async fn run(&self, sql: &str, values: Vec<sea_orm::Value>) {
    self.served.seed(sql, values).await;
  }

  /// The first column of the one row a statement reads, as a number, none where it is null or no row.
  pub async fn number(&self, sql: &str, values: Vec<sea_orm::Value>) -> Option<i64> {
    job_base::one(self.served.db(), sql, values)
      .await
      .unwrap_or_else(|failed| panic!("{sql}: {failed}"))
      .and_then(|row| row.try_get_by_index::<Option<i64>>(0).unwrap())
  }

  /// The first column of the one row a statement reads, as text.
  pub async fn text(&self, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    job_base::one(self.served.db(), sql, values)
      .await
      .unwrap_or_else(|failed| panic!("{sql}: {failed}"))
      .and_then(|row| row.try_get_by_index::<Option<String>>(0).unwrap())
  }

  pub async fn count(&self, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    self.number(sql, values).await.unwrap_or(0)
  }

  /// A column of an item's row as a number.
  pub async fn field_number(&self, id: i64, column: &str) -> Option<i64> {
    self
      .number(&format!(r#"SELECT "{column}" FROM "todos" WHERE "id" = $1"#), vec![id.into()])
      .await
  }

  /// A column of an item's row as text.
  pub async fn field(&self, id: i64, column: &str) -> Option<String> {
    self
      .text(&format!(r#"SELECT "{column}" FROM "todos" WHERE "id" = $1"#), vec![id.into()])
      .await
  }

  pub async fn done(&self, id: i64) -> bool {
    self.field_number(id, "done").await == Some(1)
  }

  /// The newest item's number.
  pub async fn last(&self) -> i64 {
    self.number(r#"SELECT MAX("id") FROM "todos""#, vec![]).await.unwrap_or(0)
  }

  /// An item written straight into the table, approved, at the queue's end.
  pub async fn item(&self, title: &str, instruction: &str) -> i64 {
    self
      .run(
        r#"INSERT INTO "todos" ("title", "instruction", "requester", "requester_id", "chat_id", "approved", "priority", "status", "created_at")
           VALUES ($1, $2, 'Owner', $3, $4, 1, (SELECT COALESCE(MAX("priority"), 0) + 1 FROM "todos"), 'waiting', 1756800000000)"#,
        vec![title.into(), instruction.into(), OWNER_SENDER.into(), OWNER_CHAT.into()],
      )
      .await;
    self.last().await
  }

  /// A job running for an item, started now; its number.
  pub async fn job(&self, todo: i64, unit: &str) -> i64 {
    self
      .run(
        r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "started_by", "todo_id", "worker") VALUES ($1, $2, $3, 'work', $4, $5, 'worker')"#,
        vec![OWNER_CHAT.into(), unit.into(), bmo_job::now_ms().into(), format!("chat:{OWNER_CHAT}").into(), todo.into()],
      )
      .await;
    self.number(r#"SELECT MAX("id") FROM "jobs""#, vec![]).await.unwrap_or(0)
  }

  /// The message the receiver stand-in answers every room's message with.
  pub fn message(&self, sender: i64, name: &str, text: &str) {
    self.served.receiver.answer(
      "GET /rooms/{chat}/messages/{message}",
      200,
      json!({ "sender": sender, "sender_name": name, "sent_at": 1_756_800_000_000_i64, "text": text }),
    );
  }

  /// The words standing on an item, oldest first.
  pub async fn notes(&self, id: i64) -> Vec<Note> {
    let rows = job_base::all(
      self.served.db(),
      r#"SELECT "id", "author", "author_id", "text", "job_id", "delivered_at", "kind" FROM "todo_notes" WHERE "todo_id" = $1 AND "removed_at" IS NULL ORDER BY "id""#,
      vec![id.into()],
    )
    .await
    .expect("the notes read");

    rows
      .iter()
      .map(|row| Note {
        id: row.try_get("", "id").unwrap(),
        author: row.try_get("", "author").unwrap(),
        author_id: row.try_get("", "author_id").unwrap(),
        text: row.try_get("", "text").unwrap(),
        job: row.try_get("", "job_id").unwrap(),
        delivered: row.try_get::<Option<i64>>("", "delivered_at").unwrap().is_some(),
        kind: row.try_get("", "kind").unwrap(),
      })
      .collect()
  }

  /// An item closed straight in the table.
  pub async fn finish(&self, id: i64) {
    self
      .run(
        r#"UPDATE "todos" SET "done" = 1, "status" = $2, "done_at" = 1756800000000 WHERE "id" = $1"#,
        vec![id.into(), queue_rows::todos::DONE_STATUS.into()],
      )
      .await;
  }

  /// The facts the agent was told through bmo-receiver, in order.
  pub fn told(&self) -> Vec<Value> {
    self
      .served
      .receiver
      .calls()
      .into_iter()
      .filter(|call| call.route == "POST /instructions")
      .filter_map(|call| call.body)
      .map(|body| body["said"].clone())
      .collect()
  }

  /// An item as the server's reading of it shows it.
  pub async fn shown(&self, id: i64) -> Value {
    let (status, body) = self.served.get(&format!("/todos/{id}")).await;

    assert_eq!(status, 200, "{body}");
    body
  }

  /// The listing as the server reads it, under the query given.
  pub async fn listed(&self, query: &str) -> Value {
    let (status, body) = self.served.get(&format!("/todos{query}")).await;

    assert_eq!(status, 200, "{body}");
    body
  }

  /// The checkout the verb reads commits in.
  pub fn source(&self) -> &Path {
    &self.stand.ground.source
  }
}
