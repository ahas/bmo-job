//! The port's HTTP client: each call made on the method and path the API document gives it and its answer
//! read as JSON.

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{Answer, Graph, GraphError, Named};

/// The calls on bmo-graph, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct GraphClient {
  base: Url,
  http: Client,
}

/// A refusal's body, as "A refused request" in the API document gives it.
#[derive(Deserialize)]
struct Said {
  error: String,
  code: String,
}

/// The failure a status that is no success stands for: bmo-graph's refusal where its body reads as one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> GraphError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => GraphError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => GraphError::Malformed {
      call,
      status: status.as_u16(),
      error: error.to_string(),
    },
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> GraphError {
  move |error| GraphError::Unreachable {
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

  serde_json::from_slice(&body).map_err(|error| GraphError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  })
}

impl GraphClient {
  /// A client of bmo-graph at `base`, which must be an `http:` address.
  pub fn new(base: Url) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(GraphError::NotAnAddress {
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

  /// The address of the path made of `segments` under the client's.
  fn url(&self, segments: &[&str]) -> Url {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    url
  }

  /// A call answered with a body.
  async fn answer<T: DeserializeOwned>(&self, call: &'static str, request: RequestBuilder) -> Answer<T> {
    read(call, request.send().await.map_err(unreachable(call))?).await
  }
}

impl Graph for GraphClient {
  async fn named(&self, checkout: &str) -> Answer<Named> {
    let mut url = self.url(&["graph", "named"]);

    url.query_pairs_mut().append_pair("checkout", checkout);

    self.answer("named", self.http.request(Method::GET, url)).await
  }
}
