//! `GET /events`: the stream, or a page of it, narrowed by kinds and scope, or by what a subscription
//! matches, in which case each event handed over is recorded as delivered (docs/api.md, "Events").

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header::ACCEPT;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bmo_job_client::{Event, EventQuery, Page, SubscriptionState};
use futures_util::stream;
use tokio::sync::watch;

use super::App;
use super::extract::InQuery;
use crate::refusal::Refusal;
use crate::store::{EventFilter, Store, page_limit};

/// How many events one reading of the stream takes at most.
const STREAM_BATCH: i64 = 100;

/// How often a comment line keeps a quiet stream open.
const KEEP_OPEN: Duration = Duration::from_secs(15);

/// Where a stream stands: what it reads, past which event, and what it has read and not yet sent.
struct Reading {
  store: Store,
  narrower: EventFilter,
  subscription_id: Option<i64>,
  cursor: i64,
  waiting: VecDeque<Event>,
  changes: watch::Receiver<u64>,
  stopping: watch::Receiver<bool>,
}

impl Reading {
  /// The next events past the cursor, or none where the subscription is paused; `Err` where the stream ends.
  async fn read(&mut self, limit: i64) -> Result<Vec<Event>, Refusal> {
    match self.subscription_id {
      Some(id) => {
        let kept = self.store.kept_subscription(id).await?;
        if kept.subscription.state != SubscriptionState::Active {
          return Ok(Vec::new());
        }
        let mut events = self.store.events_for(&kept, &self.narrower, Some(self.cursor), limit).await?;
        events.truncate(limit as usize);
        Ok(events)
      }
      None => Ok(self.store.events(&self.narrower, Some(self.cursor), Some(limit)).await?.items),
    }
  }

  /// The next event to send, waiting for one; none once the stream ends.
  async fn next(&mut self) -> Option<Event> {
    loop {
      if let Some(event) = self.waiting.pop_front() {
        if let Some(id) = self.subscription_id
          && self.store.delivered(id, event.id).await.is_err()
        {
          return None;
        }
        return Some(event);
      }
      self.changes.borrow_and_update();
      let events = self.read(STREAM_BATCH).await.ok()?;
      if let Some(last) = events.last() {
        self.cursor = last.id;
        self.waiting.extend(events);
        continue;
      }
      if *self.stopping.borrow() {
        return None;
      }
      tokio::select! {
        changed = self.changes.changed() => changed.ok()?,
        _ = self.stopping.changed() => return None,
      }
    }
  }
}

fn split(text: Option<&str>) -> Vec<String> {
  text
    .into_iter()
    .flat_map(|text| text.split(','))
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(str::to_owned)
    .collect()
}

fn as_sse(event: &Event) -> SseEvent {
  SseEvent::default()
    .event(event.kind.clone())
    .id(event.id.to_string())
    .data(serde_json::to_string(event).expect("an event renders as JSON"))
}

pub(super) async fn events(
  State(app): State<App>,
  headers: HeaderMap,
  InQuery(query): InQuery<EventQuery>,
) -> Result<Response, Refusal> {
  let store = app.store.clone();
  let narrower = EventFilter {
    kinds: split(query.kinds.as_deref()),
    scope: query.scope.clone(),
    ..EventFilter::default()
  };
  let last_event_id = match headers.get("Last-Event-ID").map(|value| value.to_str().map(str::parse::<i64>))
  {
    None => None,
    Some(Ok(Ok(id))) => Some(id),
    Some(_) => return Err(Refusal::bad_request("The Last-Event-ID is not an event number.")),
  };
  let after = query.after.or(last_event_id);

  let subscription = match query.subscription_id {
    Some(id) => {
      let kept = store.kept_subscription(id).await?;
      if kept.subscription.url.is_some() {
        return Err(Refusal::conflict(format!(
          "The subscription {id} is delivered to its url, not read as a stream."
        )));
      }
      Some(kept)
    }
    None => None,
  };

  let accept = headers.get(ACCEPT).and_then(|value| value.to_str().ok()).unwrap_or_default();
  let page_asked = accept.contains("application/json") && !accept.contains("text/event-stream");
  let limit = page_limit(query.limit)?;

  let cursor = match (after, &subscription) {
    (Some(after), _) => after,
    (None, Some(kept)) => kept.cursor(),
    (None, None) if page_asked => store.window_start().await?,
    (None, None) => store.last_event().await?,
  };
  let mut reading = Reading {
    changes: store.changes(),
    stopping: app.stopping(),
    store,
    narrower,
    subscription_id: subscription.map(|kept| kept.subscription.id),
    cursor,
    waiting: VecDeque::new(),
  };

  if page_asked {
    let events = match reading.subscription_id {
      Some(_) => reading.read(limit + 1).await?,
      None => {
        let page = reading.store.events(&reading.narrower, Some(cursor), Some(limit)).await?;
        return Ok(axum::Json(page).into_response());
      }
    };
    let more = events.len() as i64 > limit;
    let mut events = events;
    events.truncate(limit as usize);
    if let (Some(id), Some(last)) = (reading.subscription_id, events.last()) {
      reading.store.delivered(id, last.id).await?;
    }
    return Ok(
      axum::Json(Page {
        items: events,
        more,
      })
      .into_response(),
    );
  }

  // The first reading answers here, so a cursor before the window is refused before the stream opens.
  let first = reading.read(STREAM_BATCH).await?;
  if let Some(last) = first.last() {
    reading.cursor = last.id;
  }
  reading.waiting.extend(first);

  let events = stream::unfold(reading, |mut reading| async move {
    let event = reading.next().await?;
    Some((Ok::<_, Infallible>(as_sse(&event)), reading))
  });
  Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_OPEN)).into_response())
}
