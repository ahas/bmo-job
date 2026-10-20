//! Work orders and the tasks their steps make, through the real server (docs/api.md, "Work orders and tasks").

mod support;

use bmo_job_client::{TaskChange, TaskQuery, TaskState, WorkOrderBody, WorkOrderChange, WorkOrderState};

use support::Service;

fn body(steps: &[&str]) -> WorkOrderBody {
  WorkOrderBody {
    purpose: "ship the parser".to_owned(),
    directive: "write it, then test it".to_owned(),
    steps: steps.iter().map(|step| (*step).to_owned()).collect(),
    state: None,
  }
}

fn titles(tasks: &[bmo_job_client::Task]) -> Vec<(i64, &str)> {
  tasks.iter().map(|task| (task.stage, task.title.as_str())).collect()
}

#[tokio::test]
async fn saving_steps_makes_and_keeps_the_tasks() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "parser").await;
  assert_eq!(client.work_order(item.id).await.expect_err("none yet").code(), Some("not-found"));

  let made = service
    .raw(reqwest::Method::PUT, &format!("/items/{}/work-order", item.id))
    .json(&body(&["design", "build"]))
    .send()
    .await
    .expect("answered");
  assert_eq!(made.status(), 201);
  let tasks = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&tasks), [(1, "design"), (2, "build")]);

  let design = tasks[0].clone();
  let current = TaskChange {
    state: Some(TaskState::Done),
    assignee_name: Some(Some("designer".to_owned())),
    ..TaskChange::default()
  };
  client.change_task(item.id, design.id, &current).await.expect("moved");

  let order = client
    .put_work_order(item.id, &body(&["review", "design", "build"]))
    .await
    .expect("replaced");
  assert_eq!(order.state, WorkOrderState::Draft);
  let tasks = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&tasks), [(1, "review"), (2, "design"), (3, "build")]);
  let kept = tasks.iter().find(|task| task.title == "design").expect("design stands");
  assert_eq!(
    (kept.id, kept.state, kept.assignee_name.as_deref()),
    (design.id, TaskState::Done, Some("designer"))
  );

  let under_way = TaskQuery {
    current: Some(true),
    ..TaskQuery::default()
  };
  assert_eq!(titles(&client.tasks(item.id, &under_way).await.expect("listed").items), [(1, "review")]);

  let fewer = WorkOrderChange {
    steps: Some(vec!["build".to_owned()]),
    ..WorkOrderChange::default()
  };
  client.change_work_order(item.id, &fewer).await.expect("changed");
  assert_eq!(
    titles(&client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items),
    [(1, "build")]
  );

  let twice = client
    .put_work_order(item.id, &body(&["a", "a"]))
    .await
    .expect_err("a step stands twice");
  assert_eq!(twice.code(), Some("bad-request"));
}

#[tokio::test]
async fn a_work_order_moves_by_its_moves_and_a_done_one_holds_its_tasks() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "parser").await;
  client.put_work_order(item.id, &body(&["design"])).await.expect("made");
  let to = |state| WorkOrderChange {
    state: Some(state),
    ..WorkOrderChange::default()
  };
  assert_eq!(
    client
      .change_work_order(item.id, &to(WorkOrderState::Running))
      .await
      .expect_err("from draft")
      .code(),
    Some("conflict")
  );
  for state in [WorkOrderState::Saved, WorkOrderState::Running, WorkOrderState::Done] {
    assert_eq!(client.change_work_order(item.id, &to(state)).await.expect("moved").state, state);
  }
  let task = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items[0].clone();
  let skip = TaskChange {
    state: Some(TaskState::Skipped),
    ..TaskChange::default()
  };
  assert_eq!(
    client.change_task(item.id, task.id, &skip).await.expect_err("done").code(),
    Some("conflict")
  );
  client.change_work_order(item.id, &to(WorkOrderState::Running)).await.expect("reopened");
  let moved = TaskChange {
    stage: Some(4),
    ..skip
  };
  let task = client.change_task(item.id, task.id, &moved).await.expect("moved");
  assert_eq!((task.stage, task.state), (4, TaskState::Skipped));
  assert_eq!(client.task(item.id, task.id).await.expect("read"), task);
}
