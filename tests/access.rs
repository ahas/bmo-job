//! Access and the error shape: the token on every route, a route that is not served, and a request not of a
//! route's shape (docs/api.md, "Access").

use crate::support;

use bmo_job_client::{ErrorBody, Health, JobClient};

use support::Service;

async fn refusal(answer: reqwest::Response) -> (u16, String) {
  let status = answer.status().as_u16();
  let body: ErrorBody = answer.json().await.expect("a refusal in the error shape");
  (status, body.code)
}

#[tokio::test]
async fn every_route_takes_the_token() {
  let service = Service::start().await;
  let wrong = JobClient::new(&service.url, "not-the-token").expect("a client");
  let refused = wrong.health().await.expect_err("the wrong token");
  assert_eq!((refused.status(), refused.code()), (Some(401), Some("unauthorized")));
  let bare = reqwest::get(format!("{}/items", service.url)).await.expect("answered");
  assert_eq!(refusal(bare).await, (401, "unauthorized".to_owned()));

  let health: Health = service.client().health().await.expect("ready");
  assert_eq!(
    (health.ready, health.reason, health.version.as_str()),
    (true, None, env!("CARGO_PKG_VERSION"))
  );
}

#[tokio::test]
async fn an_unserved_route_and_a_misshapen_request_are_refused_in_the_error_shape() {
  let service = Service::start().await;
  let nowhere = service.raw(reqwest::Method::GET, "/todos").send().await.expect("answered");
  assert_eq!(refusal(nowhere).await, (404, "no-route".to_owned()));
  let wrong_method = service.raw(reqwest::Method::PUT, "/items").send().await.expect("answered");
  assert_eq!(refusal(wrong_method).await, (404, "no-route".to_owned()));
  let not_a_key = service.raw(reqwest::Method::GET, "/items/seven").send().await.expect("answered");
  assert_eq!(refusal(not_a_key).await, (400, "bad-request".to_owned()));
  let no_body = service.raw(reqwest::Method::POST, "/items").send().await.expect("answered");
  assert_eq!(refusal(no_body).await, (400, "bad-request".to_owned()));
  let unknown_query =
    service.raw(reqwest::Method::GET, "/items?colour=red").send().await.expect("answered");
  assert_eq!(refusal(unknown_query).await, (400, "bad-request".to_owned()));
  let bad_limit = service.raw(reqwest::Method::GET, "/items?limit=0").send().await.expect("answered");
  assert_eq!(refusal(bad_limit).await, (400, "bad-request".to_owned()));
}
