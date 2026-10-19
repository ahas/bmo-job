//! A question a job's program stops on in the middle of its turn, judged by the runner (docs/runs.md, "A
//! command the runner allows itself"): a command inside the job's own places allowed at once with no question put,
//! any other put to the owner with its recommendation and settled by the owner, by its minute running out, or by
//! the pane no longer asking it.

use bmo_job::job_run::claude::confirm::{ANSWERED_IN_PANE, Asker, OWN_WORK_NOTE, owner_refusal};
use bmo_job::queue::outcomes::JOB_DONE;
use bmo_job::queue::questions::{self, Answer};
use job_agency::{ConfirmAnswer, Confirmation, Question};

use crate::support::job::{Bench, OWNER_NAME, Ran, UNIT, WORKER};
use crate::support::turns::{FINISHED, Played, closing, reply};

/// The item the questions are put on.
const TODO: i64 = 7;

/// A permission to run `command`, as the screen shows the question.
fn permission(command: &str) -> Confirmation {
  let rule = "╌".repeat(60);

  Confirmation::Permission {
    text: [
      "Bash command",
      " A command",
      &rule,
      &format!(" │ {command}"),
      &rule,
      "",
      " Do you want to proceed?",
    ]
    .join("\n"),
    amendable: true,
  }
}

fn asked(kind: Confirmation) -> Question {
  Question {
    id: "q-1".into(),
    kind,
  }
}

// A job of the item on a worker in a checkout of its own, its turn read first on each of `questions` in turn.
async fn asking(questions: Vec<Option<Question>>) -> (Bench, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(TODO, "Fix the gate").await;

  let job = bench.job(Some(TODO), WORKER, UNIT).await;

  let played = Played::answered(reply("s0", FINISHED))
    .asking(questions)
    .doing(closing(&bench.scene.db, TODO));

  bench.agent.play([played]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran)
}

async fn put(bench: &Bench) -> Vec<questions::Question> {
  let rows = job_base::all(&bench.scene.db, r#"SELECT "id" FROM "job_questions" ORDER BY "id""#, vec![])
    .await
    .unwrap();
  let mut put = vec![];

  for row in rows {
    let id: i64 = row.try_get("", "id").unwrap();

    put.extend(questions::question(&bench.scene.db, id).await.unwrap());
  }

  put
}

#[tokio::test]
async fn a_command_inside_the_job_s_own_places_is_allowed_at_once_with_nothing_put_to_anybody() {
  let (bench, ran) = asking(vec![Some(asked(permission("cat kept.ts")))]).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(bench.agent.answers(), [("q-1".to_owned(), ConfirmAnswer::Allow)]);
  assert!(
    bench.agent.notes().iter().any(|note| note == OWN_WORK_NOTE),
    "{:?}",
    bench.agent.notes()
  );
  assert!(put(&bench).await.is_empty());
}

#[tokio::test]
async fn a_command_nothing_restores_after_is_put_to_the_owner_recommended_refused_and_withdrawn_once_the_pane_stops_asking()
 {
  let (bench, ran) = asking(vec![Some(asked(permission("rm -rf /"))), None]).await;
  let put = put(&bench).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(put.len(), 1);
  assert_eq!((put[0].todo_id, put[0].recommended), (TODO, 1));
  assert_eq!(put[0].answer.as_deref(), Some(ANSWERED_IN_PANE));
  // The pane answered it, so nothing is sent to it.
  assert!(bench.agent.answers().is_empty());
}

// The item and the job a question is put for, and the question put to the owner.
async fn put_to_the_owner(
  bench: &Bench,
  kind: Confirmation,
) -> (Asker, bmo_job::job_run::claude::confirm::Asking) {
  bench.scene.item(TODO, "Fix the gate").await;

  let asker = Asker {
    todo_id: Some(TODO),
    job_id: bench.job(Some(TODO), WORKER, UNIT).await,
  };
  let asking = asker.ask(&bench.ground(), "q-1", &kind).await.unwrap().expect("the question is put");

  (asker, asking)
}

// The question's minute run out, as if it was asked long ago.
async fn lapsed(bench: &Bench, row: i64) {
  bench
    .scene
    .run(r#"UPDATE "job_questions" SET "due_at" = 0 WHERE "id" = $1"#, vec![row.into()])
    .await;
}

#[tokio::test]
async fn an_unanswered_question_settles_on_its_recommendation_once_its_minute_runs_out() {
  let bench = Bench::new().await;
  let (asker, asking) = put_to_the_owner(&bench, permission("rm -rf /")).await;

  assert_eq!(asker.settled(&bench.ground(), &asking).await.unwrap(), None);
  lapsed(&bench, asking.row).await;
  assert_eq!(
    asker.settled(&bench.ground(), &asking).await.unwrap(),
    Some(ConfirmAnswer::Refuse { words: None })
  );
}

#[tokio::test]
async fn an_unknown_question_left_unanswered_is_closed() {
  let bench = Bench::new().await;
  let unknown = Confirmation::Unknown {
    screen: "Pick one".into(),
  };
  let (asker, asking) = put_to_the_owner(&bench, unknown).await;

  lapsed(&bench, asking.row).await;
  assert_eq!(
    asker.settled(&bench.ground(), &asking).await.unwrap(),
    Some(ConfirmAnswer::Refuse { words: None })
  );
}

#[tokio::test]
async fn the_owner_s_own_words_refuse_it_with_the_paragraph_naming_the_owner() {
  let bench = Bench::new().await;
  let kind = permission("rm -rf build");
  let (asker, asking) = put_to_the_owner(&bench, kind.clone()).await;
  let said = Answer::Words("Not that one.".into());

  assert!(questions::settle(&bench.scene.db, asking.row, &said, bmo_job::now_ms()).await.unwrap());
  assert_eq!(
    asker.settled(&bench.ground(), &asking).await.unwrap(),
    Some(ConfirmAnswer::Refuse {
      words: Some(owner_refusal(&kind, OWNER_NAME, "Not that one."))
    })
  );
}
