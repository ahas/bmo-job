//! A Claude Workflow's branches divided (docs/verbs.md, "What a job cost"): each branch's length into the
//! stretch some call it made was out and the rest, which is the model's, read out of the records the workflow
//! kept under its project folder.

use bmo_job::verbs::branch_split::{BranchSplit, BranchTool, branch_split, branch_split_lines};
use serde_json::{Value, json};

fn at(seconds: i64) -> String {
  chrono::DateTime::from_timestamp(1_788_793_931 + seconds, 0)
    .unwrap()
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn moment(seconds: i64) -> Value {
  json!({ "type": "assistant", "timestamp": at(seconds), "message": { "content": [{ "type": "text", "text": "…" }] } })
}

fn used(seconds: i64, id: &str, name: &str) -> Value {
  json!({ "type": "assistant", "timestamp": at(seconds), "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": {} }] } })
}

fn answered(seconds: i64, ids: &[&str]) -> Value {
  let content: Vec<Value> = ids
    .iter()
    .map(|id| json!({ "type": "tool_result", "tool_use_id": id, "content": "ok" }))
    .collect();

  json!({ "type": "user", "timestamp": at(seconds), "message": { "content": content } })
}

fn tool(name: &str, seconds: i64, times: i64, open: i64) -> BranchTool {
  BranchTool {
    name: name.to_owned(),
    seconds,
    times,
    open,
  }
}

fn clean() -> Vec<Value> {
  vec![
    moment(0),
    used(10, "toolu_1", "Bash"),
    answered(30, &["toolu_1"]),
    used(90, "toolu_2", "Read"),
    answered(95, &["toolu_2"]),
    used(150, "toolu_3", "Bash"),
    answered(160, &["toolu_3"]),
    moment(400),
  ]
}

#[test]
fn one_branch_divides_into_its_tools_and_the_model_writing_whatever_order_its_lines_are_in() {
  let clean = clean();

  assert_eq!(
    branch_split(&clean),
    Some(BranchSplit {
      seconds: 400,
      tooling: 35,
      generating: 365,
      calls: 3,
      open: 0,
      tools: vec![tool("Bash", 30, 2, 0), tool("Read", 5, 1, 0)],
    })
  );

  let shuffled: Vec<Value> = [4, 7, 1, 0, 2, 3, 5, 6].iter().map(|at| clean[*at].clone()).collect();

  assert_eq!(branch_split(&shuffled), branch_split(&clean));
}

#[test]
fn calls_out_side_by_side_count_once_and_apart_each() {
  let cut = [
    moment(0),
    used(10, "toolu_1", "Bash"),
    answered(30, &["toolu_1"]),
    used(100, "toolu_2", "Bash"),
    moment(300),
  ];

  assert_eq!(
    branch_split(&cut),
    Some(BranchSplit {
      seconds: 300,
      tooling: 220,
      generating: 80,
      calls: 2,
      open: 1,
      tools: vec![tool("Bash", 220, 2, 1)],
    })
  );

  let together = [
    moment(0),
    used(10, "toolu_1", "Bash"),
    used(10, "toolu_2", "Read"),
    used(20, "toolu_3", "Grep"),
    answered(60, &["toolu_1", "toolu_2", "toolu_3"]),
    moment(200),
  ];
  let split = branch_split(&together).unwrap();

  assert_eq!((split.tooling, split.generating), (50, 150));
  assert_eq!(split.tools, [tool("Bash", 50, 1, 0), tool("Read", 50, 1, 0), tool("Grep", 40, 1, 0)]);

  let apart = [
    moment(0),
    used(10, "toolu_1", "Bash"),
    answered(30, &["toolu_1"]),
    used(100, "toolu_2", "Bash"),
    used(110, "toolu_3", "Bash"),
    answered(140, &["toolu_2"]),
    answered(150, &["toolu_3"]),
    moment(300),
  ];
  let split = branch_split(&apart).unwrap();

  assert_eq!(split.tooling, 70);
  assert_eq!(split.tools, [tool("Bash", 100, 3, 0)]);
}

#[test]
fn the_edges_of_a_branch_s_division() {
  let proxied = json!({
    "type": "assistant",
    "timestamp": at(10),
    "message": { "content": [{ "type": "tool_use", "id": "toolu_book", "name": "mcp__toolbook__call", "input": { "tool": "web/naver_price", "arguments": { "query": "RTX 5090" } } }] },
  });

  assert_eq!(
    branch_split(&[moment(0), proxied, answered(16, &["toolu_book"]), moment(100)])
      .unwrap()
      .tools,
    [tool("mcp__web__naver_price", 6, 1, 0)]
  );
  assert_eq!(
    branch_split(&[moment(0), moment(240)]),
    Some(BranchSplit {
      seconds: 240,
      tooling: 0,
      generating: 240,
      calls: 0,
      open: 0,
      tools: vec![],
    })
  );
  assert_eq!(
    branch_split(&[
      moment(0),
      used(10, "toolu_1", "Bash"),
      answered(30, &["toolu_1"]),
      answered(90, &["toolu_1"]),
      moment(200)
    ])
    .unwrap()
    .tooling,
    20
  );
  assert_eq!(branch_split(&[moment(0), answered(30, &["toolu_gone"]), moment(200)]).unwrap().calls, 0);
  assert_eq!(branch_split(&[]), None);
  assert_eq!(
    branch_split(&[json!({ "type": "attachment", "message": { "content": "no moment on this one" } })]),
    None
  );
  assert_eq!(branch_split(&[moment(0), moment(0)]), None);
}

#[test]
fn a_workflow_s_branches_are_read_out_of_its_project_folder() {
  let folder = tempfile::tempdir().unwrap();
  let project = folder.path();
  let session = project.join("s1");
  let branches = session.join("subagents/workflows/wf_one");

  assert_eq!(branch_split_lines(project, None, false).unwrap_err().code, "no_workflow_records");

  std::fs::create_dir_all(session.join("workflows")).unwrap();
  std::fs::create_dir_all(&branches).unwrap();
  std::fs::write(
    session.join("workflows/wf_one.json"),
    json!({ "workflowName": "review", "timestamp": "2026-10-10T01:00:00Z", "status": "completed",
      "workflowProgress": [{ "index": 1, "label": "scout", "agentId": "a1", "model": "m", "tokens": 12345 }] })
    .to_string(),
  )
  .unwrap();
  std::fs::write(branches.join("agent-a1.meta.json"), r#"{"agentType":"scout"}"#).unwrap();
  std::fs::write(
    branches.join("agent-a1.jsonl"),
    clean().iter().map(Value::to_string).collect::<Vec<_>>().join("\n"),
  )
  .unwrap();

  assert_eq!(
    branch_split_lines(project, None, true).unwrap(),
    ["wf_one · review · 2026-10-10T01:00:00Z · 1 branches · session s1"]
  );

  let lines = branch_split_lines(project, Some("wf_one"), false).unwrap();

  assert_eq!(lines[0], "wf_one · review · 2026-10-10T01:00:00Z · completed · 1 branches");
  assert_eq!(lines[1], "branch 1 · scout · m · scout · 12,345 tokens");
  assert!(
    lines[2].contains("3 calls") && lines[2].contains("(9%") && lines[2].contains("(91%)"),
    "{lines:?}"
  );
  assert!(
    lines[3].contains("Bash") && lines[3].contains("×2") && lines[3].contains("Read"),
    "{lines:?}"
  );
  assert_eq!(
    branch_split_lines(project, Some("wf_two"), false).unwrap_err().code,
    "workflow_not_found"
  );
}
