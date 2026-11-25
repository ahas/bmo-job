//! Work orders and the tasks their steps make, through the real server (docs/api.md, "Work orders and tasks").

use crate::support;

use bmo_job_client::{
  EventQuery, Item, LeaseAsk, LeaseWait, NewStep, Task, TaskAfter, TaskChange, TaskQuery, TaskState,
  WorkOrderBody, WorkOrderChange, WorkOrderState,
};

use support::Service;

/// Leases the open item in `queue` to a holder.
async fn lease(service: &Service, queue: &str, item_id: i64) {
  service
    .client()
    .take_lease(
      &LeaseAsk {
        queue_name: queue.to_owned(),
        holder_name: "holder".to_owned(),
        ttl_ms: 60_000,
        assignee_name: None,
        item_id: Some(item_id),
        frozen: None,
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .expect("answered")
    .expect("the item is leased");
}

/// An open item in `queue`, leased by a holder.
async fn leased_item(service: &Service, queue: &str, title: &str) -> Item {
  let item = service.open_item(queue, title).await;
  lease(service, queue, item.id).await;
  item
}

fn step(title: &str) -> NewStep {
  NewStep {
    title: title.to_owned(),
    ..NewStep::default()
  }
}

fn waits_on(item_id: i64, repository: &str) -> TaskAfter {
  TaskAfter {
    item_id,
    repository: repository.to_owned(),
  }
}

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

  let once = client
    .put_work_order(item.id, &body(&["a", "a"]))
    .await
    .expect("the repeat is taken once");
  assert_eq!(once.steps, ["a"]);
  assert_eq!(
    titles(&client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items),
    [(1, "a")]
  );

  let repeated = WorkOrderChange {
    steps: Some(vec!["a".to_owned(), "b".to_owned(), "a".to_owned()]),
    ..WorkOrderChange::default()
  };
  let changed = client.change_work_order(item.id, &repeated).await.expect("the repeat is taken once");
  assert_eq!(changed.steps, ["a", "b"]);
}

#[tokio::test]
async fn a_build_list_written_twice_makes_its_steps_once() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "parser").await;
  let build = [
    "Correct the deletion set",
    "Delete dead TS and port callers",
    "Move tests into tests/",
    "Split bmo-server into layered crates",
    "Teach readers both layouts",
    "Move the layout",
  ];

  let made = client.put_work_order(item.id, &body(&build)).await.expect("made");
  assert_eq!(made.steps.iter().map(String::as_str).collect::<Vec<_>>(), build);
  let first = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(
    titles(&first),
    build.iter().enumerate().map(|(at, title)| (at as i64 + 1, *title)).collect::<Vec<_>>()
  );
  let keys = first.iter().map(|task| task.id).collect::<Vec<_>>();

  let written_twice: Vec<&str> = build.iter().chain(build.iter()).copied().collect();
  let again = client
    .put_work_order(item.id, &body(&written_twice))
    .await
    .expect("the repeat is taken once");
  assert_eq!(again.steps.iter().map(String::as_str).collect::<Vec<_>>(), build);

  let after = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&after), titles(&first));
  assert_eq!(after.iter().map(|task| task.id).collect::<Vec<_>>(), keys);
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

#[tokio::test]
async fn a_step_is_added_behind_a_leased_item_s_plan() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "parser").await;
  client
    .put_work_order(item.id, &body(&["investigate", "implement"]))
    .await
    .expect("made");
  lease(&service, "build", item.id).await;

  let added = service
    .raw(reqwest::Method::POST, &format!("/items/{}/work-order/steps", item.id))
    .json(&step("lexer"))
    .send()
    .await
    .expect("answered");
  assert_eq!(added.status(), 201);
  let task: Task = added.json().await.expect("a task");
  assert_eq!(
    (task.item_id, task.stage, task.title.as_str(), task.state, task.after.clone()),
    (item.id, 3, "lexer", TaskState::Waiting, None)
  );

  let order = client.work_order(item.id).await.expect("read");
  assert_eq!(order.steps, ["investigate", "implement", "lexer"]);
  let tasks = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&tasks), [(1, "investigate"), (2, "implement"), (3, "lexer")]);
  assert_eq!(tasks[2], task);

  let told = client
    .events_page(&EventQuery {
      kinds: Some("task.changed,work-order.changed".to_owned()),
      limit: Some(500),
      ..EventQuery::default()
    })
    .await
    .expect("read")
    .items;
  let last: Vec<(&str, Option<&str>)> = told
    .iter()
    .rev()
    .take(2)
    .rev()
    .map(|event| (event.kind.as_str(), event.data.get("title").and_then(|title| title.as_str())))
    .collect();
  assert_eq!(last, [("task.changed", Some("lexer")), ("work-order.changed", None)]);
  assert_eq!(
    told.last().expect("told").data["steps"],
    serde_json::json!(["investigate", "implement", "lexer"])
  );

  assert_eq!(
    client.add_step(item.id, &step("implement")).await.expect_err("already a step").code(),
    Some("conflict")
  );
  assert_eq!(
    client.add_step(item.id, &step("  ")).await.expect_err("blank").code(),
    Some("bad-request")
  );
  client
    .put_work_order(item.id, &body(&["investigate", "implement", "lexer"]))
    .await
    .expect("the same steps again, the added one among them");
  client
    .put_work_order(item.id, &body(&["investigate", "implement"]))
    .await
    .expect_err("the steps stand fixed while leased");
  assert_eq!(
    client.work_order(item.id).await.expect("read").steps,
    ["investigate", "implement", "lexer"]
  );
}

#[tokio::test]
async fn standing_makes_the_work_order_of_an_item_that_has_none() {
  let service = Service::start().await;
  let client = service.client();
  let item = leased_item(&service, "build", "parser").await;

  let refused = client.add_step(item.id, &step("lexer")).await.expect_err("no work order, no standing");
  assert_eq!((refused.code(), refused.status()), (Some("standing_required"), Some(422)));
  assert_eq!(client.work_order(item.id).await.expect_err("none made").code(), Some("not-found"));

  let task = client
    .add_step(
      item.id,
      &NewStep {
        standing: Some("the parser".to_owned()),
        ..step("lexer")
      },
    )
    .await
    .expect("added");
  assert_eq!((task.stage, task.title.as_str(), task.state), (2, "lexer", TaskState::Waiting));

  let order = client.work_order(item.id).await.expect("made");
  assert_eq!(
    (order.purpose.as_str(), order.directive.as_str(), order.state),
    ("parser", "the brief of parser", WorkOrderState::Draft)
  );
  assert_eq!(order.steps, ["the parser", "lexer"]);
  let tasks = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&tasks), [(1, "the parser"), (2, "lexer")]);
  assert!(tasks.iter().all(|task| task.state == TaskState::Waiting));

  let later = client
    .add_step(
      item.id,
      &NewStep {
        standing: Some("not read where a work order stands".to_owned()),
        ..step("tests")
      },
    )
    .await
    .expect("added");
  assert_eq!(later.stage, 3);
  assert_eq!(client.work_order(item.id).await.expect("read").steps, ["the parser", "lexer", "tests"]);

  let missing = client.add_step(i64::MAX, &step("lexer")).await.expect_err("no item");
  assert_eq!(missing.code(), Some("not-found"));
}

#[tokio::test]
async fn a_step_added_to_a_done_work_order_is_refused() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "parser").await;
  client.put_work_order(item.id, &body(&["design"])).await.expect("made");
  for state in [WorkOrderState::Saved, WorkOrderState::Running, WorkOrderState::Done] {
    let to = WorkOrderChange {
      state: Some(state),
      ..WorkOrderChange::default()
    };
    client.change_work_order(item.id, &to).await.expect("moved");
  }

  assert_eq!(
    client.add_step(item.id, &step("build")).await.expect_err("done").code(),
    Some("conflict")
  );
  assert_eq!(client.work_order(item.id).await.expect("read").steps, ["design"]);
}

#[tokio::test]
async fn a_step_waits_on_another_item_s_work_until_a_move_clears_it() {
  let service = Service::start().await;
  let client = service.client();
  let other = service.open_item("build", "schema").await;
  let item = leased_item(&service, "build", "parser").await;
  let standing = |title: &str| NewStep {
    standing: Some("the parser".to_owned()),
    ..step(title)
  };

  let task = client
    .add_step(
      item.id,
      &NewStep {
        after: Some(waits_on(other.id, "web")),
        ..standing("wire it in")
      },
    )
    .await
    .expect("added");
  assert_eq!(task.after, Some(waits_on(other.id, "web")));
  assert_eq!(
    client.task(item.id, task.id).await.expect("read").after,
    Some(waits_on(other.id, "web"))
  );

  let moved = TaskChange {
    after: Some(Some(waits_on(other.id, "api"))),
    ..TaskChange::default()
  };
  assert_eq!(
    client.change_task(item.id, task.id, &moved).await.expect("moved").after,
    Some(waits_on(other.id, "api"))
  );
  let current = TaskChange {
    state: Some(TaskState::Current),
    ..TaskChange::default()
  };
  let kept = client.change_task(item.id, task.id, &current).await.expect("moved");
  assert_eq!((kept.state, kept.after.clone()), (TaskState::Current, Some(waits_on(other.id, "api"))));

  let cleared = service
    .raw(reqwest::Method::PATCH, &format!("/items/{}/work-order/tasks/{}", item.id, task.id))
    .json(&serde_json::json!({ "after": null }))
    .send()
    .await
    .expect("answered");
  assert_eq!(cleared.status(), 200);
  assert_eq!(client.task(item.id, task.id).await.expect("read").after, None);

  let refusals = [
    (waits_on(i64::MAX, "web"), "unknown-item"),
    (waits_on(item.id, "web"), "conflict"),
    (waits_on(other.id, "  "), "bad-request"),
  ];
  for (after, code) in refusals {
    let added = client
      .add_step(
        item.id,
        &NewStep {
          after: Some(after.clone()),
          ..standing("another")
        },
      )
      .await
      .expect_err("refused");
    assert_eq!(added.code(), Some(code), "{after:?}");
    let moved = TaskChange {
      after: Some(Some(after.clone())),
      ..TaskChange::default()
    };
    let refused = client.change_task(item.id, task.id, &moved).await.expect_err("refused");
    assert_eq!(refused.code(), Some(code), "{after:?}");
  }
  assert_eq!(client.work_order(item.id).await.expect("read").steps, ["the parser", "wire it in"]);
  assert_eq!(client.task(item.id, task.id).await.expect("read").after, None);
}

#[tokio::test]
async fn a_task_keeps_what_it_waits_on_when_its_steps_are_saved_again() {
  let service = Service::start().await;
  let client = service.client();
  let other = service.open_item("build", "schema").await;
  let item = service.open_item("build", "parser").await;
  client.put_work_order(item.id, &body(&["design", "build"])).await.expect("made");
  let build = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items[1].clone();
  let moved = TaskChange {
    after: Some(Some(waits_on(other.id, "web"))),
    ..TaskChange::default()
  };
  client.change_task(item.id, build.id, &moved).await.expect("moved");

  client
    .put_work_order(item.id, &body(&["design", "build"]))
    .await
    .expect("the same steps");
  client
    .put_work_order(item.id, &body(&["review", "design", "build"]))
    .await
    .expect("re-made");
  let tasks = client.tasks(item.id, &TaskQuery::default()).await.expect("listed").items;
  assert_eq!(titles(&tasks), [(1, "review"), (2, "design"), (3, "build")]);
  let kept = &tasks[2];
  assert_eq!((kept.id, kept.after.clone()), (build.id, Some(waits_on(other.id, "web"))));
  assert!(tasks[..2].iter().all(|task| task.after.is_none()));
}
