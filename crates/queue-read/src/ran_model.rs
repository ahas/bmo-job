//! The model an item ran on (docs/reading.md, "The model an item ran on"): the road its last run went out
//! on, whether the provider order fell to it, whether its runs went out on more than one road, and the
//! request where the two differ. Each reading puts its own words around these.

use job_agency::Provider;
use job_codes::todo_reading::ModelMark;

use crate::queue::RanOn;

/// One reading of a run's model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RanModel {
  /// `provider/model` of the road taken, or the requested model where nothing has gone out yet.
  pub shown: String,
  /// The requested model, only where it is not the road taken.
  pub requested: Option<String>,
  pub fallback: bool,
  pub changed: bool,
}

impl RanModel {
  /// The marks it carries, the request only where an opened item asks for it.
  pub fn marks(&self, with_request: bool) -> Vec<ModelMark> {
    let requested = self.requested.as_ref().filter(|_| with_request);

    [
      self.fallback.then_some(ModelMark::Fallback),
      self.changed.then_some(ModelMark::Changed),
      requested.map(|model| ModelMark::Requested {
        model: model.clone(),
      }),
    ]
    .into_iter()
    .flatten()
    .collect()
  }
}

/// The provider a request naming none walks to first: the first that is on, in the provider order.
pub fn head_provider(providers: &[Provider]) -> Option<&str> {
  providers
    .iter()
    .find(|provider| provider.enabled)
    .map(|provider| provider.name.as_str())
}

/// A requested model is `provider::model`, or a bare model where no provider was named; a bare one is the
/// road taken whenever the model matches.
fn split(requested: &str) -> (Option<&str>, &str) {
  match requested.split_once("::") {
    Some((provider, model)) => (Some(provider), model),
    None => (None, requested),
  }
}

fn asked(requested: &str, ran: &RanOn) -> bool {
  let (provider, model) = split(requested);

  provider.is_none_or(|provider| provider == ran.provider) && model == ran.model
}

/// A road is a fallback when it is on another provider than the one asked for: the head of the order
/// where the request named none. Another model on the provider asked for is the provider's own choice,
/// not the order's.
fn fell_back(requested: &str, ran: &RanOn, head: Option<&str>) -> bool {
  !asked(requested, ran) && split(requested).0.or(head).is_some_and(|provider| provider != ran.provider)
}

pub fn ran_model(requested: &str, ran: Option<&RanOn>, providers: &[Provider]) -> RanModel {
  match ran.filter(|ran| !ran.provider.is_empty() && !ran.model.is_empty()) {
    None => RanModel {
      shown: requested.to_owned(),
      requested: None,
      fallback: false,
      changed: false,
    },
    Some(ran) => RanModel {
      shown: format!("{}/{}", ran.provider, ran.model),
      requested: (!requested.is_empty() && !asked(requested, ran)).then(|| requested.to_owned()),
      fallback: fell_back(requested, ran, head_provider(providers)),
      changed: ran.changed,
    },
  }
}

/// An item's model: what its work last went out on, and the model it asked for, `auto` above all, where
/// nothing has gone out yet; nothing where it asked for the default and nothing has gone out.
pub fn item_ran_model(
  requested: Option<&str>,
  ran: Option<&RanOn>,
  providers: &[Provider],
) -> Option<RanModel> {
  (requested.is_some() || ran.is_some()).then(|| ran_model(requested.unwrap_or(""), ran, providers))
}
