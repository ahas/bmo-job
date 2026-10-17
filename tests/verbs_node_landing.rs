//! A landing from a checkout on a work node (docs/verbs.md, "Where a verb stands" and "Landing a branch"):
//! the settings' `[node]` section tells the verb it runs on a node, the worker is the one whose row names the
//! node and the checkout, and the landing is held on the operations machine through `node-land --worker`,
//! which a shell stands in for here: it records the words it was started with and the node's word, and says
//! `held` and then a line of its own.

mod support;

use std::path::Path;
use std::process::Command;

use bmo_job::verbs::land::land;
use bmo_job::verbs::stand::{Stand, VerbError};
use job_base::{Settings, run};
use migration::{Migrator, MigratorTrait};
use serde_json::json;
use support::server::{DONE, host_text, settings_text};
use support::{agency, gate, receiver};

fn git(cwd: &Path, args: &[&str]) -> String {
  let ran = Command::new("git").current_dir(cwd).args(args).output().expect("git runs");

  assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
  String::from_utf8_lossy(&ran.stdout).trim().to_owned()
}

/// A repository on a remote of its own, cloned into `checkout` and standing on a job's branch with one commit
/// past main.
fn repository(scratch: &Path) -> std::path::PathBuf {
  let origin = scratch.join("origin.git");
  let checkout = scratch.join("bench");

  git(scratch, &["init", "-q", "--bare", "-b", "main", &origin.display().to_string()]);
  git(
    scratch,
    &["clone", "-q", &origin.display().to_string(), &checkout.display().to_string()],
  );

  for (key, value) in [("user.name", "Tester"), ("user.email", "tester@example.invalid")] {
    git(&checkout, &["config", key, value]);
  }

  git(&checkout, &["checkout", "-q", "-b", "main"]);
  std::fs::write(checkout.join("README.md"), "A repository.\n").unwrap();
  git(&checkout, &["add", "README.md"]);
  git(&checkout, &["commit", "-q", "-m", "Start the repository"]);
  git(&checkout, &["push", "-q", "origin", "main"]);
  git(&checkout, &["remote", "set-head", "origin", "main"]);
  git(&checkout, &["checkout", "-q", "-b", "work/7"]);
  std::fs::write(checkout.join("notes.md"), "What the job wrote.\n").unwrap();
  git(&checkout, &["add", "notes.md"]);
  git(&checkout, &["commit", "-q", "-m", "Write the job's notes"]);
  checkout
}

/// The stand of a verb run on the node `n1`, the operations machine reached through a shell that stands in
/// for `node-land`, its words and the node's word written under `heard`.
async fn node_stand(
  scratch: &Path,
  heard: &Path,
  agency: &agency::Stub,
  gate: &gate::Stub,
  receiver: &receiver::Stub,
) -> Stand {
  let url = format!("sqlite://{}?mode=rwc", scratch.join("job.db").display());
  let script = format!(
    r#"printf '%s\n' "$@" > "{heard}/args"; echo held; read word; printf '%s\n' "$word" > "{heard}/word"; echo "home took $word""#,
    heard = heard.display()
  );
  let node = format!(
    "\n[node]\nname = \"n1\"\nhome = [\"sh\", \"-c\", {script}, \"home\"]\n",
    // A JSON string is a TOML basic string as it stands.
    script = serde_json::to_string(&script).unwrap()
  );
  let text = settings_text(&url, &agency.address(), &gate.address(), &receiver.address())
    + &host_text(scratch, Path::new(DONE))
    + &node;
  let settings = Settings::read(&text).expect("the node's settings read");
  let db = settings.database.url.connect(1).await.unwrap();

  Migrator::up(&db, None).await.unwrap();
  receiver.answer("GET /owner", 200, json!({ "sender": 5, "chat": 100 }));

  Stand::of(&settings).await.expect("the stand is built")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_landing_on_a_node_is_held_on_the_operations_machine_for_the_worker_this_checkout_is() {
  let scratch = tempfile::tempdir().unwrap();
  let heard = scratch.path().join("heard");
  let (agency, gate, receiver) =
    (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);

  std::fs::create_dir_all(&heard).unwrap();

  let checkout = repository(scratch.path());
  let stand = node_stand(scratch.path(), &heard, &agency, &gate, &receiver).await;

  run(
    &stand.db,
    r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "support_of", "node", "created_at")
       VALUES ('bench-n1', 'bench@n1', $1, 0, 0, 'bench', 'n1', 1)"#,
    vec![checkout.display().to_string().into()],
  )
  .await
  .unwrap();

  let answer = match land(&stand, &checkout).await {
    Ok(answer) => answer,
    Err(VerbError::Ended { said, .. }) => panic!("the landing was refused: {said}"),
    Err(_) => panic!("the landing failed"),
  };
  let head = git(&checkout, &["rev-parse", "HEAD"]);

  assert_eq!(
    git(scratch.path().join("origin.git").as_path(), &["rev-parse", "main"]),
    head,
    "main was pushed"
  );
  assert_eq!(
    std::fs::read_to_string(heard.join("args")).unwrap(),
    "node-land\n--worker\nbench-n1\n",
    "the operations machine was asked to hold the landing for this checkout's worker"
  );
  assert_eq!(std::fs::read_to_string(heard.join("word")).unwrap(), format!("landed {head}\n"));
  assert!(answer.lines[0].contains(&head[..12]), "{:?}", answer.lines);
  assert_eq!(answer.lines.last(), Some(&format!("home took landed {head}")), "{:?}", answer.lines);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checkout_on_a_node_no_worker_s_row_names_is_refused_and_lands_nothing() {
  let scratch = tempfile::tempdir().unwrap();
  let heard = scratch.path().join("heard");
  let (agency, gate, receiver) =
    (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);

  std::fs::create_dir_all(&heard).unwrap();

  let checkout = repository(scratch.path());
  let stand = node_stand(scratch.path(), &heard, &agency, &gate, &receiver).await;
  let before = git(scratch.path().join("origin.git").as_path(), &["rev-parse", "main"]);

  match land(&stand, &checkout).await {
    Err(VerbError::Ended { code: 1, said }) => {
      let locked = stand.policy.fill(&stand.policy.landing.refusal.locked, &[("main", "main")]);

      assert!(said.contains(&locked), "{said}");
    }
    Ok(answer) => panic!("a checkout no worker names landed: {:?}", answer.lines),
    Err(_) => panic!("the landing failed otherwise"),
  }

  assert_eq!(git(scratch.path().join("origin.git").as_path(), &["rev-parse", "main"]), before);
  assert!(!heard.join("args").exists(), "the operations machine was never asked");
}
