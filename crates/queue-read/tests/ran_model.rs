//! The model an item ran on (docs/reading.md, "The model an item ran on").

use job_agency::Provider;
use job_codes::todo_reading::ModelMark;
use queue_read::queue::RanOn;
use queue_read::ran_model::{RanModel, head_provider, item_ran_model, ran_model};

fn provider(name: &str, enabled: bool) -> Provider {
  Provider {
    name: name.to_owned(),
    enabled,
    registered: false,
    coworker: false,
    lines: vec![],
  }
}

/// The provider order: one that is off, then the head, then the rest.
fn providers() -> Vec<Provider> {
  vec![provider("off", false), provider("claude-code", true), provider("registered", true)]
}

fn road(provider: &str, model: &str, changed: bool) -> RanOn {
  RanOn {
    provider: provider.to_owned(),
    model: model.to_owned(),
    changed,
  }
}

fn read(requested: &str, ran: Option<&RanOn>) -> RanModel {
  ran_model(requested, ran, &providers())
}

#[test]
fn the_head_is_the_first_provider_that_is_on() {
  assert_eq!(head_provider(&providers()), Some("claude-code"));
  assert_eq!(head_provider(&[provider("off", false)]), None);
}

#[test]
fn a_road_the_order_fell_to_is_marked_and_the_request_said_only_in_detail() {
  let model = read("claude-code::claude-opus-5-5", Some(&road("registered", "flash-next-coder", true)));

  assert_eq!(model.shown, "registered/flash-next-coder");
  assert_eq!(model.marks(false), vec![ModelMark::Fallback, ModelMark::Changed]);
  assert_eq!(
    model.marks(true),
    vec![
      ModelMark::Fallback,
      ModelMark::Changed,
      ModelMark::Requested {
        model: "claude-code::claude-opus-5-5".to_owned()
      }
    ]
  );
}

#[test]
fn the_road_asked_for_shows_alone() {
  for (requested, ran) in [
    ("claude-code::claude-opus-5-5", road("claude-code", "claude-opus-5-5", false)),
    ("claude-opus-5-5", road("claude-code", "claude-opus-5-5", false)),
    ("registered::flash-next-coder", road("registered", "flash-next-coder", false)),
  ] {
    let model = read(requested, Some(&ran));

    assert_eq!(model.shown, format!("{}/{}", ran.provider, ran.model));
    assert!(model.marks(true).is_empty(), "{requested}");
  }
}

#[test]
fn auto_shows_the_model_it_settled_on() {
  let settled = read("auto", Some(&road("claude-code", "claude-opus-5-5", false)));

  assert_eq!(settled.shown, "claude-code/claude-opus-5-5");
  assert!(settled.marks(false).is_empty());
  assert_eq!(
    settled.marks(true),
    vec![ModelMark::Requested {
      model: "auto".to_owned()
    }]
  );
  assert_eq!(
    read("auto", Some(&road("registered", "flash-next-coder", false))).marks(false),
    vec![ModelMark::Fallback]
  );
}

#[test]
fn another_model_on_the_provider_asked_for_is_no_fallback() {
  let model = read("claude-code::claude-opus-5-5", Some(&road("claude-code", "claude-sonnet-5", false)));

  assert_eq!(model.shown, "claude-code/claude-sonnet-5");
  assert!(!model.fallback);
  assert_eq!(model.requested.as_deref(), Some("claude-code::claude-opus-5-5"));
}

#[test]
fn where_no_provider_is_on_a_request_naming_none_falls_back_nowhere() {
  let model =
    ran_model("auto", Some(&road("registered", "flash-next-coder", false)), &[provider("off", false)]);

  assert!(!model.fallback);
}

#[test]
fn nothing_gone_out_shows_the_request() {
  assert_eq!(read("auto", None).shown, "auto");
  assert!(read("auto", None).marks(true).is_empty());
  assert_eq!(item_ran_model(None, None, &providers()), None);
  assert_eq!(
    item_ran_model(None, Some(&road("codex", "gpt-6", false)), &providers()).map(|model| model.marks(true)),
    Some(vec![ModelMark::Fallback])
  );
}
