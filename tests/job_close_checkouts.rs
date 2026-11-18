//! A close of several checkouts (docs/cli.md, 「Several checkouts in one close」): every checkout read before
//! anything is checked, one check over them all through the testbed's check over several checkouts, the pushes
//! ordered by base libraries, and each repository's pair written as its push lands.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use bmo_job::close_watch::Running;
use bmo_job::job_close::{
  Checkouts, JobCloseArgs, Standing, Steps, checkout_roots, close_checkouts, standing_labels,
  standings_labels, waiting_commits,
};
use bmo_job_client::{ItemChange, Labels, NewItem};

use crate::job_close::{
  Started, commit, executable, git, init, main_of, quick, record_at, remote_for, soon,
};

// A checkout of the repository `name`: its first commit on its remote's main, and recorded live where it deploys;
// its second the job's own, not yet pushed.
struct Repo {
  repo: tempfile::TempDir,
  remote: tempfile::TempDir,
  _deploy_root: tempfile::TempDir,
  base: String,
  head: String,
}

fn repo(name: &str, deploys: bool, base_libraries: &[&str]) -> Repo {
  let repo = tempfile::tempdir().expect("a repository");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", &format!("Add the first page of {name}"));
  let remote = remote_for(repo.path());
  let head = commit(repo.path(), "two.md", &format!("Add the second page of {name}"));
  let deploy = if deploys {
    record_at(deploy_root.path(), &base);
    format!("\n[deploy]\nroot = \"{}\"\nlink = \"deploy\"\n", deploy_root.path().display())
  } else {
    String::new()
  };
  let libraries: Vec<String> = base_libraries.iter().map(|library| format!("\"{library}\"")).collect();

  fs::write(
    repo.path().join("testbed.toml"),
    format!(
      "testbed = 0\nname = \"{name}\"\n{deploy}\n[check]\nbase_libraries = [{}]\n",
      libraries.join(", ")
    ),
  )
  .expect("the declaration");

  Repo {
    repo,
    remote,
    _deploy_root: deploy_root,
    base,
    head,
  }
}

// A testbed standing in for the live one's check over several checkouts, read as its usage reads it,
// `bmo-testbed check --checkout <PATH[@REV]>` given once for each checkout and never with `--since` or files: it
// writes the words it was asked with on a line, refuses any other words with 2 as its parser does, and a word
// naming no checkout or a since that is no ancestor of the checkout's HEAD with 2 as its check does, then runs
// `then`.
fn set_testbed_at(dir: &Path, then: &str) -> (PathBuf, PathBuf) {
  let calls = dir.join("testbed-calls");
  let path = dir.join("bmo-testbed");

  executable(
    &path,
    &format!(
      r#"#!/bin/sh
echo "$@" >> "{calls}"
[ "$1" = check ] || exit 2
shift
while [ $# -gt 0 ]; do
  case "$1" in
    --checkout)
      [ $# -ge 2 ] || {{ echo "error: a value is required for '--checkout <PATH[@REV]>'" >&2; exit 2; }}
      path="${{2%%@*}}"
      since="${{2#*@}}"
      git -C "$path" rev-parse --show-toplevel > /dev/null 2>&1 || {{ echo "check: $2 names no checkout" >&2; exit 2; }}
      if [ "$since" != "$2" ]; then
        git -C "$path" merge-base --is-ancestor "$since" HEAD || {{ echo "check: $2: no ancestor of HEAD" >&2; exit 2; }}
      fi
      shift 2;;
    *) echo "error: the argument '--checkout <PATH[@REV]>' cannot be used with '$1'" >&2; exit 2;;
  esac
done
{then}
"#,
      calls = calls.display()
    ),
  );

  (path, calls)
}

const PASSES: &str = "echo 'check: ok'";

// Every set of standings a close told, in order.
type Told = Vec<Vec<(String, Standing)>>;

// A close of the checkouts, run from `data` by the test's clocks, with every set of standings it told.
fn close_of(
  checkouts: &[&Repo],
  data: &Path,
  waiting: &BTreeMap<String, String>,
  testbed: &Path,
) -> (Result<Vec<String>, String>, Told) {
  let paths: Vec<PathBuf> = checkouts.iter().map(|checkout| checkout.repo.path().to_owned()).collect();
  let roots = checkout_roots(data, &paths).expect("every path names a checkout");
  let told = RefCell::new(Vec::new());
  let steps = Steps {
    testbed,
    clocks: quick(),
    running: Running::default(),
    say: &|_| {},
  };
  let closed = close_checkouts(
    &Checkouts {
      from: data,
      roots: &roots,
      waiting,
    },
    data,
    &JobCloseArgs {
      then_reason: "Land the pages".to_owned(),
      skip_suite: None,
    },
    &steps,
    &|standings| told.borrow_mut().push(standings.to_vec()),
  );

  (closed, told.into_inner())
}

// The commits an item waits on once every standing told is written on it, as the close's item writes them.
fn waiting_after(told: &Told) -> BTreeMap<String, String> {
  let labels = told
    .iter()
    .fold(Labels::default(), |labels, standings| standings_labels(&labels, standings));

  waiting_commits(&labels)
}

fn top(checkout: &Repo) -> String {
  git(checkout.repo.path(), &["rev-parse", "--show-toplevel"])
}

fn standing(repository: &str, standing: Standing) -> (String, Standing) {
  (repository.to_owned(), standing)
}

// The base library is given last and pushed first; the consumer waits for its batch, the base library deploys
// nothing and ends on main.
#[test]
fn every_checkout_passing_is_pushed_base_library_first_and_each_pair_written() {
  let data = tempfile::tempdir().expect("a data root");
  let consumer = repo("the-consumer", true, &["the-base"]);
  let library = repo("the-base", false, &[]);
  let (testbed, calls) = set_testbed_at(data.path(), PASSES);
  let (closed, told) = close_of(&[&consumer, &library], data.path(), &BTreeMap::new(), &testbed);
  let lines = closed.expect("closed");
  let (consumer_head, library_head) = (&consumer.head[..12], &library.head[..12]);

  assert_eq!(
    fs::read_to_string(calls).expect("the stand-in's calls"),
    format!(
      "check --checkout {}@{} --checkout {}@{}\n",
      top(&consumer),
      consumer.base,
      top(&library),
      library.base
    ),
    "one check over both, each since its own main"
  );
  assert_eq!(
    told,
    vec![
      vec![standing("the-consumer", Standing::Testing), standing("the-base", Standing::Testing)],
      vec![standing("the-base", Standing::Off)],
      vec![standing("the-consumer", Standing::WaitingBatch(consumer.head.clone()))],
    ],
    "the base library is pushed first"
  );
  assert_eq!(
    waiting_after(&told),
    BTreeMap::from([("the-consumer".to_owned(), consumer.head.clone())])
  );
  assert_eq!(main_of(consumer.remote.path()), consumer.head);
  assert_eq!(main_of(library.remote.path()), library.head);
  assert_eq!(
    lines,
    [
      "job-close: Land the pages".to_owned(),
      format!("the-consumer: merged: {consumer_head} Add the second page of the-consumer onto origin/main"),
      format!(
        "the-consumer: checked: the static check passed over what changed since {}",
        &consumer.base[..12]
      ),
      format!("the-consumer: waiting for the batch deployment: {consumer_head} stands on origin/main"),
      format!("the-base: merged: {library_head} Add the second page of the-base onto origin/main"),
      format!(
        "the-base: checked: the static check passed over what changed since {}",
        &library.base[..12]
      ),
      format!(
        "the-base: on main: {library_head} stands on origin/main, and this repository deploys nothing"
      ),
    ]
  );
}

#[test]
fn a_check_that_fails_pushes_nothing_takes_the_pairs_off_and_names_the_failed_repository() {
  let data = tempfile::tempdir().expect("a data root");
  let consumer = repo("the-consumer", true, &["the-base"]);
  let library = repo("the-base", false, &[]);
  let (testbed, _calls) = set_testbed_at(
    data.path(),
    "echo 'the-consumer: cargo test: failed' >&2\necho 'check FAILED: the-consumer: cargo test' >&2\necho 'log: /nowhere' >&2\nexit 1",
  );
  let (closed, told) = close_of(&[&consumer, &library], data.path(), &BTreeMap::new(), &testbed);

  assert_eq!(
    closed.expect_err("the check fails"),
    "the combined check did not pass: check FAILED: the-consumer: cargo test"
  );
  assert_eq!(
    told,
    vec![
      vec![standing("the-consumer", Standing::Testing), standing("the-base", Standing::Testing)],
      vec![standing("the-consumer", Standing::Off), standing("the-base", Standing::Off)],
    ]
  );
  assert_eq!(main_of(consumer.remote.path()), consumer.base);
  assert_eq!(main_of(library.remote.path()), library.base);
}

// The first push lands and its pair stays; the second run reads that pair, leaves the first alone and checks and
// pushes the second alone.
#[test]
fn a_refused_second_push_leaves_the_first_on_main_and_a_second_run_closes_the_rest() {
  let data = tempfile::tempdir().expect("a data root");
  let web = repo("the-web", true, &[]);
  let api = repo("the-api", true, &[]);
  let hook = api.remote.path().join("hooks").join("pre-receive");
  let (testbed, calls) = set_testbed_at(data.path(), PASSES);

  executable(&hook, "#!/bin/sh\nexit 1\n");

  let (closed, told) = close_of(&[&web, &api], data.path(), &BTreeMap::new(), &testbed);
  let refused = closed.expect_err("the second push is refused");

  assert!(refused.starts_with("the-api: the push onto origin/main was refused: "), "{refused}");
  assert!(
    refused.contains(
      "; the-web stay on their main with their pairs; run the close again with the same --checkout list"
    ),
    "{refused}"
  );
  assert_eq!(main_of(web.remote.path()), web.head);
  assert_eq!(main_of(api.remote.path()), api.base);
  assert_eq!(
    told,
    vec![
      vec![standing("the-web", Standing::Testing), standing("the-api", Standing::Testing)],
      vec![standing("the-web", Standing::WaitingBatch(web.head.clone()))],
      vec![standing("the-api", Standing::Off)],
    ]
  );

  let waiting = waiting_after(&told);

  assert_eq!(waiting, BTreeMap::from([("the-web".to_owned(), web.head.clone())]));
  fs::remove_file(&hook).expect("the hook taken away");

  let (closed, told) = close_of(&[&web, &api], data.path(), &waiting, &testbed);
  let lines = closed.expect("the rest closed");

  assert!(
    lines.contains(&format!("the-web: already waits on origin/main: {}", &web.head[..12])),
    "{lines:?}"
  );
  assert!(
    lines.contains(&format!(
      "the-api: waiting for the batch deployment: {} stands on origin/main",
      &api.head[..12]
    )),
    "{lines:?}"
  );
  assert_eq!(
    fs::read_to_string(calls).expect("the stand-in's calls").lines().nth(1),
    Some(format!("check --checkout {}@{}", top(&api), api.base).as_str()),
    "the second run checks the rest alone"
  );
  assert_eq!(
    told,
    vec![
      vec![standing("the-api", Standing::Testing)],
      vec![standing("the-api", Standing::WaitingBatch(api.head.clone()))],
    ]
  );
  assert_eq!(main_of(api.remote.path()), api.head);
  assert_eq!(main_of(web.remote.path()), web.head);
}

#[test]
fn a_hangul_message_in_one_checkout_refuses_the_close_before_anything_is_checked() {
  let data = tempfile::tempdir().expect("a data root");
  let web = repo("the-web", true, &[]);
  let api = repo("the-api", true, &[]);
  let worded =
    commit(api.repo.path(), "three.md", &format!("Add the third page\n\nAs asked: {}", '\u{AC00}'));
  let (testbed, calls) = set_testbed_at(data.path(), PASSES);
  let (closed, told) = close_of(&[&web, &api], data.path(), &BTreeMap::new(), &testbed);
  let refused = closed.expect_err("a message with Hangul");

  assert!(
    refused.starts_with(&format!(
      "the-api: these commits hold Hangul, and what goes into git is English only: {} Add the third page",
      &worded[..12]
    )),
    "{refused}"
  );
  assert!(refused.contains("git rebase -i origin/main"), "{refused}");
  assert!(!calls.exists(), "a refused close checks nothing");
  assert!(told.is_empty(), "a refused close writes nothing on the item");
  assert_eq!(main_of(web.remote.path()), web.base);
  assert_eq!(main_of(api.remote.path()), api.base);
}

// The binary end to end over two checkouts and an item: the item's pair for one already waiting on main leaves
// that one out; a stop while the check runs ends it with everything under it and takes off the pair it wrote
// `testing` alone; run again, the close checks and pushes the rest and ends at the merge.
#[tokio::test]
async fn the_binary_closes_the_checkouts_the_item_does_not_name_as_waiting_and_a_stop_takes_off_what_it_tested()
 {
  let service = crate::support::Service::start().await;
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: "closed across checkouts".to_owned(),
      brief: "close".to_owned(),
      queue_name: "todo".to_owned(),
      ..NewItem::default()
    })
    .await
    .expect("the item")
    .id;
  let data = tempfile::tempdir().expect("a data root");
  let tools = tempfile::tempdir().expect("a directory for the testbed stand-in");
  let web = repo("the-web", true, &[]);
  let api = repo("the-api", true, &[]);

  git(web.repo.path(), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
  client
    .change_item(
      item,
      &ItemChange {
        labels: Some(standing_labels(
          &Labels::default(),
          "the-web",
          &Standing::WaitingBatch(web.head.clone()),
        )),
        ..ItemChange::default()
      },
    )
    .await
    .expect("the earlier close's pair");

  let hold = tools.path().join("hold");
  let pids = tools.path().join("stand-in-pids");
  let started = Started(pids.clone());
  let (_, calls) = set_testbed_at(
    tools.path(),
    &format!(
      "echo $$ >> \"{}\"\nwhile [ -e \"{}\" ]; do sleep 0.05; done\necho 'check: ok'",
      pids.display(),
      hold.display()
    ),
  );
  let path = format!("{}:{}", tools.path().display(), std::env::var("PATH").unwrap_or_default());
  let close = || {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"))
      .args(["job", "close", "--then", "Land the pages", "--todo", &item.to_string()])
      .arg("--checkout")
      .arg(web.repo.path())
      .arg("--checkout")
      .arg(api.repo.path())
      .current_dir(data.path())
      .env("BMO_JOB_URL", &service.url)
      .env("BMO_JOB_TOKEN", crate::support::TOKEN)
      .env("BMO_DATA_DIR", data.path())
      .env("PATH", &path)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true)
      .spawn()
      .expect("the close runs")
  };

  fs::write(&hold, "").expect("the check held");

  let stopped = close();
  let mut looked = 0;

  while started.pids().is_empty() && looked < 3_000 {
    tokio::time::sleep(Duration::from_millis(10)).await;
    looked += 1;
  }
  assert!(!started.pids().is_empty(), "the check started");

  let tested = client.item(item).await.expect("the item").labels;

  assert_eq!(tested.get("close-phase:the-api").map(String::as_str), Some("testing"), "{tested:?}");

  // SAFETY: a TERM to the close this test started.
  unsafe { libc::kill(stopped.id().expect("the close's process") as i32, libc::SIGTERM) };

  let output = tokio::time::timeout(Duration::from_secs(60), stopped.wait_with_output())
    .await
    .expect("the stopped close ends")
    .expect("its output");

  assert_eq!(output.status.code(), Some(130), "{}", String::from_utf8_lossy(&output.stderr));
  assert!(
    String::from_utf8_lossy(&output.stdout)
      .trim_end()
      .ends_with("job-close: ended, exit 130")
  );
  assert!(soon(|| started.all_gone()), "nothing the check started is left running");

  let after_stop = client.item(item).await.expect("the item").labels;

  assert!(!after_stop.contains_key("close-phase:the-api"), "{after_stop:?}");
  assert_eq!(after_stop.get("close-commit:the-web"), Some(&web.head), "{after_stop:?}");
  assert_eq!(main_of(api.remote.path()), api.base);

  fs::remove_file(&hold).expect("the check let go");

  let output = tokio::time::timeout(Duration::from_secs(60), close().wait_with_output())
    .await
    .expect("the close ends")
    .expect("its output");
  let (said, out) = (String::from_utf8_lossy(&output.stderr), String::from_utf8_lossy(&output.stdout));

  assert_eq!(output.status.code(), Some(0), "{said}");
  assert!(
    out.contains(&format!("the-web: already waits on origin/main: {}\n", &web.head[..12])),
    "{out}"
  );
  assert!(
    out.trim_end().ends_with(&format!(
      "the-api: waiting for the batch deployment: {} stands on origin/main\njob-close: ended, exit 0",
      &api.head[..12]
    )),
    "{out}"
  );
  assert_eq!(
    fs::read_to_string(&calls).expect("the stand-in's calls"),
    format!("check --checkout {0}@{1}\ncheck --checkout {0}@{1}\n", top(&api), api.base),
    "the checkout already waiting is never checked"
  );
  assert_eq!(main_of(api.remote.path()), api.head);
  assert_eq!(
    waiting_commits(&client.item(item).await.expect("the item").labels),
    BTreeMap::from([("the-api".to_owned(), api.head.clone()), ("the-web".to_owned(), web.head.clone())])
  );
}
