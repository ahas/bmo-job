//! A control turn's answer read as a plan (docs/queue.md, "The work-order runner"), and what a failed run says
//! of its road and the names a work order's task runs under.

mod support;

use bmo_job::job_run::plan_check::{
  Roster, chosen_model, nonempty, object, plan_title, split_stages, stage_kind, written_stages,
};
use bmo_job::job_run::recovery::{RoadFailure, task_key, text_failure, unit_step};
use job_agency::Tier;
use job_codes::WorkOrderRefusal;
use job_codes::work_order_edits::PlanPart;
use serde_json::{Value, json};
use support::queue::policy;

// A ladder of two tiers: one a co-worker is lent at and one it is not.
fn roster() -> Roster {
  Roster {
    tiers: vec![
      Tier {
        name: "Frontier".into(),
        coworker: false,
      },
      Tier {
        name: "Fine".into(),
        coworker: true,
      },
    ],
    roles: vec![],
  }
}

#[test]
fn an_answer_is_read_as_one_object_with_its_fence_taken_off() {
  assert!(object("```json\n{\"tasks\":[]}\n```").is_ok());
  assert_eq!(object("[1]").unwrap_err(), WorkOrderRefusal::PlanNotObject.to_string());
  assert!(object("{\"stages\":[").unwrap_err().starts_with("JSON Parse error: "));
}

#[test]
fn a_text_field_is_refused_blank_too_long_or_not_text() {
  let empty = |part| WorkOrderRefusal::PlanPartEmpty { part }.to_string();

  assert_eq!(nonempty(Some(&json!("  work  ")), PlanPart::TaskInstruction, 10), Ok("work".to_owned()));
  assert_eq!(
    nonempty(Some(&json!(" ")), PlanPart::TaskInstruction, 10),
    Err(empty(PlanPart::TaskInstruction))
  );
  assert!(nonempty(Some(&json!("abc")), PlanPart::Model, 2).is_err());
  assert!(nonempty(Some(&json!(3)), PlanPart::Model, 2).is_err());
  assert!(nonempty(None, PlanPart::Model, 2).is_err());
}

#[test]
fn a_plan_holds_as_many_stages_and_tasks_as_it_designs_but_no_empty_stage() {
  let stage = |tasks: usize| json!({ "title": "Look", "tasks": vec![json!({ "role": "scout" }); tasks] });
  let plan =
    |stages: Vec<Value>| object(&json!({ "stages": stages }).to_string()).expect("the plan is an object");
  let wide = written_stages(&plan(vec![stage(12)])).expect("the plan holds");

  assert_eq!(wide.len(), 1);
  assert_eq!(wide[0].2.len(), 12);
  assert_eq!(written_stages(&plan(vec![stage(1); 30])).map(|stages| stages.len()), Ok(30));
  assert_eq!(written_stages(&plan(vec![])), Ok(vec![]));
  assert_eq!(
    written_stages(&plan(vec![stage(2), stage(0)])),
    Err(WorkOrderRefusal::PlanStageEmpty.to_string())
  );
  assert_eq!(
    written_stages(&object("{}").expect("an empty object")),
    Err(WorkOrderRefusal::PlanNoStages.to_string())
  );
}

#[test]
fn a_stage_holding_a_builder_beside_others_is_split_the_others_first() {
  let task = |role: &str| json!({ "role": role });
  let split = split_stages(vec![
    ("Look".to_owned(), "review", vec![task("builder"), task("scout"), task("builder")]),
    ("Build".to_owned(), "investigate", vec![task("builder")]),
  ]);

  assert_eq!(
    split,
    vec![
      ("Look".to_owned(), "review", vec![task("scout")]),
      ("Look (2)".to_owned(), "review", vec![task("builder")]),
      ("Look (3)".to_owned(), "review", vec![task("builder")]),
      ("Build".to_owned(), "investigate", vec![task("builder")]),
    ]
  );
}

#[test]
fn a_stage_builder_given_no_choice_runs_at_the_stage_builders_tier() {
  let policy = policy();
  let roster = roster();
  let planner = "claude-code::claude-opus-5-5";
  let stage = Some(policy.queue.stage_builder_tier.clone());
  let chosen = |role: &str, named: Option<&str>| {
    chosen_model(&roster, &policy, role, named.map(str::to_owned), planner)
  };

  assert_eq!(chosen("builder", None), stage);
  assert_eq!(chosen("builder", Some(planner)), stage);
  assert_eq!(chosen("builder", Some("Frontier")), Some("Frontier".to_owned()));
  assert_eq!(chosen("scout", None), None);
  assert_eq!(chosen("verifier", Some("Frontier")), None);
  assert_eq!(chosen("verifier", Some("Fine")), Some("Fine".to_owned()));
}

#[test]
fn a_stage_kind_the_order_does_not_know_reads_as_finding_things_out() {
  assert_eq!(stage_kind(Some(&json!(" review "))), "review");
  assert_eq!(stage_kind(Some(&json!("investigate"))), "investigate");
  assert_eq!(stage_kind(Some(&json!("audit"))), "investigate");
  assert_eq!(stage_kind(Some(&json!(2))), "investigate");
  assert_eq!(stage_kind(None), "investigate");
}

#[test]
fn a_task_name_is_trimmed_cut_to_a_line_or_left_out() {
  assert_eq!(plan_title(Some(&json!("  Trace gate road "))), Some("Trace gate road".to_owned()));
  assert_eq!(plan_title(Some(&json!(" "))), None);
  assert_eq!(plan_title(Some(&json!(3))), None);
  assert_eq!(plan_title(Some(&json!("é".repeat(250)))).map(|title| title.chars().count()), Some(200));
}

#[test]
fn a_failure_is_read_as_the_road_it_closes() {
  assert_eq!(text_failure("Error: claude usage limit reached"), Some(RoadFailure::Quota));
  assert_eq!(text_failure("Error: 503 Service Unavailable"), Some(RoadFailure::Outage));
  assert_eq!(text_failure("Error: 401 unauthorized"), Some(RoadFailure::Access));
  assert_eq!(
    text_failure("SelectedRouteUnavailableError: job-worker: selected codex route did not answer"),
    Some(RoadFailure::Outage)
  );
  assert_eq!(text_failure("JobWaitingOnPerson: the run is waiting on a person"), None);
}

#[test]
fn a_work_orders_step_is_read_off_its_unit_and_its_round() {
  assert_eq!(unit_step("bmo-work-9-stage-0-0-1").as_deref(), Some("stage-0-0"));
  assert_eq!(unit_step("bmo-job-41"), None);

  let order = json!({ "round": 2 });

  assert_eq!(task_key(&order, &json!({ "id": "stage-0-0" })), "r2-stage-0-0");
  assert_eq!(task_key(&order, &json!({ "id": "r1-stage-0-0" })), "r1-stage-0-0");
  assert_eq!(task_key(&json!({}), &json!({ "id": "stage-0-0" })), "stage-0-0");
}
