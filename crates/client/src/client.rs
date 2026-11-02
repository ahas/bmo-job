//! The client: one call for every route the service serves.

use std::time::Duration;

use futures_util::{Stream, StreamExt};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use sse_stream::SseStream;
use url::Url;

use crate::codes::ErrorBody;
use crate::events::{Event, EventQuery, NewEvent};
use crate::health::Health;
use crate::items::{Item, ItemChange, ItemQuery, NewItem};
use crate::leases::{Lease, LeaseAsk, LeaseQuery, LeaseRenewal, LeaseWait};
use crate::notes::{NewNote, Note, WaitQuery};
use crate::page::{Page, PageQuery};
use crate::questions::{NewQuestion, NewReply, Question, QuestionChange, QuestionQuery, Reply};
use crate::queues::{Queue, QueueChange, QueueQuery};
use crate::reports::{NewReport, Report};
use crate::scopes::{ScopeDeletion, ScopeHolding};
use crate::subscriptions::{NewSubscription, Subscription, SubscriptionChange, SubscriptionQuery};
use crate::work_orders::{Task, TaskChange, TaskQuery, WorkOrder, WorkOrderBody, WorkOrderChange};

/// How often a quiet connection to the service is probed, so a service gone is told from one still answering.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// What a call came to short of the route's answer.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ClientFailure {
  /// Nothing answered at the service's address.
  #[error("the job service could not be reached: {0}")]
  Unreachable(String),
  /// The service answered with a status other than the route's, and the error body where it sent one.
  #[error("the job service answered {status}: {}", answer.as_ref().map_or(body.as_str(), |answer| answer.error.as_str()))]
  Answered {
    status: u16,
    answer: Option<ErrorBody>,
    body: String,
  },
  /// The answer was not of the route's shape.
  #[error("the job service's answer could not be read: {0}")]
  Unreadable(String),
}

impl ClientFailure {
  /// The code the service refused with, where it answered one.
  pub fn code(&self) -> Option<&str> {
    match self {
      Self::Answered {
        answer: Some(answer),
        ..
      } => Some(&answer.code),
      _ => None,
    }
  }

  /// The status the service answered with, where it answered.
  pub fn status(&self) -> Option<u16> {
    match self {
      Self::Answered { status, .. } => Some(*status),
      _ => None,
    }
  }
}

/// A client of one job service, at its address and with its token.
#[derive(Debug, Clone)]
pub struct JobClient {
  base: Url,
  token: String,
  http: reqwest::Client,
}

impl JobClient {
  /// A client of the service whose routes stand at `base`, sending `token` as its bearer token.
  pub fn new(base: &str, token: impl Into<String>) -> Result<Self, ClientFailure> {
    let mut base =
      Url::parse(base).map_err(|error| ClientFailure::Unreachable(format!("{base}: {error}")))?;
    if base.cannot_be_a_base() {
      return Err(ClientFailure::Unreachable(format!("{base} cannot carry paths")));
    }
    if !base.path().ends_with('/') {
      let path = format!("{}/", base.path());
      base.set_path(&path);
    }
    let http = reqwest::Client::builder()
      .tcp_keepalive(KEEP_ALIVE)
      .build()
      .map_err(|error| ClientFailure::Unreachable(error.to_string()))?;
    Ok(Self {
      base,
      token: token.into(),
      http,
    })
  }

  /// The address of the route whose path is `segments`, each written as one path segment.
  fn url(&self, segments: &[&str]) -> Url {
    let mut url = self.base.clone();
    {
      let mut path = url.path_segments_mut().expect("the base carries paths");
      path.pop_if_empty();
      path.extend(segments);
    }
    url
  }

  fn request(&self, method: reqwest::Method, segments: &[&str]) -> RequestBuilder {
    self
      .http
      .request(method, self.url(segments))
      .header(AUTHORIZATION, format!("Bearer {}", self.token))
  }

  fn get(&self, segments: &[&str]) -> RequestBuilder {
    self.request(reqwest::Method::GET, segments)
  }

  fn post(&self, segments: &[&str]) -> RequestBuilder {
    self.request(reqwest::Method::POST, segments)
  }

  fn patch(&self, segments: &[&str]) -> RequestBuilder {
    self.request(reqwest::Method::PATCH, segments)
  }

  fn delete(&self, segments: &[&str]) -> RequestBuilder {
    self.request(reqwest::Method::DELETE, segments)
  }

  /// Readiness and version.
  pub async fn health(&self) -> Result<Health, ClientFailure> {
    answer(self.get(&["health"])).await
  }

  /// A page of the event stream.
  pub async fn events_page(&self, query: &EventQuery) -> Result<Page<Event>, ClientFailure> {
    answer(self.get(&["events"]).query(query).header(ACCEPT, "application/json")).await
  }

  /// The event stream, read as it comes, from past `last_event_id` where one is given. It ends when the
  /// service closes it.
  pub async fn event_stream(
    &self,
    query: &EventQuery,
    last_event_id: Option<i64>,
  ) -> Result<impl Stream<Item = Result<Event, ClientFailure>> + Send + 'static, ClientFailure> {
    let mut request = self
      .http
      .get(self.url(&["events"]))
      .header(AUTHORIZATION, format!("Bearer {}", self.token))
      .header(ACCEPT, "text/event-stream")
      .query(query);
    if let Some(id) = last_event_id {
      request = request.header("Last-Event-ID", id.to_string());
    }
    let response = sent(request).await?;
    let response = expected(response, &[StatusCode::OK]).await?;
    let events = SseStream::from_bytes_stream(response.bytes_stream()).filter_map(|sse| async move {
      match sse {
        Err(error) => Some(Err(ClientFailure::Unreadable(error.to_string()))),
        // A comment line keeps the stream open and carries no event.
        Ok(sse) => sse.data.map(|data| {
          serde_json::from_str::<Event>(&data).map_err(|error| ClientFailure::Unreadable(error.to_string()))
        }),
      }
    });
    Ok(events)
  }

  /// Publishes an event of a process of the service's own.
  pub async fn publish_event(&self, event: &NewEvent) -> Result<Event, ClientFailure> {
    answer(self.post(&["events"]).json(event)).await
  }

  /// Items in queue order.
  pub async fn items(&self, query: &ItemQuery) -> Result<Page<Item>, ClientFailure> {
    answer(self.get(&["items"]).query(query)).await
  }

  /// Raises an item.
  pub async fn create_item(&self, item: &NewItem) -> Result<Item, ClientFailure> {
    answer(self.post(&["items"]).json(item)).await
  }

  /// One item.
  pub async fn item(&self, item_id: i64) -> Result<Item, ClientFailure> {
    answer(self.get(&["items", &item_id.to_string()])).await
  }

  /// Changes an item.
  pub async fn change_item(&self, item_id: i64, change: &ItemChange) -> Result<Item, ClientFailure> {
    answer(self.patch(&["items", &item_id.to_string()]).json(change)).await
  }

  /// An item's notes past a cursor, waiting for one where `waitMs` asks.
  pub async fn notes(&self, item_id: i64, query: &WaitQuery) -> Result<Page<Note>, ClientFailure> {
    let segments = ["items", &item_id.to_string(), "notes"];
    answer(self.request(reqwest::Method::GET, &segments).query(query)).await
  }

  /// Appends a note to an item.
  pub async fn add_note(&self, item_id: i64, note: &NewNote) -> Result<Note, ClientFailure> {
    answer(self.post(&["items", &item_id.to_string(), "notes"]).json(note)).await
  }

  /// Takes a note back.
  pub async fn delete_note(&self, item_id: i64, note_id: i64) -> Result<(), ClientFailure> {
    nothing(self.delete(&["items", &item_id.to_string(), "notes", &note_id.to_string()])).await
  }

  /// An item's reports, oldest first.
  pub async fn reports(&self, item_id: i64, query: &PageQuery) -> Result<Page<Report>, ClientFailure> {
    answer(self.get(&["items", &item_id.to_string(), "reports"]).query(query)).await
  }

  /// Appends a report to an item.
  pub async fn add_report(&self, item_id: i64, report: &NewReport) -> Result<Report, ClientFailure> {
    answer(self.post(&["items", &item_id.to_string(), "reports"]).json(report)).await
  }

  /// An item's work order.
  pub async fn work_order(&self, item_id: i64) -> Result<WorkOrder, ClientFailure> {
    answer(self.get(&["items", &item_id.to_string(), "work-order"])).await
  }

  /// Saves an item's work order whole.
  pub async fn put_work_order(
    &self,
    item_id: i64,
    body: &WorkOrderBody,
  ) -> Result<WorkOrder, ClientFailure> {
    let segments = ["items", &item_id.to_string(), "work-order"];
    answer(self.request(reqwest::Method::PUT, &segments).json(body)).await
  }

  /// Changes an item's work order.
  pub async fn change_work_order(
    &self,
    item_id: i64,
    change: &WorkOrderChange,
  ) -> Result<WorkOrder, ClientFailure> {
    answer(self.patch(&["items", &item_id.to_string(), "work-order"]).json(change)).await
  }

  /// A work order's tasks.
  pub async fn tasks(&self, item_id: i64, query: &TaskQuery) -> Result<Page<Task>, ClientFailure> {
    answer(self.get(&["items", &item_id.to_string(), "work-order", "tasks"]).query(query)).await
  }

  /// One task.
  pub async fn task(&self, item_id: i64, task_id: i64) -> Result<Task, ClientFailure> {
    answer(self.get(&["items", &item_id.to_string(), "work-order", "tasks", &task_id.to_string()])).await
  }

  /// Moves a task.
  pub async fn change_task(
    &self,
    item_id: i64,
    task_id: i64,
    change: &TaskChange,
  ) -> Result<Task, ClientFailure> {
    let segments = ["items", &item_id.to_string(), "work-order", "tasks", &task_id.to_string()];
    answer(self.patch(&segments).json(change)).await
  }

  /// Every queue.
  pub async fn queues(&self, query: &QueueQuery) -> Result<Page<Queue>, ClientFailure> {
    answer(self.get(&["queues"]).query(query)).await
  }

  /// One queue.
  pub async fn queue(&self, queue_name: &str) -> Result<Queue, ClientFailure> {
    answer(self.get(&["queues", queue_name])).await
  }

  /// Pauses or opens a queue, or replaces its holds.
  pub async fn change_queue(&self, queue_name: &str, change: &QueueChange) -> Result<Queue, ClientFailure> {
    answer(self.patch(&["queues", queue_name]).json(change)).await
  }

  /// Takes a lease: the item, or `None` where nothing waits once the wait has run out.
  pub async fn take_lease(&self, ask: &LeaseAsk, wait: &LeaseWait) -> Result<Option<Lease>, ClientFailure> {
    let request = self.request(reqwest::Method::POST, &["leases"]).query(wait).json(ask);
    let response = sent(request).await?;
    if response.status() == StatusCode::NO_CONTENT {
      return Ok(None);
    }
    read(expected(response, &[StatusCode::CREATED]).await?).await.map(Some)
  }

  /// The leases standing now.
  pub async fn leases(&self, query: &LeaseQuery) -> Result<Page<Lease>, ClientFailure> {
    answer(self.get(&["leases"]).query(query)).await
  }

  /// One lease.
  pub async fn lease(&self, lease_id: i64) -> Result<Lease, ClientFailure> {
    answer(self.get(&["leases", &lease_id.to_string()])).await
  }

  /// Renews a lease.
  pub async fn renew_lease(&self, lease_id: i64, renewal: &LeaseRenewal) -> Result<Lease, ClientFailure> {
    answer(self.patch(&["leases", &lease_id.to_string()]).json(renewal)).await
  }

  /// Gives a lease back.
  pub async fn release_lease(&self, lease_id: i64) -> Result<(), ClientFailure> {
    nothing(self.delete(&["leases", &lease_id.to_string()])).await
  }

  /// Questions.
  pub async fn questions(&self, query: &QuestionQuery) -> Result<Page<Question>, ClientFailure> {
    answer(self.get(&["questions"]).query(query)).await
  }

  /// Asks a question.
  pub async fn ask(&self, question: &NewQuestion) -> Result<Question, ClientFailure> {
    answer(self.post(&["questions"]).json(question)).await
  }

  /// One question.
  pub async fn question(&self, question_id: i64) -> Result<Question, ClientFailure> {
    answer(self.get(&["questions", &question_id.to_string()])).await
  }

  /// Pauses a question, opens it again or withdraws it.
  pub async fn change_question(
    &self,
    question_id: i64,
    change: &QuestionChange,
  ) -> Result<Question, ClientFailure> {
    answer(self.patch(&["questions", &question_id.to_string()]).json(change)).await
  }

  /// A question's replies past a cursor, waiting for one where `waitMs` asks.
  pub async fn replies(&self, question_id: i64, query: &WaitQuery) -> Result<Page<Reply>, ClientFailure> {
    let segments = ["questions", &question_id.to_string(), "replies"];
    answer(self.request(reqwest::Method::GET, &segments).query(query)).await
  }

  /// Replies to a question.
  pub async fn reply(&self, question_id: i64, reply: &NewReply) -> Result<Reply, ClientFailure> {
    answer(self.post(&["questions", &question_id.to_string(), "replies"]).json(reply)).await
  }

  /// Subscriptions.
  pub async fn subscriptions(
    &self,
    query: &SubscriptionQuery,
  ) -> Result<Page<Subscription>, ClientFailure> {
    answer(self.get(&["subscriptions"]).query(query)).await
  }

  /// Subscribes.
  pub async fn subscribe(&self, subscription: &NewSubscription) -> Result<Subscription, ClientFailure> {
    answer(self.post(&["subscriptions"]).json(subscription)).await
  }

  /// One subscription.
  pub async fn subscription(&self, subscription_id: i64) -> Result<Subscription, ClientFailure> {
    answer(self.get(&["subscriptions", &subscription_id.to_string()])).await
  }

  /// Renews or changes a subscription.
  pub async fn change_subscription(
    &self,
    subscription_id: i64,
    change: &SubscriptionChange,
  ) -> Result<Subscription, ClientFailure> {
    answer(self.patch(&["subscriptions", &subscription_id.to_string()]).json(change)).await
  }

  /// Ends a subscription.
  pub async fn unsubscribe(&self, subscription_id: i64) -> Result<(), ClientFailure> {
    nothing(self.delete(&["subscriptions", &subscription_id.to_string()])).await
  }

  /// What the service holds under a scope.
  pub async fn scope(&self, scope: &str) -> Result<ScopeHolding, ClientFailure> {
    answer(self.get(&["scopes", scope])).await
  }

  /// Lets go of everything held under a scope, or previews it.
  pub async fn delete_scope(
    &self,
    scope: &str,
    deletion: &ScopeDeletion,
  ) -> Result<ScopeHolding, ClientFailure> {
    answer(self.delete(&["scopes", scope]).query(deletion)).await
  }
}

async fn sent(request: RequestBuilder) -> Result<Response, ClientFailure> {
  request.send().await.map_err(|error| ClientFailure::Unreachable(error.to_string()))
}

/// The response where its status is one of `statuses`, the failure it answered otherwise.
async fn expected(response: Response, statuses: &[StatusCode]) -> Result<Response, ClientFailure> {
  if statuses.contains(&response.status()) {
    return Ok(response);
  }
  let status = response.status().as_u16();
  let body = response.text().await.unwrap_or_default();
  Err(ClientFailure::Answered {
    status,
    answer: serde_json::from_str(&body).ok(),
    body,
  })
}

async fn read<T: DeserializeOwned>(response: Response) -> Result<T, ClientFailure> {
  let bytes = response.bytes().await.map_err(|error| ClientFailure::Unreadable(error.to_string()))?;
  serde_json::from_slice(&bytes).map_err(|error| ClientFailure::Unreadable(error.to_string()))
}

/// The answer of a route that answers a member or a page, at 200 or 201.
async fn answer<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, ClientFailure> {
  let response = sent(request).await?;
  read(expected(response, &[StatusCode::OK, StatusCode::CREATED]).await?).await
}

/// The answer of a route that answers 204.
async fn nothing(request: RequestBuilder) -> Result<(), ClientFailure> {
  let response = sent(request).await?;
  expected(response, &[StatusCode::NO_CONTENT]).await.map(|_| ())
}
