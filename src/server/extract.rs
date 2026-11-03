//! The request's path, query and body, each read into the route's own type, and a request not of that
//! shape answered 400 `bad-request` in the error shape.

use std::collections::HashMap;

use axum::extract::rejection::QueryRejection;
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::http::request::Parts;
use bmo_job_client::ItemQuery;
use serde::de::DeserializeOwned;

use crate::refusal::Refusal;

/// The path's parameters.
pub struct InPath<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for InPath<T> {
  type Rejection = Refusal;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    Path::<T>::from_request_parts(parts, state)
      .await
      .map(|Path(value)| Self(value))
      .map_err(|rejection| {
        Refusal::bad_request(format!("The path does not read: {}.", rejection.body_text()))
      })
  }
}

/// The query.
pub struct InQuery<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for InQuery<T> {
  type Rejection = Refusal;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    Query::<T>::from_request_parts(parts, state)
      .await
      .map(|Query(value)| Self(value))
      .map_err(not_read)
  }
}

/// The query of `GET /items`: the open listing the port asks for when `open` is set, or the list's own
/// filters.
pub enum ItemsQuery {
  List(Box<ItemQuery>),
  Open,
}

pub struct InItemsQuery(pub ItemsQuery);

impl<S: Send + Sync> FromRequestParts<S> for InItemsQuery {
  type Rejection = Refusal;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let Query(values) = Query::<HashMap<String, String>>::from_request_parts(parts, state)
      .await
      .map_err(not_read)?;
    match values.get("open") {
      None => {
        let Query(list) = Query::<ItemQuery>::from_request_parts(parts, state).await.map_err(not_read)?;
        Ok(Self(ItemsQuery::List(Box::new(list))))
      }
      Some(open) if matches!(open.as_str(), "false" | "0") => Err(Refusal::bad_request(
        "The open flag is false or 0; the open listing is the one this flag asks for.",
      )),
      Some(_) => Ok(Self(ItemsQuery::Open)),
    }
  }
}

fn not_read(rejection: QueryRejection) -> Refusal {
  Refusal::bad_request(format!("The query does not read: {}.", rejection.body_text()))
}

/// The JSON body.
pub struct InBody<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequest<S> for InBody<T> {
  type Rejection = Refusal;

  async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
    axum::Json::<T>::from_request(request, state)
      .await
      .map(|axum::Json(value)| Self(value))
      .map_err(|rejection| {
        Refusal::bad_request(format!("The body does not read: {}.", rejection.body_text()))
      })
  }
}
