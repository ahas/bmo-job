//! Every route the service serves, as the OpenAPI document states it: its method and path, its query and
//! body, what it answers and the codes it can refuse with.

use schemars::generate::SchemaGenerator;
use schemars::{JsonSchema, Schema};
use serde_json::Value;

use crate::codes::Code;
use crate::events::{Event, EventQuery, NewEvent};
use crate::health::Health;
use crate::items::{Item, ItemChange, ItemQuery, NewItem};
use crate::label_values::{LabelValues, LabelValuesBody, LabelValuesQuery};
use crate::leases::{Lease, LeaseAsk, LeaseQuery, LeaseRenewal, LeaseWait};
use crate::notes::{NewNote, Note, WaitQuery};
use crate::page::{Page, PageQuery};
use crate::questions::{NewQuestion, NewReply, Question, QuestionChange, QuestionQuery, Reply};
use crate::queues::{Queue, QueueChange, QueueQuery};
use crate::reports::{NewReport, Report};
use crate::reviews::{NewReview, Review, ReviewQuery};
use crate::scopes::{ScopeDeletion, ScopeHolding};
use crate::subscriptions::{NewSubscription, Subscription, SubscriptionChange, SubscriptionQuery};
use crate::work_orders::{Task, TaskChange, TaskQuery, WorkOrder, WorkOrderBody, WorkOrderChange};

/// A schema the generator makes, its nested types referred to where the generator keeps its definitions.
pub type SchemaOf = fn(&mut SchemaGenerator) -> Schema;

/// An HTTP method a route answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
  Get,
  Post,
  Put,
  Patch,
  Delete,
}

impl Method {
  /// The method as the OpenAPI document keys it, in lower case.
  pub fn key(self) -> &'static str {
    match self {
      Self::Get => "get",
      Self::Post => "post",
      Self::Put => "put",
      Self::Patch => "patch",
      Self::Delete => "delete",
    }
  }

  /// The method as HTTP writes it.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Get => "GET",
      Self::Post => "POST",
      Self::Put => "PUT",
      Self::Patch => "PATCH",
      Self::Delete => "DELETE",
    }
  }
}

/// One answer a route gives on success.
pub struct Answer {
  pub status: u16,
  pub description: &'static str,
  /// The body's schema by media type; empty for an answer with no body.
  pub content: Vec<(&'static str, SchemaOf)>,
}

/// One route.
pub struct Route {
  pub method: Method,
  pub path: &'static str,
  pub operation_id: &'static str,
  pub tag: &'static str,
  pub summary: &'static str,
  /// The query's parameters, as the properties of one object.
  pub query: Option<SchemaOf>,
  pub body: Option<SchemaOf>,
  pub answers: Vec<Answer>,
  pub codes: Vec<Code>,
}

impl Route {
  /// The route as `METHOD /path`.
  pub fn named(&self) -> String {
    format!("{} {}", self.method.as_str(), self.path)
  }
}

fn object<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
  T::json_schema(generator)
}

fn referred<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
  generator.subschema_for::<T>()
}

fn text_stream(_: &mut SchemaGenerator) -> Schema {
  Schema::try_from(serde_json::json!({ "type": "string" })).expect("a string's schema")
}

fn json<T: JsonSchema>(status: u16, description: &'static str) -> Answer {
  Answer {
    status,
    description,
    content: vec![("application/json", referred::<T>)],
  }
}

fn empty(description: &'static str) -> Answer {
  Answer {
    status: 204,
    description,
    content: Vec::new(),
  }
}

struct Building {
  method: Method,
  path: &'static str,
  operation_id: &'static str,
  tag: &'static str,
  summary: &'static str,
}

impl Building {
  fn with(
    self,
    query: Option<SchemaOf>,
    body: Option<SchemaOf>,
    answers: Vec<Answer>,
    codes: &[Code],
  ) -> Route {
    let mut codes = codes.to_vec();
    codes.insert(0, Code::Unauthorized);
    Route {
      method: self.method,
      path: self.path,
      operation_id: self.operation_id,
      tag: self.tag,
      summary: self.summary,
      query,
      body,
      answers,
      codes,
    }
  }
}

fn route(
  method: Method,
  path: &'static str,
  operation_id: &'static str,
  tag: &'static str,
  summary: &'static str,
) -> Building {
  Building {
    method,
    path,
    operation_id,
    tag,
    summary,
  }
}

/// Every route, in the order the OpenAPI document lists them.
pub fn routes() -> Vec<Route> {
  use Code::*;
  use Method::*;

  vec![
    route(Get, "/health", "listHealth", "health", "Readiness and version.").with(
      None,
      None,
      vec![json::<Health>(200, "Readiness and version.")],
      &[],
    ),
    route(
      Get,
      "/events",
      "listEvents",
      "events",
      "The service's one event stream (SSE), or a page of it with Accept: application/json; with subscriptionId, the \
       events that subscription matches, the stream form of push.",
    )
    .with(
      Some(object::<EventQuery>),
      None,
      vec![Answer {
        status: 200,
        description: "The event stream, or a page of it.",
        content: vec![("text/event-stream", text_stream), ("application/json", referred::<Page<Event>>)],
      }],
      &[BadRequest, NotFound, Conflict, Gone],
    ),
    route(Post, "/events", "createEvents", "events", "A process of the service's own publishes an event.").with(
      None,
      Some(referred::<NewEvent>),
      vec![json::<Event>(201, "The event published.")],
      &[BadRequest],
    ),
    route(
      Get,
      "/items",
      "listItems",
      "items",
      "Items by state, queue, assignee, scope, label, ids, the item they were raised from or words, in queue order.",
    )
    .with(Some(object::<ItemQuery>), None, vec![json::<Page<Item>>(200, "A page of items.")], &[BadRequest]),
    route(Post, "/items", "createItems", "items", "Raise an item: a draft, or open where it carries an unattended label.").with(
      None,
      Some(referred::<NewItem>),
      vec![json::<Item>(201, "The item raised.")],
      &[BadRequest, UnknownItem, LabelValueUnknown],
    ),
    route(Get, "/items/{itemId}", "readItemsByItemId", "items", "One item: brief, state, approvals, queue place.")
      .with(None, None, vec![json::<Item>(200, "The item.")], &[NotFound]),
    route(
      Patch,
      "/items/{itemId}",
      "changeItemsByItemId",
      "items",
      "Change an item: its state (approve, hold, release, done, drop, merge), priority, position, assignee, brief, \
       labels, and whether notices of it are switched off. An approval is recorded under the caller the token \
       names; it opens a draft only where that caller is an approver, and is otherwise a recommendation that \
       leaves the item a draft. A hold carries its reason in the same change, which the item keeps and writes as \
       its held report.",
    )
    .with(
      None,
      Some(referred::<ItemChange>),
      vec![json::<Item>(200, "The item changed.")],
      &[
        BadRequest,
        ReasonRequired,
        ReasonWithoutHold,
        NotFound,
        Conflict,
        ItemFrozen,
        UnknownItem,
        LabelValueUnknown,
      ],
    ),
    route(
      Get,
      "/items/{itemId}/notes",
      "listItemsNotesByItemId",
      "items",
      "An item's notes after a cursor; waitMs holds the answer open until one is appended.",
    )
    .with(
      Some(object::<WaitQuery>),
      None,
      vec![json::<Page<Note>>(200, "A page of notes.")],
      &[BadRequest, NotFound],
    ),
    route(Post, "/items/{itemId}/notes", "createItemsNotesByItemId", "items", "Append a note to an item.").with(
      None,
      Some(referred::<NewNote>),
      vec![json::<Note>(201, "The note appended.")],
      &[BadRequest, NotFound],
    ),
    route(
      Delete,
      "/items/{itemId}/notes/{noteId}",
      "deleteItemsNotesByItemIdNoteId",
      "items",
      "Take an appended note back.",
    )
    .with(None, None, vec![empty("The note is taken back.")], &[NotFound]),
    route(Get, "/items/{itemId}/reports", "listItemsReportsByItemId", "items", "An item's reports, oldest first.")
      .with(
        Some(object::<PageQuery>),
        None,
        vec![json::<Page<Report>>(200, "A page of reports.")],
        &[BadRequest, NotFound],
      ),
    route(
      Post,
      "/items/{itemId}/reports",
      "createItemsReportsByItemId",
      "items",
      "Append a report to an item: progress, final, or why it is held.",
    )
    .with(None, Some(referred::<NewReport>), vec![json::<Report>(201, "The report appended.")], &[BadRequest, NotFound]),
    route(Get, "/items/{itemId}/reviews", "listItemsReviewsByItemId", "items", "An item's review, oldest first.")
      .with(
        Some(object::<ReviewQuery>),
        None,
        vec![json::<Page<Review>>(200, "A page of review entries.")],
        &[BadRequest, NotFound],
      ),
    route(
      Post,
      "/items/{itemId}/reviews",
      "createItemsReviewsByItemId",
      "items",
      "Append an entry to an item's review: a report, a question, an answer, a rejection, a pass, an escalation, a ruling, or the work accepted.",
    )
    .with(None, Some(referred::<NewReview>), vec![json::<Review>(201, "The entry appended.")], &[BadRequest, NotFound]),
    route(Get, "/items/{itemId}/work-order", "listItemsWorkOrderByItemId", "items", "An item's work order.").with(
      None,
      None,
      vec![json::<WorkOrder>(200, "The work order.")],
      &[NotFound],
    ),
    route(Put, "/items/{itemId}/work-order", "putItemsWorkOrderByItemId", "items", "Save an item's work order whole.")
      .with(
        None,
        Some(referred::<WorkOrderBody>),
        vec![
          json::<WorkOrder>(200, "The work order replaced."),
          json::<WorkOrder>(201, "The work order made."),
        ],
        &[BadRequest, NotFound],
      ),
    route(
      Patch,
      "/items/{itemId}/work-order",
      "changeItemsWorkOrderByItemId",
      "items",
      "Edit, resume or reopen a work order.",
    )
    .with(
      None,
      Some(referred::<WorkOrderChange>),
      vec![json::<WorkOrder>(200, "The work order changed.")],
      &[BadRequest, NotFound, Conflict],
    ),
    route(
      Get,
      "/items/{itemId}/work-order/tasks",
      "listItemsWorkOrderTasksByItemId",
      "items",
      "A work order's tasks; current=true for the stage under way.",
    )
    .with(
      Some(object::<TaskQuery>),
      None,
      vec![json::<Page<Task>>(200, "A page of tasks.")],
      &[BadRequest, NotFound],
    ),
    route(
      Get,
      "/items/{itemId}/work-order/tasks/{taskId}",
      "readItemsWorkOrderTasksByItemIdTaskId",
      "items",
      "One task.",
    )
    .with(None, None, vec![json::<Task>(200, "The task.")], &[NotFound]),
    route(
      Patch,
      "/items/{itemId}/work-order/tasks/{taskId}",
      "changeItemsWorkOrderTasksByItemIdTaskId",
      "items",
      "Move a task: current, done, skipped, waiting again, or to another stage.",
    )
    .with(
      None,
      Some(referred::<TaskChange>),
      vec![json::<Task>(200, "The task moved.")],
      &[BadRequest, NotFound, Conflict],
    ),
    route(Get, "/label-values", "listLabelValues", "label-values", "Every label key's list of values, in key order.")
      .with(
        Some(object::<LabelValuesQuery>),
        None,
        vec![json::<Page<LabelValues>>(200, "A page of lists.")],
        &[BadRequest],
      ),
    route(Get, "/label-values/{key}", "readLabelValuesByKey", "label-values", "The values one label key may take.")
      .with(None, None, vec![json::<LabelValues>(200, "The list.")], &[NotFound]),
    route(
      Put,
      "/label-values/{key}",
      "putLabelValuesByKey",
      "label-values",
      "Publish the values a label key may take, replacing the list that stood; an item then takes only a value on \
       it under that key.",
    )
    .with(
      None,
      Some(referred::<LabelValuesBody>),
      vec![json::<LabelValues>(200, "The list as kept.")],
      &[BadRequest],
    ),
    route(
      Delete,
      "/label-values/{key}",
      "deleteLabelValuesByKey",
      "label-values",
      "Remove a label key's list of values; the key is checked no more.",
    )
    .with(None, None, vec![empty("The list is removed.")], &[NotFound]),
    route(Get, "/queues", "listQueues", "queues", "Every queue with its state and holds.").with(
      Some(object::<QueueQuery>),
      None,
      vec![json::<Page<Queue>>(200, "A page of queues.")],
      &[BadRequest],
    ),
    route(Get, "/queues/{queueName}", "readQueuesByQueueName", "queues", "One queue.").with(
      None,
      None,
      vec![json::<Queue>(200, "The queue.")],
      &[NotFound],
    ),
    route(
      Patch,
      "/queues/{queueName}",
      "changeQueuesByQueueName",
      "queues",
      "Pause or open a queue, or add or release a hold on it.",
    )
    .with(
      None,
      Some(referred::<QueueChange>),
      vec![json::<Queue>(200, "The queue changed.")],
      &[BadRequest, NotFound],
    ),
    route(
      Post,
      "/leases",
      "createLeases",
      "leases",
      "Take the next open item of a queue for a holder, for ttlMs; 204 when nothing waits, waitMs to wait for one. \
       With itemId, the item a push offered: 409 conflict when another holder took it first.",
    )
    .with(
      Some(object::<LeaseWait>),
      Some(referred::<LeaseAsk>),
      vec![json::<Lease>(201, "The lease taken."), empty("Nothing waits to be leased.")],
      &[BadRequest, Conflict, UnknownItem, QueueNotOpen],
    ),
    route(Get, "/leases", "listLeases", "leases", "Leases standing now, by holder or queue.").with(
      Some(object::<LeaseQuery>),
      None,
      vec![json::<Page<Lease>>(200, "A page of leases.")],
      &[BadRequest],
    ),
    route(Get, "/leases/{leaseId}", "readLeasesByLeaseId", "leases", "One lease and its item.").with(
      None,
      None,
      vec![json::<Lease>(200, "The lease.")],
      &[NotFound, Gone],
    ),
    route(Patch, "/leases/{leaseId}", "changeLeasesByLeaseId", "leases", "Renew a lease for another ttlMs, and set what it holds fixed.").with(
      None,
      Some(referred::<LeaseRenewal>),
      vec![json::<Lease>(200, "The lease renewed.")],
      &[BadRequest, NotFound, Gone],
    ),
    route(
      Delete,
      "/leases/{leaseId}",
      "deleteLeasesByLeaseId",
      "leases",
      "Give a lease back; the item is open again.",
    )
    .with(None, None, vec![empty("The lease is given back.")], &[NotFound, Gone]),
    route(Get, "/questions", "listQuestions", "questions", "Questions by scope, item, addressee and state.").with(
      Some(object::<QuestionQuery>),
      None,
      vec![json::<Page<Question>>(200, "A page of questions.")],
      &[BadRequest],
    ),
    route(
      Post,
      "/questions",
      "createQuestions",
      "questions",
      "Ask a question about an item, with choices or open.",
    )
    .with(
      None,
      Some(referred::<NewQuestion>),
      vec![json::<Question>(201, "The question asked.")],
      &[BadRequest, UnknownItem, NotAChoice, NoChoices],
    ),
    route(
      Get,
      "/questions/{questionId}",
      "readQuestionsByQuestionId",
      "questions",
      "One question, its state and its due time.",
    )
    .with(None, None, vec![json::<Question>(200, "The question.")], &[NotFound]),
    route(
      Patch,
      "/questions/{questionId}",
      "changeQuestionsByQuestionId",
      "questions",
      "Pause or start a question's clock again, withdraw it or close it unsent, put it again in other words, move what its asker was handed, or put a question the supervisor holds to a person.",
    )
    .with(
      None,
      Some(referred::<QuestionChange>),
      vec![json::<Question>(200, "The question changed.")],
      &[BadRequest, NotTheAddressee, NotFound, Conflict, NotAChoice, NoChoices, NoRecommendation],
    ),
    route(
      Get,
      "/questions/{questionId}/replies",
      "listQuestionsRepliesByQuestionId",
      "questions",
      "A question's replies; waitMs holds the answer open until one comes.",
    )
    .with(
      Some(object::<WaitQuery>),
      None,
      vec![json::<Page<Reply>>(200, "A page of replies.")],
      &[BadRequest, NotFound],
    ),
    route(
      Post,
      "/questions/{questionId}/replies",
      "createQuestionsRepliesByQuestionId",
      "questions",
      "Settle a question with a choice or an answer, or add a word on it.",
    )
    .with(
      None,
      Some(referred::<NewReply>),
      vec![json::<Reply>(201, "The reply given.")],
      &[BadRequest, NotTheAddressee, NotFound, Conflict, NotAChoice],
    ),
    route(Get, "/subscriptions", "listSubscriptions", "subscriptions", "Subscriptions, by holder or state.").with(
      Some(object::<SubscriptionQuery>),
      None,
      vec![json::<Page<Subscription>>(200, "A page of subscriptions.")],
      &[BadRequest],
    ),
    route(
      Post,
      "/subscriptions",
      "createSubscriptions",
      "subscriptions",
      "Subscribe to be told of work: the event kinds, queues, scope and items to hear of, a url for webhook \
       delivery or none for the stream form, and ttlMs.",
    )
    .with(
      None,
      Some(referred::<NewSubscription>),
      vec![json::<Subscription>(201, "The subscription made.")],
      &[BadRequest],
    ),
    route(
      Get,
      "/subscriptions/{subscriptionId}",
      "readSubscriptionsBySubscriptionId",
      "subscriptions",
      "One subscription, its state and the last event delivered.",
    )
    .with(None, None, vec![json::<Subscription>(200, "The subscription.")], &[NotFound, Gone]),
    route(
      Patch,
      "/subscriptions/{subscriptionId}",
      "changeSubscriptionsBySubscriptionId",
      "subscriptions",
      "Renew a subscription for another ttlMs, change what it hears of, pause it, or set a failing one active \
       again.",
    )
    .with(
      None,
      Some(referred::<SubscriptionChange>),
      vec![json::<Subscription>(200, "The subscription changed.")],
      &[BadRequest, NotFound, Conflict, Gone],
    ),
    route(
      Delete,
      "/subscriptions/{subscriptionId}",
      "deleteSubscriptionsBySubscriptionId",
      "subscriptions",
      "Stop being told; nothing more is delivered.",
    )
    .with(None, None, vec![empty("The subscription is ended.")], &[NotFound, Gone]),
    route(
      Get,
      "/scopes/{scope}",
      "readScopesByScope",
      "scopes",
      "What the service holds under a scope, counted by resource.",
    )
    .with(None, None, vec![json::<ScopeHolding>(200, "What is held.")], &[NotFound]),
    route(
      Delete,
      "/scopes/{scope}",
      "deleteScopesByScope",
      "scopes",
      "Let go of everything held under a scope; dryRun=true answers what would go.",
    )
    .with(
      Some(object::<ScopeDeletion>),
      None,
      vec![json::<ScopeHolding>(200, "What went, or would go.")],
      &[BadRequest, NotFound],
    ),
  ]
}

/// The type a path parameter is written in: an integer key where its name ends in `Id`, a name otherwise.
pub(crate) fn path_parameter_schema(name: &str) -> Value {
  if name.ends_with("Id") {
    serde_json::json!({ "type": "integer", "format": "int64" })
  } else {
    serde_json::json!({ "type": "string" })
  }
}
