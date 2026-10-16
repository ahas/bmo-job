//! A work order's lock (docs/rows.md, "A work order's lock"): its takers in one process go one at a time, the
//! row is gone once the work is done, and a holder whose process is gone is cleared.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use job_base::Refusal;
use queue_rows::order_lock::with_order_lock;
use support::Scene;

#[tokio::test]
async fn takers_hold_the_lock_one_at_a_time_and_leave_no_row_behind() {
  let scene = Scene::new().await;
  let inside = Arc::new(AtomicUsize::new(0));
  let most = Arc::new(AtomicUsize::new(0));
  let work = |inside: Arc<AtomicUsize>, most: Arc<AtomicUsize>| async move {
    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;

    most.fetch_max(now, Ordering::SeqCst);
    tokio::task::yield_now().await;
    inside.fetch_sub(1, Ordering::SeqCst);

    Ok::<_, Refusal>(())
  };
  let takers = (0..4).map(|_| with_order_lock(&scene.db, 3, work(inside.clone(), most.clone())));

  for taken in futures_util::future::join_all(takers).await {
    taken.unwrap();
  }

  assert_eq!(most.load(Ordering::SeqCst), 1);
  assert!(
    job_base::one(&scene.db, r#"SELECT 1 AS "one" FROM "work_order_locks""#, vec![])
      .await
      .unwrap()
      .is_none()
  );
}

#[tokio::test]
async fn a_holder_whose_process_is_gone_is_cleared_and_the_work_answers_its_own_refusal() {
  let scene = Scene::new().await;
  let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
    .unwrap_or_default()
    .trim()
    .to_owned();

  scene
    .run(
      r#"INSERT INTO "work_order_locks" ("todo_id", "host", "pid", "start", "token", "taken_at") VALUES (3, $1, $2, 'gone', 'stale', 0)"#,
      vec![host.into(), i64::from(std::process::id()).into()],
    )
    .await;

  let answer = with_order_lock(&scene.db, 3, async { Ok::<_, Refusal>(7) }).await.unwrap();

  assert_eq!(answer, 7);

  let refused: Result<(), Refusal> =
    with_order_lock(&scene.db, 3, async { Err(sea_orm::DbErr::Custom("work failed".into()).into()) }).await;

  assert_eq!(refused.unwrap_err().code, "database");
}
