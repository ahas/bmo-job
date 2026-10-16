//! A request as a route reads it (docs/api.md, "Routes"): its body as JSON, refused `body_not_json` where
//! it is none; its fields, each refused `field_wrong` where it is missing or not of the form it takes; and
//! a form, as the console's box sends one where it carries files.

use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::header;
use job_base::Refusal;
use job_codes::{ControlRefusal, FieldForm, JobError, Refused};
use serde_json::{Map, Value};

use crate::verbs::WordFile;

/// A refusal of any area, as a route answers it.
pub fn refused(refusal: impl Into<JobError>) -> Refusal {
  refusal.into().refusal()
}

/// The refusal of a field that is missing or not of its form.
pub fn wrong(name: &str, takes: FieldForm) -> Refusal {
  refused(ControlRefusal::FieldWrong {
    name: name.to_owned(),
    takes,
  })
}

/// A body read as JSON; an empty body is an empty object.
pub struct Body(pub Value);

impl<S: Send + Sync> FromRequest<S> for Body {
  type Rejection = Refusal;

  async fn from_request(request: Request, state: &S) -> Result<Self, Refusal> {
    let bytes = Bytes::from_request(request, state)
      .await
      .map_err(|_| refused(ControlRefusal::BodyNotJson))?;

    if bytes.is_empty() {
      return Ok(Self(Value::Object(Map::new())));
    }

    serde_json::from_slice(&bytes)
      .map(Self)
      .map_err(|_| refused(ControlRefusal::BodyNotJson))
  }
}

/// A body that comes as JSON, or as a form: its fields as a JSON object of strings, and the files that
/// stand under `file`.
pub struct Sent {
  pub body: Value,
  pub files: Vec<WordFile>,
}

// The field a form carries its files under.
const FILE_FIELD: &str = "file";

impl<S: Send + Sync> FromRequest<S> for Sent {
  type Rejection = Refusal;

  async fn from_request(request: Request, state: &S) -> Result<Self, Refusal> {
    let form = request
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|value| value.to_str().ok())
      .is_some_and(|value| value.starts_with("multipart/form-data"));

    if !form {
      let Body(body) = Body::from_request(request, state).await?;

      return Ok(Self {
        body,
        files: vec![],
      });
    }

    let mut parts = Multipart::from_request(request, state).await.map_err(not_read)?;
    let mut fields = Map::new();
    let mut files = vec![];

    while let Some(part) = parts.next_field().await.map_err(not_read)? {
      let name = part.name().unwrap_or_default().to_owned();

      match part.file_name().map(str::to_owned) {
        Some(file) if name == FILE_FIELD => files.push(WordFile {
          name: file,
          bytes: part.bytes().await.map_err(not_read)?.to_vec(),
        }),
        Some(_) => {}
        None => {
          fields.insert(name, Value::String(part.text().await.map_err(not_read)?));
        }
      }
    }

    Ok(Self {
      body: Value::Object(fields),
      files,
    })
  }
}

// A form that does not read is a body that does not read.
fn not_read<E>(_: E) -> Refusal {
  refused(ControlRefusal::BodyNotJson)
}

/// A whole number as JSON carries one: an integer, or a number with no fraction.
fn whole(value: &Value) -> Option<i64> {
  value.as_i64().or_else(|| {
    value
      .as_f64()
      .filter(|number| number.fract() == 0.0 && number.abs() < 9.2e18)
      .map(|number| number as i64)
  })
}

/// The fields of a body.
pub struct Fields<'a>(pub &'a Value);

impl Fields<'_> {
  // A field that stands and is not null.
  fn given(&self, name: &str) -> Option<&Value> {
    self.0.get(name).filter(|value| !value.is_null())
  }

  /// A whole number the field must carry.
  pub fn whole(&self, name: &str) -> Result<i64, Refusal> {
    self.whole_or_none(name)?.ok_or_else(|| wrong(name, FieldForm::WholeNumber))
  }

  /// A whole number the field may carry.
  pub fn whole_or_none(&self, name: &str) -> Result<Option<i64>, Refusal> {
    self
      .given(name)
      .map(|value| whole(value).ok_or_else(|| wrong(name, FieldForm::WholeNumber)))
      .transpose()
  }

  /// True or false, which the field must carry.
  pub fn boolean(&self, name: &str) -> Result<bool, Refusal> {
    self.boolean_or_none(name)?.ok_or_else(|| wrong(name, FieldForm::Boolean))
  }

  /// True or false, which the field may carry.
  pub fn boolean_or_none(&self, name: &str) -> Result<Option<bool>, Refusal> {
    self
      .given(name)
      .map(|value| value.as_bool().ok_or_else(|| wrong(name, FieldForm::Boolean)))
      .transpose()
  }

  /// Whether the field is true, and nothing else: a switch left out is off.
  pub fn on(&self, name: &str) -> bool {
    self.0.get(name) == Some(&Value::Bool(true))
  }

  /// A string the field may carry, as it stands.
  pub fn text_or_none(&self, name: &str) -> Result<Option<String>, Refusal> {
    self
      .given(name)
      .map(|value| value.as_str().map(str::to_owned).ok_or_else(|| wrong(name, FieldForm::Text)))
      .transpose()
  }

  /// A string with something in it the field must carry, trimmed.
  pub fn words(&self, name: &str) -> Result<String, Refusal> {
    self.words_or_none(name)?.ok_or_else(|| wrong(name, FieldForm::Words))
  }

  /// A string the field may carry, trimmed; one with nothing in it is none.
  pub fn words_or_none(&self, name: &str) -> Result<Option<String>, Refusal> {
    Ok(
      self
        .text_or_none(name)
        .map_err(|_| wrong(name, FieldForm::Words))?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty()),
    )
  }

  /// A list of whole numbers the field must carry.
  pub fn numbers(&self, name: &str) -> Result<Vec<i64>, Refusal> {
    self.numbers_or_none(name)?.ok_or_else(|| wrong(name, FieldForm::Numbers))
  }

  /// A list of whole numbers the field may carry.
  pub fn numbers_or_none(&self, name: &str) -> Result<Option<Vec<i64>>, Refusal> {
    self
      .given(name)
      .map(|value| {
        value
          .as_array()
          .and_then(|members| members.iter().map(whole).collect::<Option<Vec<i64>>>())
          .ok_or_else(|| wrong(name, FieldForm::Numbers))
      })
      .transpose()
  }

  /// An object the field must carry.
  pub fn object(&self, name: &str) -> Result<Map<String, Value>, Refusal> {
    self
      .given(name)
      .and_then(Value::as_object)
      .cloned()
      .ok_or_else(|| wrong(name, FieldForm::Object))
  }
}
