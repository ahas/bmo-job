//! The harness tools: one for each route the draft names, each a thin call of one method of the client. The
//! MCP server lists them and the OpenAPI document states them under `x-tools`, both from this one table
//! (docs/api.md, "The tools").

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bmo_job_client::{
  ClientFailure, Item, ItemChange, ItemQuery, Lease, LeaseAsk, LeaseRenewal, LeaseWait, NewItem, NewNote,
  NewQuestion, NewReport, NewSubscription, Note, Page, Question, Queue, QueueQuery, Reply, Report,
  Subscription, Task, TaskQuery, WaitQuery, WorkOrder,
};
use schemars::generate::SchemaGenerator;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::harness::Harness;

/// What a tool's call came to short of the route's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolFailure {
  /// The arguments are not the shape the tool takes.
  Arguments(String),
  /// The call reached no answer of the route's shape.
  Client(ClientFailure),
}

impl ToolFailure {
  /// The failure as the tool error's content: the status, code and sentence the service answered, or the
  /// sentence saying why nothing answered.
  pub fn content(&self) -> Value {
    match self {
      Self::Arguments(why) => json!({ "code": "bad-request", "error": why }),
      Self::Client(ClientFailure::Answered {
        status,
        answer: Some(answer),
        ..
      }) => json!({ "status": status, "code": answer.code, "error": answer.error }),
      Self::Client(failure @ ClientFailure::Answered { status, .. }) => {
        json!({ "status": status, "error": failure.to_string() })
      }
      Self::Client(failure) => json!({ "error": failure.to_string() }),
    }
  }
}

/// A tool's answer: the route's answer as JSON, or why there is none.
pub type Answered = Pin<Box<dyn Future<Output = Result<Value, ToolFailure>> + Send>>;

type Call = Box<dyn Fn(Arc<Harness>, Value) -> Answered + Send + Sync>;

/// One tool: its name, what it does, the route it calls, its input and output, and the call itself.
pub struct JobTool {
  pub name: &'static str,
  pub description: &'static str,
  /// The route the tool calls, as its method and path.
  pub route: &'static str,
  input: fn(&mut SchemaGenerator) -> Schema,
  output: fn(&mut SchemaGenerator) -> Schema,
  call: Call,
}

impl JobTool {
  /// The tool's input, its nested types referred to where `generator` keeps its definitions.
  pub fn input(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.input)(generator)
  }

  /// The tool's output, referred to where `generator` keeps its definitions.
  pub fn output(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.output)(generator)
  }

  /// Calls the tool's route for the harness, with the tool's arguments.
  pub fn call(&self, harness: Arc<Harness>, arguments: Value) -> Answered {
    (self.call)(harness, arguments)
  }
}

fn tool<I, O, F, Fut>(
  name: &'static str,
  description: &'static str,
  route: &'static str,
  call: F,
) -> JobTool
where
  I: DeserializeOwned + JsonSchema,
  O: Serialize + JsonSchema,
  F: Fn(Arc<Harness>, I) -> Fut + Send + Sync + 'static,
  Fut: Future<Output = Result<O, ClientFailure>> + Send + 'static,
{
  JobTool {
    name,
    description,
    route,
    input: I::json_schema,
    output: SchemaGenerator::subschema_for::<O>,
    call: Box::new(move |harness, arguments| {
      let input = match serde_json::from_value::<I>(arguments) {
        Ok(input) => input,
        Err(error) => {
          let why = format!("The arguments are not the shape this tool takes: {error}.");
          return Box::pin(std::future::ready(Err(ToolFailure::Arguments(why))));
        }
      };
      let answer = call(harness, input);
      Box::pin(async move {
        let answer = answer.await.map_err(ToolFailure::Client)?;
        Ok(serde_json::to_value(answer).expect("a route's answer always renders as JSON"))
      })
    }),
  }
}

/// An item named by its key.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemKey {
  pub item_id: i64,
}

/// A lease named by its key.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaseKey {
  pub lease_id: i64,
}

/// A question named by its key.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionKey {
  pub question_id: i64,
}

/// A subscription named by its key.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionKey {
  pub subscription_id: i64,
}

/// A route's path key and the rest of its input together, as one tool's input.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Keyed<K, B> {
  #[serde(flatten)]
  pub key: K,
  #[serde(flatten)]
  pub rest: B,
}

/// A lease asked for, and how long to wait for an item where none is open.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LeaseNext {
  #[serde(flatten)]
  pub wait: LeaseWait,
  #[serde(flatten)]
  pub ask: LeaseAsk,
}

/// The lease taken, or null where nothing waited.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeaseTaken {
  pub lease: Option<Lease>,
}

/// A route that answers nothing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Nothing {}

/// Every tool, in the order the draft lists them.
pub fn tools() -> Vec<JobTool> {
  vec![
    tool(
      "job_items",
      "Items by state, queue, assignee, scope, label, ids or words, in queue order.",
      "GET /items",
      |harness, query: ItemQuery| async move { harness.client.items(&query).await },
    ),
    tool::<NewItem, Item, _, _>(
      "job_item_create",
      "Raise an item: a draft, or open where it carries an unattended label.",
      "POST /items",
      |harness, item| async move { harness.client.create_item(&item).await },
    ),
    tool::<ItemKey, Item, _, _>(
      "job_item",
      "One item: brief, state, approvals, queue place.",
      "GET /items/{itemId}",
      |harness, key| async move { harness.client.item(key.item_id).await },
    ),
    tool::<Keyed<ItemKey, ItemChange>, Item, _, _>(
      "job_item_update",
      "Change an item: its state (approve, hold, release, done, drop, merge), priority, position, assignee, brief. \
       An approval takes no name of who approves: it is recorded under the caller whose token this server holds, \
       and opens a draft only where the deployment marks that caller as an approver; otherwise it is a \
       recommendation, recorded for an approver to see, and the item stays a draft.",
      "PATCH /items/{itemId}",
      |harness, input| async move { harness.client.change_item(input.key.item_id, &input.rest).await },
    ),
    tool::<Keyed<ItemKey, WaitQuery>, Page<Note>, _, _>(
      "job_notes",
      "An item's notes after a cursor; waitMs holds the answer open until one is appended.",
      "GET /items/{itemId}/notes",
      |harness, input| async move { harness.client.notes(input.key.item_id, &input.rest).await },
    ),
    tool::<Keyed<ItemKey, NewNote>, Note, _, _>(
      "job_note_add",
      "Append a note to an item.",
      "POST /items/{itemId}/notes",
      |harness, input| async move { harness.client.add_note(input.key.item_id, &input.rest).await },
    ),
    tool::<Keyed<ItemKey, NewReport>, Report, _, _>(
      "job_report",
      "Append a report to an item: progress, final, or why it is held.",
      "POST /items/{itemId}/reports",
      |harness, input| async move { harness.client.add_report(input.key.item_id, &input.rest).await },
    ),
    tool::<ItemKey, WorkOrder, _, _>(
      "job_work_order",
      "An item's work order.",
      "GET /items/{itemId}/work-order",
      |harness, key| async move { harness.client.work_order(key.item_id).await },
    ),
    tool::<Keyed<ItemKey, TaskQuery>, Page<Task>, _, _>(
      "job_tasks",
      "A work order's tasks; current=true for the stage under way.",
      "GET /items/{itemId}/work-order/tasks",
      |harness, input| async move { harness.client.tasks(input.key.item_id, &input.rest).await },
    ),
    tool::<QueueQuery, Page<Queue>, _, _>(
      "job_queues",
      "Every queue with its state and holds.",
      "GET /queues",
      |harness, query| async move { harness.client.queues(&query).await },
    ),
    tool::<LeaseNext, LeaseTaken, _, _>(
      "job_lease_next",
      "Take the next open item of a queue for a holder, for ttlMs, or with itemId the item a push named; lease is \
       null when nothing waits, and waitMs waits for one. The server renews the lease while the session lasts and \
       gives it back when the session ends.",
      "POST /leases",
      |harness, input| async move {
        let lease = harness.client.take_lease(&input.ask, &input.wait).await?;
        if let Some(lease) = &lease {
          harness.keep_lease(lease.id, lease.ttl_ms);
        }
        Ok(LeaseTaken { lease })
      },
    ),
    tool::<Keyed<LeaseKey, LeaseRenewal>, Lease, _, _>(
      "job_lease_renew",
      "Renew a lease for another ttlMs.",
      "PATCH /leases/{leaseId}",
      |harness, input| async move {
        let lease = harness.client.renew_lease(input.key.lease_id, &input.rest).await?;
        harness.renewed_lease(lease.id, lease.ttl_ms);
        Ok(lease)
      },
    ),
    tool::<LeaseKey, Nothing, _, _>(
      "job_lease_release",
      "Give a lease back; the item is open again.",
      "DELETE /leases/{leaseId}",
      |harness, key| async move {
        harness.client.release_lease(key.lease_id).await?;
        harness.let_lease_go(key.lease_id);
        Ok(Nothing {})
      },
    ),
    tool::<NewQuestion, Question, _, _>(
      "job_question_ask",
      "Ask a question about an item, with choices or open; a recommendation shown to whoever answers, and why.",
      "POST /questions",
      |harness, question| async move { harness.client.ask(&question).await },
    ),
    tool::<QuestionKey, Question, _, _>(
      "job_question",
      "One question, its state and its due time.",
      "GET /questions/{questionId}",
      |harness, key| async move { harness.client.question(key.question_id).await },
    ),
    tool::<Keyed<QuestionKey, WaitQuery>, Page<Reply>, _, _>(
      "job_replies",
      "A question's replies; waitMs holds the answer open until one comes.",
      "GET /questions/{questionId}/replies",
      |harness, input| async move { harness.client.replies(input.key.question_id, &input.rest).await },
    ),
    tool::<NewSubscription, Subscription, _, _>(
      "job_subscribe",
      "Subscribe to be told of work: the event kinds, queues, scope and items to hear of, a url for webhook delivery \
       or none to be told in this session as MCP notifications, and ttlMs. The server renews it while the session \
       lasts and deletes it when the session ends.",
      "POST /subscriptions",
      |harness, subscription| async move {
        let made = harness.client.subscribe(&subscription).await?;
        harness.keep_subscription(&made);
        Ok(made)
      },
    ),
    tool::<SubscriptionKey, Nothing, _, _>(
      "job_unsubscribe",
      "Stop being told; nothing more is delivered.",
      "DELETE /subscriptions/{subscriptionId}",
      |harness, key| async move {
        harness.client.unsubscribe(key.subscription_id).await?;
        harness.let_subscription_go(key.subscription_id);
        Ok(Nothing {})
      },
    ),
  ]
}
