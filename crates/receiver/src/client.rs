//! The port's HTTP client: each call made on the method and path the API document gives it, its body
//! written and its answer read as JSON.

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{
  Answer, Answerers, Holder, Instruction, InstructionState, Message, Notice, Owner, Receiver,
  ReceiverError, Room, Talk,
};

/// The calls on bmo-receiver, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct ReceiverClient {
  base: Url,
  http: Client,
}

/// A refusal's body, as "A refused request" in the API document gives it.
#[derive(Deserialize)]
struct Said {
  error: String,
  code: String,
}

#[derive(Deserialize)]
struct Queued {
  id: i64,
}

#[derive(Deserialize)]
struct Stands {
  state: InstructionState,
}

#[derive(Deserialize)]
struct Withdrawn {
  withdrawn: bool,
}

#[derive(Deserialize)]
struct Answered {
  answer: Option<String>,
}

/// The failure a status that is no success stands for: bmo-receiver's refusal where its body reads as
/// one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> ReceiverError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => ReceiverError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => ReceiverError::Malformed {
      call,
      status: status.as_u16(),
      error: error.to_string(),
    },
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> ReceiverError {
  move |error| ReceiverError::Unreachable {
    call,
    error: error.to_string(),
  }
}

/// A response's body read as `T`, or the failure its status stands for.
async fn read<T: DeserializeOwned>(call: &'static str, response: Response) -> Answer<T> {
  let status = response.status();
  let body = response.bytes().await.map_err(unreachable(call))?;

  if !status.is_success() {
    return Err(refused(call, status, &body));
  }

  serde_json::from_slice(&body).map_err(|error| ReceiverError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  })
}

impl ReceiverClient {
  /// A client of bmo-receiver at `base`, which must be an `http:` address.
  pub fn new(base: Url) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(ReceiverError::NotAnAddress {
        address: base.to_string(),
      });
    }

    Ok(Self {
      base,
      http: Client::new(),
    })
  }

  /// The address the client calls.
  pub fn address(&self) -> &Url {
    &self.base
  }

  /// A request on the path made of `segments` under the address, each percent-encoded.
  fn request(&self, method: Method, segments: &[&str]) -> RequestBuilder {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    self.http.request(method, url)
  }

  async fn send(&self, call: &'static str, request: RequestBuilder) -> Answer<Response> {
    request.send().await.map_err(unreachable(call))
  }

  /// A call answered with a body.
  async fn answer<T: DeserializeOwned>(&self, call: &'static str, request: RequestBuilder) -> Answer<T> {
    read(call, self.send(call, request).await?).await
  }

  /// A read of one thing by its id, where 404 is nothing found.
  async fn found<T: DeserializeOwned>(
    &self,
    call: &'static str,
    request: RequestBuilder,
  ) -> Answer<Option<T>> {
    let response = self.send(call, request).await?;

    if response.status() == StatusCode::NOT_FOUND {
      return Ok(None);
    }

    read(call, response).await.map(Some)
  }

  /// A call answered with no body.
  async fn done(&self, call: &'static str, request: RequestBuilder) -> Answer<()> {
    let response = self.send(call, request).await?;
    let status = response.status();

    if status.is_success() {
      return Ok(());
    }

    let body = response.bytes().await.map_err(unreachable(call))?;

    Err(refused(call, status, &body))
  }
}

impl Receiver for ReceiverClient {
  async fn queue_instruction(&self, instruction: &Instruction) -> Answer<i64> {
    let request = self.request(Method::POST, &["instructions"]).json(instruction);
    let queued: Queued = self.answer("queue_instruction", request).await?;

    Ok(queued.id)
  }

  async fn instruction_state(&self, id: i64) -> Answer<Option<InstructionState>> {
    let request = self.request(Method::GET, &["instructions", &id.to_string()]);
    let stands: Option<Stands> = self.found("instruction_state", request).await?;

    Ok(stands.map(|stands| stands.state))
  }

  async fn withdraw_instruction(&self, id: i64) -> Answer<bool> {
    let request = self.request(Method::DELETE, &["instructions", &id.to_string()]);
    let withdrawn: Withdrawn = self.answer("withdraw_instruction", request).await?;

    Ok(withdrawn.withdrawn)
  }

  async fn voice_notice(&self, chat: i64, notice: &Notice) -> Answer<i64> {
    let request = self.request(Method::POST, &["rooms", &chat.to_string(), "notices"]).json(notice);
    let queued: Queued = self.answer("voice_notice", request).await?;

    Ok(queued.id)
  }

  async fn owner(&self) -> Answer<Owner> {
    self.answer("owner", self.request(Method::GET, &["owner"])).await
  }

  async fn room(&self, chat: i64) -> Answer<Room> {
    self.answer("room", self.request(Method::GET, &["rooms", &chat.to_string()])).await
  }

  async fn answerers(&self, chat: i64) -> Answer<Answerers> {
    let request = self.request(Method::GET, &["rooms", &chat.to_string(), "answerers"]);

    self.answer("answerers", request).await
  }

  async fn message(&self, chat: i64, message: i64) -> Answer<Option<Message>> {
    let request =
      self.request(Method::GET, &["rooms", &chat.to_string(), "messages", &message.to_string()]);

    self.found("message", request).await
  }

  async fn talk(&self, talk: &Talk) -> Answer<Option<String>> {
    let request = self.request(Method::POST, &["talks"]).json(talk);
    let answered: Answered = self.answer("talk", request).await?;

    Ok(answered.answer)
  }

  async fn declare_holder(&self, unit: &str, holder: &Holder) -> Answer<()> {
    let request = self.request(Method::PUT, &["room-holders", unit]).json(holder);

    self.done("declare_holder", request).await
  }
}
