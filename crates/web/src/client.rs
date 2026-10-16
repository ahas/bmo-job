//! The port's HTTP client: each call made on the method and path the API document gives it, its body
//! written and its answer read as JSON.

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{Answer, Checked, ObjectCheck, Web, WebError};

/// The calls on bmo-web, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct WebClient {
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
struct Model {
  in_force: bool,
}

#[derive(Deserialize)]
struct Linked {
  user: String,
}

/// The failure a status that is no success stands for: bmo-web's refusal where its body reads as one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> WebError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => WebError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => WebError::Malformed {
      call,
      status: status.as_u16(),
      error: error.to_string(),
    },
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> WebError {
  move |error| WebError::Unreachable {
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

  serde_json::from_slice(&body).map_err(|error| WebError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  })
}

impl WebClient {
  /// A client of bmo-web at `base`, which must be an `http:` address.
  pub fn new(base: Url) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(WebError::NotAnAddress {
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
}

impl Web for WebClient {
  async fn model_in_force(&self) -> Answer<bool> {
    let request = self.request(Method::GET, &["accounts", "model"]);
    let model: Model = self.answer("model_in_force", request).await?;

    Ok(model.in_force)
  }

  async fn user_of_sender(&self, sender: i64) -> Answer<Option<String>> {
    let request = self.request(Method::GET, &["senders", &sender.to_string(), "user"]);
    let linked: Option<Linked> = self.found("user_of_sender", request).await?;

    Ok(linked.map(|linked| linked.user))
  }

  async fn check_object(&self, check: &ObjectCheck) -> Answer<Checked> {
    let request = self.request(Method::POST, &["checks", "object"]).json(check);

    self.answer("check_object", request).await
  }
}
