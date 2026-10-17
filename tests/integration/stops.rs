//! A run stopped from outside (docs/queue.md, "Stopping runs"): its execution asked cancelled through
//! bmo-agency and its unit stopped by systemd only where it still stands after that, and the runs a work
//! order's change ended confirmed gone before the rows they left are closed — systemctl standing in, each
//! test setting what it answers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_job::queue::stops::Stops;
use bmo_job::queue::work_orders::StoppedRun;

use crate::support::program::{linked, set_shell};
use crate::support::queue::{NOW, Scene};

// systemctl standing in a folder of its own: `is-active` answering `state`, `stop` ending with `stop_exit`,
// and every call's words written to `lines`.
fn systemctl(dir: &Path, state: &str, stop_exit: u8) {
  let lines = dir.join("lines");

  set_shell(
    dir,
    &format!(
      "STAND_IN_LINES='{}'\ncase \"$2\" in is-active) STAND_IN_OUT={state};; stop) STAND_IN_EXIT={stop_exit};; esac\n",
      lines.display()
    ),
  );
}

fn said(dir: &Path) -> String {
  std::fs::read_to_string(dir.join("lines")).unwrap_or_default()
}

fn stops<'a>(scene: &'a Scene, link: &Path) -> Stops<'a, sea_orm::DatabaseConnection> {
  Stops {
    db: &scene.db,
    units: &scene.units,
    systemctl: Some(PathBuf::from(link)),
    settle: Duration::ZERO,
  }
}

async fn count(scene: &Scene, sql: &str) -> i64 {
  bmo_job::one(&scene.db, sql, vec![])
    .await
    .unwrap()
    .unwrap()
    .try_get_by_index::<i64>(0)
    .unwrap()
}

#[tokio::test]
async fn a_run_is_asked_cancelled_and_stopped_by_systemd_only_where_it_still_stands() {
  let scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");
  let stops = stops(&scene, &link);

  // Gone once asked: systemd is asked whether it stands and nothing more.
  systemctl(dir.path(), "inactive", 0);
  assert_eq!(stops.stop("bmo-task-1").await.unwrap(), None);
  assert_eq!(scene.units.cancelled(), ["bmo-task-1"]);
  assert_eq!(said(dir.path()), "--user is-active bmo-task-1.service\n");

  // Still standing: systemd stops it, and a stop that failed is said.
  std::fs::remove_file(dir.path().join("lines")).unwrap();
  systemctl(dir.path(), "active", 5);
  assert_eq!(
    stops.stop("bmo-task-2").await.unwrap().as_deref(),
    Some("systemctl stopped bmo-task-2 with exit code 5")
  );
  assert_eq!(
    said(dir.path()),
    "--user is-active bmo-task-2.service\n--user stop bmo-task-2.service\n"
  );

  systemctl(dir.path(), "active", 0);
  assert_eq!(stops.stop("bmo-task-3").await.unwrap(), None);
  assert_eq!(scene.units.cancelled(), ["bmo-task-1", "bmo-task-2", "bmo-task-3"]);
}

#[tokio::test]
async fn the_runs_an_order_s_change_ended_are_confirmed_gone_before_their_rows_close() {
  let scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");
  let stops = stops(&scene, &link);

  scene.item(3, "Skipped").await;

  let job = scene.job(Some(3), "worker", "bmo-task-a", 1).await;
  let runs = [StoppedRun {
    unit: Some("bmo-task-a".into()),
    job_id: Some(job),
    pool_request: Some("token-a".into()),
  }];
  let open = format!(r#"SELECT COUNT(*) FROM "jobs" WHERE "id" = {job} AND "finished_at" IS NULL"#);

  // A unit systemd still holds after its stop refuses the whole stop, writing nothing.
  systemctl(dir.path(), "active", 0);

  let refused = stops.stop_runs(&runs, "task skipped", "died", NOW).await.unwrap_err();

  assert_eq!(refused.code, "runs_still_alive");
  assert_eq!(count(&scene, &open).await, 1);
  assert!(scene.units.finished().is_empty());
  assert!(scene.units.dropped().is_empty());

  // Once gone, the row closes with the ending named, the place in the pool is given back and the cancel
  // taken back.
  systemctl(dir.path(), "inactive", 0);
  stops.stop_runs(&runs, "task skipped", "died", NOW).await.unwrap();

  assert_eq!(
    count(
      &scene,
      &format!(
        r#"SELECT COUNT(*) FROM "jobs" WHERE "id" = {job} AND "finished_at" = {NOW}
             AND "report" = 'task skipped' AND "outcome" = 'died' AND "ok" = 0"#
      )
    )
    .await,
    1
  );
  assert_eq!(scene.units.finished(), [("token-a".to_owned(), "task skipped".to_owned(), true)]);
  assert_eq!(scene.units.dropped(), ["bmo-task-a"]);
}
