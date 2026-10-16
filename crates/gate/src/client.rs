//! The port's HTTP client: each call made on the method and path the API document gives it, its body
//! written and its answer read as JSON.

use std::collections::HashMap;

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Answer, Gate, GateError, KnowledgeAsked, Piece, Reading, Sanitise, Sanitised};

/// The calls on bmo-gate, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct GateClient {
  base: Url,
  http: Client,
}

/// A refusal's body, as "A refused request" in the API document gives it.
#[derive(Deserialize)]
struct Said {
  error: String,
  code: String,
}

#[derive(Serialize)]
struct Originals<'a> {
  language: &'a str,
  originals: &'a [String],
}

#[derive(Serialize)]
struct Keep<'a> {
  language: &'a str,
  original: &'a str,
  text: &'a str,
}

#[derive(Deserialize)]
struct Translation {
  original: String,
  text: String,
}

#[derive(Deserialize)]
struct Kept {
  kept: Vec<Translation>,
}

#[derive(Deserialize)]
struct Readings {
  readings: Vec<Reading>,
}

#[derive(Deserialize)]
struct Pieces {
  pieces: Vec<Piece>,
}

/// The failure a status that is no success stands for: bmo-gate's refusal where its body reads as one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> GateError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => GateError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => GateError::Malformed {
      call,
      status: status.as_u16(),
      error: error.to_string(),
    },
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> GateError {
  move |error| GateError::Unreachable {
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

  serde_json::from_slice(&body).map_err(|error| GateError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  })
}

impl GateClient {
  /// A client of bmo-gate at `base`, which must be an `http:` address.
  pub fn new(base: Url) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(GateError::NotAnAddress {
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

impl Gate for GateClient {
  async fn kept(&self, language: &str, originals: &[String]) -> Answer<HashMap<String, String>> {
    let request = self.request(Method::POST, &["translations", "kept", "read"]).json(&Originals {
      language,
      originals,
    });
    let kept: Kept = self.answer("kept", request).await?;

    Ok(kept.kept.into_iter().map(|kept| (kept.original, kept.text)).collect())
  }

  async fn translate(&self, language: &str, originals: &[String]) -> Answer<Vec<Reading>> {
    const CALL: &str = "translate";

    let request = self.request(Method::POST, &["translations"]).json(&Originals {
      language,
      originals,
    });
    let response = self.send(CALL, request).await?;
    let status = response.status().as_u16();
    let read: Readings = read(CALL, response).await?;

    // A reading stands for the original at its place, so one missing or one too many reads as none.
    if read.readings.len() != originals.len() {
      return Err(GateError::Malformed {
        call: CALL,
        status,
        error: format!("{} readings for {} originals", read.readings.len(), originals.len()),
      });
    }

    Ok(read.readings)
  }

  async fn keep(&self, language: &str, original: &str, text: &str) -> Answer<()> {
    let request = self.request(Method::PUT, &["translations", "kept"]).json(&Keep {
      language,
      original,
      text,
    });

    self.done("keep", request).await
  }

  async fn sanitise(&self, asked: &Sanitise) -> Answer<Sanitised> {
    let request = self.request(Method::POST, &["sanitised"]).json(asked);

    self.answer("sanitise", request).await
  }

  async fn knowledge(&self, asked: &KnowledgeAsked) -> Answer<Vec<Piece>> {
    let request = self.request(Method::POST, &["knowledge", "selected"]).json(asked);
    let selected: Pieces = self.answer("knowledge", request).await?;

    Ok(selected.pieces)
  }
}
