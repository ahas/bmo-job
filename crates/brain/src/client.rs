//! The port's HTTP client: each call made on the method and path the API document gives it, with the
//! brain's token, its body written and its answer read as JSON.

use std::fmt;

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Answer, Brain, BrainError, Hold, Holds, Memory, MemoryAdd, Turn, Waited};

/// The calls on bmo-brain, made over HTTP to the address given with the token given.
#[derive(Clone)]
pub struct BrainClient {
  base: Url,
  token: String,
  http: Client,
}

/// The client as a log may show it: its address, never its token.
impl fmt::Debug for BrainClient {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("BrainClient").field("base", &self.base).finish_non_exhaustive()
  }
}

/// The body of a status that is no success, as bmo-brain's error shape gives it.
#[derive(Deserialize)]
struct Said {
  error: String,
  code: String,
}

/// Whether an answer at 200 was carried out, and the refusal where it was not.
#[derive(Deserialize)]
struct Verdict {
  answered: bool,
  refusal: Option<Refusal>,
}

#[derive(Deserialize)]
struct Refusal {
  code: String,
  message: String,
}

#[derive(Serialize)]
struct Run<'a> {
  run: &'a str,
}

#[derive(Serialize)]
struct Filing<'a> {
  turn: &'a Turn,
  #[serde(flatten)]
  asked: &'a MemoryAdd,
}

#[derive(Deserialize)]
struct Memories {
  memories: Vec<Memory>,
}

#[derive(Deserialize)]
struct Reading {
  lines: Vec<String>,
}

fn malformed(call: &'static str, status: StatusCode, error: impl ToString) -> BrainError {
  BrainError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  }
}

/// The failure a status that is no success stands for: bmo-brain's refusal where its body reads as one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> BrainError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => BrainError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => malformed(call, status, error),
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> BrainError {
  move |error| BrainError::Unreachable {
    call,
    error: error.to_string(),
  }
}

/// A response's body read as `T` where it was carried out, or the refusal or failure it stands for.
async fn read<T: DeserializeOwned>(call: &'static str, response: Response) -> Answer<T> {
  let status = response.status();
  let body = response.bytes().await.map_err(unreachable(call))?;

  if !status.is_success() {
    return Err(refused(call, status, &body));
  }

  let verdict: Verdict = serde_json::from_slice(&body).map_err(|error| malformed(call, status, error))?;

  if !verdict.answered {
    let Some(refusal) = verdict.refusal else {
      return Err(malformed(call, status, "an answer not carried out carries no refusal"));
    };

    return Err(BrainError::Refused {
      call,
      status: status.as_u16(),
      code: refusal.code,
      error: refusal.message,
    });
  }

  serde_json::from_slice(&body).map_err(|error| malformed(call, status, error))
}

impl BrainClient {
  /// A client of bmo-brain at `base`, which must be an `http:` address, calling with `token`.
  pub fn new(base: Url, token: impl Into<String>) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(BrainError::NotAnAddress {
        address: base.to_string(),
      });
    }

    Ok(Self {
      base,
      token: token.into(),
      http: Client::new(),
    })
  }

  /// The address the client calls.
  pub fn address(&self) -> &Url {
    &self.base
  }

  /// The address of the path made of `segments` under the client's, each percent-encoded.
  fn url(&self, segments: &[&str]) -> Url {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    url
  }

  /// A request at `url`, carrying the brain's token.
  fn request(&self, method: Method, url: Url) -> RequestBuilder {
    self.http.request(method, url).bearer_auth(&self.token)
  }

  /// A call answered with a body.
  async fn answer<T: DeserializeOwned>(&self, call: &'static str, request: RequestBuilder) -> Answer<T> {
    read(call, request.send().await.map_err(unreachable(call))?).await
  }
}

impl Brain for BrainClient {
  async fn hold(&self, run: &str) -> Answer<Hold> {
    let request = self.request(Method::POST, self.url(&["brain", "hold"])).json(&Run { run });

    self.answer("hold", request).await
  }

  async fn holds(&self) -> Answer<Holds> {
    let request = self.request(Method::GET, self.url(&["brain", "hold"]));

    self.answer("holds", request).await
  }

  async fn wait(&self) -> Answer<Waited> {
    let request = self.request(Method::POST, self.url(&["brain", "wait"])).json(&serde_json::json!({}));

    self.answer("wait", request).await
  }

  async fn memories(&self, agent: &str, chat: i64) -> Answer<Vec<Memory>> {
    let mut url = self.url(&["memories"]);

    url.query_pairs_mut().append_pair("agent", agent).append_pair("chat", &chat.to_string());

    let read: Memories = self.answer("memories", self.request(Method::GET, url)).await?;

    Ok(read.memories)
  }

  async fn add_memory(&self, turn: &Turn, asked: &MemoryAdd) -> Answer<Vec<String>> {
    let request = self.request(Method::POST, self.url(&["memory", "add"])).json(&Filing { turn, asked });
    let reading: Reading = self.answer("add_memory", request).await?;

    Ok(reading.lines)
  }
}
