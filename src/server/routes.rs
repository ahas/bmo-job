//! The routes: each reads its request into the contract's types and answers what the store answers.

use std::future::Future;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use bmo_job_client::{
  Health, ItemChange, LeaseAsk, LeaseQuery, LeaseRenewal, LeaseWait, NewEvent, NewItem, NewNote,
  NewQuestion, NewReply, NewReport, NewReview, NewSubscription, Page, PageQuery, QuestionChange,
  QuestionQuery, QueueChange, QueueQuery, ReviewQuery, ScopeDeletion, SubscriptionChange,
  SubscriptionQuery, TaskChange, TaskQuery, WaitQuery, WorkOrderBody, WorkOrderChange,
};
use serde::Serialize;
use tokio::time::Instant;

use super::App;
use super::extract::{InBody, InItemsQuery, InPath, InQuery, ItemsQuery};
use super::port::{self, PortError};
use super::stream;
use crate::access::Caller;
use crate::refusal::Refusal;
use crate::store::{Store, wait_span};

type Answer = Result<Response, Refusal>;

fn ok<T: Serialize>(value: T) -> Answer {
  Ok(Json(value).into_response())
}

fn created<T: Serialize>(value: T) -> Answer {
  Ok((StatusCode::CREATED, Json(value)).into_response())
}

fn none() -> Answer {
  Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) fn routes() -> Router<App> {
  Router::new()
    .route("/health", get(health))
    .route("/events", get(stream::events).post(publish))
    .route("/items", get(items).post(create_item))
    .route("/items/{itemId}", get(item).patch(change_item))
    .route("/rooms/{chat}/holdings", get(port::holdings))
    .route("/questions/open", get(port::open_questions))
    .route("/question", post(port::settle))
    .route("/question-talk", post(port::talk))
    .route("/queue/moved", post(port::queue_moved))
    .route("/items/{itemId}/notes", get(notes).post(add_note))
    .route("/items/{itemId}/notes/{noteId}", delete(delete_note))
    .route("/items/{itemId}/reports", get(reports).post(add_report))
    .route("/items/{itemId}/reviews", get(reviews).post(add_review))
    .route(
      "/items/{itemId}/work-order",
      get(work_order).put(put_work_order).patch(change_work_order),
    )
    .route("/items/{itemId}/work-order/tasks", get(tasks))
    .route("/items/{itemId}/work-order/tasks/{taskId}", get(task).patch(change_task))
    .route("/queues", get(queues))
    .route("/queues/{queueName}", get(queue).patch(change_queue))
    .route("/leases", get(leases).post(take_lease))
    .route("/leases/{leaseId}", get(lease).patch(renew_lease).delete(release_lease))
    .route("/questions", get(questions).post(ask))
    .route("/questions/{questionId}", get(question).patch(change_question))
    .route("/questions/{questionId}/replies", get(replies).post(reply))
    .route("/subscriptions", get(subscriptions).post(subscribe))
    .route(
      "/subscriptions/{subscriptionId}",
      get(subscription).patch(change_subscription).delete(unsubscribe),
    )
    .route("/scopes/{scope}", get(scope).delete(delete_scope))
}

/// Reads until the page holds something or the wait runs out, woken by each change the store commits.
async fn waited<T, F, Fut>(store: &Store, wait: Duration, read: F) -> Result<Page<T>, Refusal>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Result<Page<T>, Refusal>>,
{
  let deadline = Instant::now() + wait;
  let mut changes = store.changes();
  loop {
    changes.borrow_and_update();
    let page = read().await?;
    if !page.items.is_empty() || Instant::now() >= deadline {
      return Ok(page);
    }
    if tokio::time::timeout_at(deadline, changes.changed()).await.is_err() {
      return read().await;
    }
  }
}

async fn health() -> Answer {
  ok(Health {
    ready: true,
    reason: None,
    version: env!("CARGO_PKG_VERSION").to_owned(),
  })
}

async fn publish(State(app): State<App>, InBody(event): InBody<NewEvent>) -> Answer {
  created(app.store.publish(&event.kind, event.data).await?)
}

async fn items(State(app): State<App>, InItemsQuery(query): InItemsQuery) -> Result<Response, PortError> {
  match query {
    ItemsQuery::List(query) => Ok(Json(app.store.items(&query).await?).into_response()),
    ItemsQuery::Open => port::open_items(State(app)).await,
  }
}

async fn create_item(
  State(app): State<App>,
  Extension(caller): Extension<Caller>,
  InBody(item): InBody<NewItem>,
) -> Answer {
  let unattended = app.unattended.matched(&item.labels).map(str::to_owned);
  created(app.store.create_item(item, unattended.as_deref(), &caller).await?)
}

async fn item(State(app): State<App>, InPath(item_id): InPath<i64>) -> Answer {
  ok(app.store.item(item_id).await?)
}

async fn change_item(
  State(app): State<App>,
  Extension(caller): Extension<Caller>,
  InPath(item_id): InPath<i64>,
  InBody(change): InBody<ItemChange>,
) -> Answer {
  ok(app.store.change_item(item_id, change, &caller).await?)
}

async fn notes(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InQuery(query): InQuery<WaitQuery>,
) -> Answer {
  let wait = wait_span(query.wait_ms)?;
  let store = &app.store;
  ok(waited(store, wait, || store.notes(item_id, query.after, query.limit)).await?)
}

async fn add_note(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InBody(note): InBody<NewNote>,
) -> Answer {
  created(app.store.add_note(item_id, note).await?)
}

async fn delete_note(State(app): State<App>, InPath((item_id, note_id)): InPath<(i64, i64)>) -> Answer {
  app.store.delete_note(item_id, note_id).await?;
  none()
}

async fn reports(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InQuery(query): InQuery<PageQuery>,
) -> Answer {
  ok(app.store.reports(item_id, query.after, query.limit).await?)
}

async fn add_report(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InBody(report): InBody<NewReport>,
) -> Answer {
  created(app.store.add_report(item_id, report).await?)
}

async fn reviews(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InQuery(query): InQuery<ReviewQuery>,
) -> Answer {
  ok(app.store.reviews(item_id, &query).await?)
}

async fn add_review(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InBody(review): InBody<NewReview>,
) -> Answer {
  created(app.store.add_review(item_id, review).await?)
}

async fn work_order(State(app): State<App>, InPath(item_id): InPath<i64>) -> Answer {
  ok(app.store.work_order(item_id).await?)
}

async fn put_work_order(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InBody(body): InBody<WorkOrderBody>,
) -> Answer {
  match app.store.put_work_order(item_id, body).await? {
    (order, true) => created(order),
    (order, false) => ok(order),
  }
}

async fn change_work_order(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InBody(change): InBody<WorkOrderChange>,
) -> Answer {
  ok(app.store.change_work_order(item_id, change).await?)
}

async fn tasks(
  State(app): State<App>,
  InPath(item_id): InPath<i64>,
  InQuery(query): InQuery<TaskQuery>,
) -> Answer {
  ok(app.store.tasks(item_id, &query).await?)
}

async fn task(State(app): State<App>, InPath((item_id, task_id)): InPath<(i64, i64)>) -> Answer {
  ok(app.store.task(item_id, task_id).await?)
}

async fn change_task(
  State(app): State<App>,
  InPath((item_id, task_id)): InPath<(i64, i64)>,
  InBody(change): InBody<TaskChange>,
) -> Answer {
  ok(app.store.change_task(item_id, task_id, change).await?)
}

async fn queues(State(app): State<App>, InQuery(query): InQuery<QueueQuery>) -> Answer {
  ok(app.store.queues(&query).await?)
}

async fn queue(State(app): State<App>, InPath(name): InPath<String>) -> Answer {
  ok(app.store.queue(&name).await?)
}

async fn change_queue(
  State(app): State<App>,
  InPath(name): InPath<String>,
  InBody(change): InBody<QueueChange>,
) -> Answer {
  ok(app.store.change_queue(&name, change).await?)
}

async fn take_lease(
  State(app): State<App>,
  Extension(caller): Extension<Caller>,
  InQuery(wait): InQuery<LeaseWait>,
  InBody(ask): InBody<LeaseAsk>,
) -> Answer {
  let deadline = Instant::now() + wait_span(wait.wait_ms)?;
  let mut changes = app.store.changes();
  loop {
    changes.borrow_and_update();
    if let Some(lease) = app.store.take_lease(&ask, &caller).await? {
      return created(lease);
    }
    if Instant::now() >= deadline || tokio::time::timeout_at(deadline, changes.changed()).await.is_err() {
      return none();
    }
  }
}

async fn leases(State(app): State<App>, InQuery(query): InQuery<LeaseQuery>) -> Answer {
  ok(app.store.leases(&query).await?)
}

async fn lease(State(app): State<App>, InPath(lease_id): InPath<i64>) -> Answer {
  ok(app.store.lease(lease_id).await?)
}

async fn renew_lease(
  State(app): State<App>,
  InPath(lease_id): InPath<i64>,
  InBody(renewal): InBody<LeaseRenewal>,
) -> Answer {
  ok(app.store.renew_lease(lease_id, renewal.ttl_ms).await?)
}

async fn release_lease(
  State(app): State<App>,
  Extension(caller): Extension<Caller>,
  InPath(lease_id): InPath<i64>,
) -> Answer {
  app.store.release_lease(lease_id, &caller).await?;
  none()
}

async fn questions(State(app): State<App>, InQuery(query): InQuery<QuestionQuery>) -> Answer {
  ok(app.store.questions(&query).await?)
}

async fn ask(State(app): State<App>, InBody(question): InBody<NewQuestion>) -> Answer {
  created(app.store.ask(question).await?)
}

async fn question(State(app): State<App>, InPath(question_id): InPath<i64>) -> Answer {
  ok(app.store.question(question_id).await?)
}

async fn change_question(
  State(app): State<App>,
  InPath(question_id): InPath<i64>,
  InBody(change): InBody<QuestionChange>,
) -> Answer {
  ok(app.store.change_question(question_id, change).await?)
}

async fn replies(
  State(app): State<App>,
  InPath(question_id): InPath<i64>,
  InQuery(query): InQuery<WaitQuery>,
) -> Answer {
  let wait = wait_span(query.wait_ms)?;
  let store = &app.store;
  ok(waited(store, wait, || store.replies(question_id, query.after, query.limit)).await?)
}

async fn reply(
  State(app): State<App>,
  InPath(question_id): InPath<i64>,
  InBody(reply): InBody<NewReply>,
) -> Answer {
  created(app.store.reply(question_id, reply).await?)
}

async fn subscriptions(State(app): State<App>, InQuery(query): InQuery<SubscriptionQuery>) -> Answer {
  ok(app.store.subscriptions(&query).await?)
}

async fn subscribe(State(app): State<App>, InBody(subscription): InBody<NewSubscription>) -> Answer {
  created(app.store.subscribe(subscription).await?)
}

async fn subscription(State(app): State<App>, InPath(subscription_id): InPath<i64>) -> Answer {
  ok(app.store.subscription(subscription_id).await?)
}

async fn change_subscription(
  State(app): State<App>,
  InPath(subscription_id): InPath<i64>,
  InBody(change): InBody<SubscriptionChange>,
) -> Answer {
  ok(app.store.change_subscription(subscription_id, change).await?)
}

async fn unsubscribe(State(app): State<App>, InPath(subscription_id): InPath<i64>) -> Answer {
  app.store.unsubscribe(subscription_id).await?;
  none()
}

async fn scope(State(app): State<App>, InPath(scope): InPath<String>) -> Answer {
  ok(app.store.scope(&scope).await?)
}

async fn delete_scope(
  State(app): State<App>,
  InPath(scope): InPath<String>,
  InQuery(deletion): InQuery<ScopeDeletion>,
) -> Answer {
  ok(app.store.delete_scope(&scope, deletion.dry_run.unwrap_or(false)).await?)
}
