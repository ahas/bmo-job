//! Every code and every kind is listed in the API document under its enum, every code is snake_case and
//! unique, and a refusal, a fact and a kind take the shapes the document gives them (docs/api.md,
//! "Codes").

use std::collections::{BTreeMap, BTreeSet};

use axum::http::StatusCode;
use job_base::Refusal;
use job_codes::agents::AgentRefusal;
use job_codes::branch_split::BranchSplitRefusal;
use job_codes::chat_health::{FailureFact, RunDeath};
use job_codes::checkout_holds::{
  CheckoutNote, CheckoutPlace, CheckoutStop, GitFailure, GitStep, LinePlace, PlaceReason, ReleaseWait,
};
use job_codes::control_answers::{ControlRefusal, FieldForm};
use job_codes::job_ask::{JobAskAnswer, JobAskRefusal};
use job_codes::job_ended::{JobEnding, JobFact};
use job_codes::job_questions::{Lapse, PaneOption, PaneWindow, QuestionAnswer, QuestionFact};
use job_codes::job_roads::{RoadFact, RoadFailure, SwitchCause, WorkOutput};
use job_codes::job_wait::{JobWaitRefusal, WaitTarget};
use job_codes::providers::ProviderRefusal;
use job_codes::queue_facts::{HoldCause, QueueFact, WaitStanding};
use job_codes::queue_told::QueueTold;
use job_codes::room_removal::RoomHoldings;
use job_codes::run_stop::{RunStopAnswer, RunStopRefusal, StopState, TargetStop};
use job_codes::span_words::{SpanName, TokenKind};
use job_codes::talk::{TalkRefusal, Unanswered};
use job_codes::task::{LaunchNote, TaskAnswer, TaskRefusal};
use job_codes::todo_blocks::BlockHead;
use job_codes::todo_reading::{HeldWord, LineMark, ModelMark, QuestionState, RowState};
use job_codes::todo_verbs::{
  AfterWrong, ClosingPart, EditedPart, FilingPart, ImpactDifference, OrderHold, ResumeCondition,
  TodoAnswer, TodoFact, TodoRefusal, Verdict, WrittenRecord,
};
use job_codes::units::UnitRefusal;
use job_codes::work_lines::{JobLabel, PassReason, WorkStep};
use job_codes::work_order_edits::{PlanPart, WorkOrderRefusal};
use job_codes::work_order_lines::{QueueHold, WorkOrderAnswer};
use job_codes::work_order_stops::{RetryStep, RunawayAction, RunawayCause, StopReason, WorkOrderStop};
use job_codes::{Code, Fact, JobError, Refused};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::json;
use strum::VariantNames;

const API: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api.md");

/// Whether an enum's names are codes, unique across the service, or kinds, unique within their list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Names {
  Codes,
  Kinds,
}

/// Every enum of the crate by the name the document lists it under, with its names.
fn enums() -> Vec<(&'static str, Names, &'static [&'static str])> {
  use Names::{Codes, Kinds};

  vec![
    ("ProviderRefusal", Codes, ProviderRefusal::VARIANTS),
    ("WorkOrderRefusal", Codes, WorkOrderRefusal::VARIANTS),
    ("RunStopRefusal", Codes, RunStopRefusal::VARIANTS),
    ("TodoRefusal", Codes, TodoRefusal::VARIANTS),
    ("JobAskRefusal", Codes, JobAskRefusal::VARIANTS),
    ("JobWaitRefusal", Codes, JobWaitRefusal::VARIANTS),
    ("TalkRefusal", Codes, TalkRefusal::VARIANTS),
    ("ControlRefusal", Codes, ControlRefusal::VARIANTS),
    ("BranchSplitRefusal", Codes, BranchSplitRefusal::VARIANTS),
    ("AgentRefusal", Codes, AgentRefusal::VARIANTS),
    ("UnitRefusal", Codes, UnitRefusal::VARIANTS),
    ("TaskRefusal", Codes, TaskRefusal::VARIANTS),
    ("QuestionAnswer", Codes, QuestionAnswer::VARIANTS),
    ("TaskAnswer", Codes, TaskAnswer::VARIANTS),
    ("RunStopAnswer", Codes, RunStopAnswer::VARIANTS),
    ("TodoAnswer", Codes, TodoAnswer::VARIANTS),
    ("RoomHoldings", Codes, RoomHoldings::VARIANTS),
    ("WorkOrderAnswer", Codes, WorkOrderAnswer::VARIANTS),
    ("JobAskAnswer", Codes, JobAskAnswer::VARIANTS),
    ("JobFact", Codes, JobFact::VARIANTS),
    ("QuestionFact", Codes, QuestionFact::VARIANTS),
    ("FailureFact", Codes, FailureFact::VARIANTS),
    ("WorkOrderStop", Codes, WorkOrderStop::VARIANTS),
    ("CheckoutStop", Codes, CheckoutStop::VARIANTS),
    ("QueueFact", Codes, QueueFact::VARIANTS),
    ("RoadFact", Codes, RoadFact::VARIANTS),
    ("TodoFact", Codes, TodoFact::VARIANTS),
    ("PlanPart", Kinds, PlanPart::VARIANTS),
    ("StopState", Kinds, StopState::VARIANTS),
    ("AfterWrong", Kinds, AfterWrong::VARIANTS),
    ("EditedPart", Kinds, EditedPart::VARIANTS),
    ("ClosingPart", Kinds, ClosingPart::VARIANTS),
    ("Verdict", Kinds, Verdict::VARIANTS),
    ("ImpactDifference", Kinds, ImpactDifference::VARIANTS),
    ("FilingPart", Kinds, FilingPart::VARIANTS),
    ("ResumeCondition", Kinds, ResumeCondition::VARIANTS),
    ("WrittenRecord", Kinds, WrittenRecord::VARIANTS),
    ("LineMark", Kinds, LineMark::VARIANTS),
    ("HeldWord", Kinds, HeldWord::VARIANTS),
    ("RowState", Kinds, RowState::VARIANTS),
    ("QuestionState", Kinds, QuestionState::VARIANTS),
    ("ModelMark", Kinds, ModelMark::VARIANTS),
    ("Lapse", Kinds, Lapse::VARIANTS),
    ("PaneWindow", Kinds, PaneWindow::VARIANTS),
    ("PaneOption", Kinds, PaneOption::VARIANTS),
    ("OrderHold", Kinds, OrderHold::VARIANTS),
    ("QueueHold", Kinds, QueueHold::VARIANTS),
    ("WorkStep", Kinds, WorkStep::VARIANTS),
    ("PassReason", Kinds, PassReason::VARIANTS),
    ("JobLabel", Kinds, JobLabel::VARIANTS),
    ("Unanswered", Kinds, Unanswered::VARIANTS),
    ("ReleaseWait", Kinds, ReleaseWait::VARIANTS),
    ("LinePlace", Kinds, LinePlace::VARIANTS),
    ("PlaceReason", Kinds, PlaceReason::VARIANTS),
    ("StopReason", Kinds, StopReason::VARIANTS),
    ("RunawayCause", Kinds, RunawayCause::VARIANTS),
    ("RunawayAction", Kinds, RunawayAction::VARIANTS),
    ("RetryStep", Kinds, RetryStep::VARIANTS),
    ("CheckoutNote", Kinds, CheckoutNote::VARIANTS),
    ("CheckoutPlace", Kinds, CheckoutPlace::VARIANTS),
    ("GitFailure", Kinds, GitFailure::VARIANTS),
    ("GitStep", Kinds, GitStep::VARIANTS),
    ("RunDeath", Kinds, RunDeath::VARIANTS),
    ("SpanName", Kinds, SpanName::VARIANTS),
    ("TokenKind", Kinds, TokenKind::VARIANTS),
    ("BlockHead", Kinds, BlockHead::VARIANTS),
    ("HoldCause", Kinds, HoldCause::VARIANTS),
    ("WaitStanding", Kinds, WaitStanding::VARIANTS),
    ("WaitTarget", Kinds, WaitTarget::VARIANTS),
    ("JobEnding", Kinds, JobEnding::VARIANTS),
    ("QueueTold", Kinds, QueueTold::VARIANTS),
    ("LaunchNote", Kinds, LaunchNote::VARIANTS),
    ("FieldForm", Kinds, FieldForm::VARIANTS),
    ("SwitchCause", Kinds, SwitchCause::VARIANTS),
    ("RoadFailure", Kinds, RoadFailure::VARIANTS),
    ("WorkOutput", Kinds, WorkOutput::VARIANTS),
  ]
}

/// The names the document's "Codes" section lists, by the enum named in the heading above each table.
fn listed() -> BTreeMap<String, Vec<String>> {
  let text = std::fs::read_to_string(API).expect("the API document reads");
  let mut listed: BTreeMap<String, Vec<String>> = BTreeMap::new();
  let mut in_codes = false;
  let mut heading: Option<(HeadingLevel, String, Option<String>)> = None;
  let mut current: Option<String> = None;
  let mut in_head = false;
  let mut cell = 0;

  for event in Parser::new_ext(&text, Options::ENABLE_TABLES) {
    match event {
      Event::Start(Tag::Heading { level, .. }) => heading = Some((level, String::new(), None)),
      Event::Text(said) if heading.is_some() => heading.as_mut().unwrap().1.push_str(&said),
      Event::Code(name) if heading.is_some() => heading.as_mut().unwrap().2 = Some(name.to_string()),
      Event::End(TagEnd::Heading(_)) => {
        let (level, said, name) = heading.take().unwrap();

        if level == HeadingLevel::H2 {
          in_codes = said.trim() == "Codes";
        }

        current = (in_codes && level == HeadingLevel::H4).then_some(name).flatten();

        if let Some(name) = &current {
          assert!(listed.insert(name.clone(), Vec::new()).is_none(), "{name} is listed twice");
        }
      }
      Event::Start(Tag::Table(_)) if in_codes => {
        assert!(current.is_some(), "a table of the codes stands under no enum's heading");
      }
      Event::Start(Tag::TableHead) => in_head = true,
      Event::End(TagEnd::TableHead) => in_head = false,
      Event::Start(Tag::TableRow) => cell = 0,
      Event::Start(Tag::TableCell) => cell += 1,
      Event::Code(name) if in_codes && !in_head && cell == 1 => {
        listed.get_mut(current.as_ref().unwrap()).unwrap().push(name.to_string());
      }
      _ => {}
    }
  }

  listed
}

fn snake_case(name: &str) -> bool {
  !name.is_empty()
    && name.chars().all(|character| character.is_ascii_lowercase() || character == '_')
    && !name.starts_with('_')
    && !name.ends_with('_')
    && !name.contains("__")
}

#[test]
fn every_enum_is_listed_with_exactly_its_names() {
  let listed = listed();
  let enums = enums();
  let named: BTreeSet<&str> = enums.iter().map(|(name, ..)| *name).collect();
  let headed: BTreeSet<&str> = listed.keys().map(String::as_str).collect();

  assert_eq!(named.len(), enums.len(), "an enum is named twice in the test");
  assert_eq!(headed, named, "the document's enums and the crate's differ");

  for (name, _, variants) in &enums {
    let rows = &listed[*name];
    let in_document: BTreeSet<&str> = rows.iter().map(String::as_str).collect();
    let in_crate: BTreeSet<&str> = variants.iter().copied().collect();

    assert_eq!(in_document.len(), rows.len(), "{name} lists a name twice");
    assert_eq!(in_document, in_crate, "{name}: the document's names and the crate's differ");
  }
}

#[test]
fn every_name_is_snake_case_and_every_code_is_unique() {
  let mut codes = BTreeMap::new();

  for (name, names, variants) in enums() {
    for variant in variants {
      assert!(snake_case(variant), "{name}::{variant} is not snake_case");

      if names == Names::Codes {
        let earlier = codes.insert(*variant, name);

        assert!(earlier.is_none(), "{variant} is a code of both {} and {name}", earlier.unwrap());
      }
    }
  }
}

#[test]
fn a_refusal_answers_its_status_its_code_and_an_english_sentence() {
  let absent: Refusal = JobError::from(WorkOrderRefusal::TaskAbsent {
    task: "build-2".into(),
  })
  .into();
  let terms = JobError::from(TodoRefusal::BannedTerms {
    terms: vec!["one".into(), "two".into()],
  });
  let unhold = JobError::from(TodoRefusal::JobCannotUnhold);

  assert_eq!(absent.status, StatusCode::NOT_FOUND);
  assert_eq!(absent.code, "task_absent");
  assert_eq!(absent.error, "the order has no task build-2");
  assert_eq!(terms.status(), StatusCode::BAD_REQUEST);
  assert_eq!(terms.code(), "banned_terms");
  assert_eq!(terms.to_string(), "the words use terms the owner has set aside: one, two");
  assert_eq!(unhold.refusal().status, StatusCode::FORBIDDEN);
  assert_eq!(
    AgentRefusal::CheckoutNotRemoved { worker: "w".into() }.status(),
    StatusCode::INTERNAL_SERVER_ERROR
  );
}

#[test]
fn a_fact_and_an_answer_carry_their_code_beside_their_fields_and_read_back() {
  let ended = Fact::Job(JobFact::JobEnded {
    reason: JobEnding::LiveTasks,
    job_id: 7,
    checkout: "base".into(),
  });
  let written = serde_json::to_value(&ended).unwrap();

  assert_eq!(
    written,
    json!({"code": "job_ended", "reason": "live_tasks", "job_id": 7, "checkout": "base"})
  );
  assert_eq!(serde_json::from_value::<Fact>(written).unwrap(), ended);
  assert_eq!(ended.code(), "job_ended");

  let stop = Fact::Checkout(CheckoutStop::ParkedNotMerged {
    item: 3,
    why: GitFailure::StepFailed {
      step: GitStep::TakeBack,
      said: "conflict".into(),
    },
  });
  let written = serde_json::to_value(&stop).unwrap();

  assert_eq!(
    written,
    json!({"code": "parked_not_merged", "item": 3, "why": {"kind": "step_failed", "step": "take_back", "said": "conflict"}})
  );
  assert_eq!(serde_json::from_value::<Fact>(written).unwrap(), stop);
  assert_eq!(serde_json::to_value(TodoAnswer::QueuePaused).unwrap(), json!({"code": "queue_paused"}));
}

#[test]
fn a_kind_is_a_word_or_an_object_holding_it_where_its_list_carries_fields() {
  let asked = RunStopAnswer::AllAsked {
    asked: 2,
    confirmed: 1,
    targets: vec![
      TargetStop {
        target: "job 4".into(),
        state: StopState::Ended,
      },
      TargetStop {
        target: "job 5".into(),
        state: StopState::Refused { why: "gone".into() },
      },
    ],
  };

  assert_eq!(
    serde_json::to_value(&asked).unwrap(),
    json!({"code": "all_asked", "asked": 2, "confirmed": 1, "targets": [
      {"target": "job 4", "state": {"kind": "ended"}},
      {"target": "job 5", "state": {"kind": "refused", "why": "gone"}},
    ]})
  );
  assert_eq!(serde_json::to_value(HeldWord::Pausing).unwrap(), json!("pausing"));
  assert_eq!(WorkStep::Final.to_string(), "final");
}
