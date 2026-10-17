//! What a job's run asks of other units beyond what the queue asks (docs/api.md, "Calls on other units"):
//! its work turns and the upkeep of the worker's sessions on bmo-agency, the sanitiser, kept translations
//! and knowledge on bmo-gate, what a checkout's graph names on bmo-graph, and the unit links into a checkout
//! on bmo-testbed; and the screens of the service's own server told the queue moved. Each is a call made
//! through the ports, a failed one refused 502 under the unit's code.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use futures_util::future::BoxFuture;
use job_agency::{
  Agency, AsideOutcome, ConfirmAnswer, Handover, JobDocuments, LogNote, Prepared, Reply, SessionAnswered,
  SessionKept, SshReach, TurnAsking, WorkTurn,
};
use job_codes::{JobError, UnitRefusal};
use job_gate::{Gate, KnowledgeAsked, Piece, Sanitise, Sanitised};
use job_graph::{Graph, Named};
use job_testbed::{Relinked, Testbed, UnitPlan};
use serde_json::{Value, json};
use url::Url;

use crate::Refusal;
use crate::queue::units::{agency_failed, gate_failed, testbed_failed};

// How long the server is given to take a word that the queue moved.
const MOVED_WAIT: Duration = Duration::from_secs(5);

/// A failed call on bmo-graph, refused under its code.
pub fn graph_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Graph {
    failure: why.to_string(),
  })
  .into()
}

/// Every call a job's run makes beyond the queue's.
pub trait Calls: Sync {
  /// A work turn prepared.
  fn prepare_turn<'a>(&'a self, asking: &'a TurnAsking) -> BoxFuture<'a, Result<Prepared, Refusal>>;
  /// A prepared work turn sent its prompt.
  fn send_turn_prompt<'a>(&'a self, turn: &'a str, prompt: &'a str) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A work turn as it stands, none where bmo-agency has no such turn.
  fn work_turn<'a>(&'a self, turn: &'a str) -> BoxFuture<'a, Result<Option<WorkTurn>, Refusal>>;
  /// Words pushed into a running work turn.
  fn push_aside<'a>(&'a self, turn: &'a str, text: &'a str)
  -> BoxFuture<'a, Result<AsideOutcome, Refusal>>;
  /// The question a work turn stands on answered.
  fn answer_question<'a>(
    &'a self,
    turn: &'a str,
    question: &'a str,
    answer: &'a ConfirmAnswer,
  ) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A reset waiting for a worker's sessions carried out; whether one waited.
  fn carry_out_reset<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<bool, Refusal>>;
  /// The session a job's last answer came back on recorded.
  fn record_session<'a>(
    &'a self,
    worker: &'a str,
    answered: &'a SessionAnswered,
    cwd: &'a str,
    job: i64,
  ) -> BoxFuture<'a, Result<SessionKept, Refusal>>;
  /// A worker's session handed over.
  fn hand_over_session<'a>(
    &'a self,
    worker: &'a str,
    handover: &'a Handover,
  ) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A run of a worker's noted.
  fn note_run<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>>;
  /// What a job's answer spent recorded.
  fn record_usage<'a>(
    &'a self,
    worker: &'a str,
    reply: &'a Reply,
    run: Option<&'a str>,
  ) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A worker's keepalive woken for a job.
  fn wake_keepalive<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A worker's bench made ready for a job.
  fn make_bench<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>>;
  /// The servers a worker's jobs left listening swept; what was swept.
  fn sweep_servers<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>>;
  /// What a worker's agent definition gives a job.
  fn job_documents<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<JobDocuments, Refusal>>;
  /// Which of the files touch the prefix a worker's session is opened on.
  fn prefix_touched<'a>(
    &'a self,
    worker: &'a str,
    files: &'a [String],
  ) -> BoxFuture<'a, Result<Vec<String>, Refusal>>;
  /// Whether a registered model's engine answers its health and stands aside for nothing.
  fn model_health<'a>(&'a self, provider: &'a str, model: &'a str) -> BoxFuture<'a, Result<bool, Refusal>>;
  /// A Claude login rested until the moment given.
  fn rest_login<'a>(&'a self, account: &'a str, until: i64) -> BoxFuture<'a, Result<(), Refusal>>;
  /// The rest a done job's session usage calls for begun.
  fn usage_rest<'a>(&'a self, account: &'a str, usage: &'a Value) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A note written into a run's log.
  fn write_note<'a>(&'a self, note: &'a LogNote) -> BoxFuture<'a, Result<(), Refusal>>;
  /// Where the copy of a node's checkout is kept on this machine.
  fn node_copy<'a>(&'a self, node: &'a str, checkout: &'a str) -> BoxFuture<'a, Result<String, Refusal>>;
  /// How SSH reaches a node, none where it names none.
  fn ssh_reach<'a>(&'a self, node: &'a str) -> BoxFuture<'a, Result<Option<SshReach>, Refusal>>;
  /// Words through the sanitiser.
  fn sanitise<'a>(&'a self, asked: &'a Sanitise) -> BoxFuture<'a, Result<Sanitised, Refusal>>;
  /// The translations kept in a language of each original that has one.
  fn kept<'a>(
    &'a self,
    language: &'a str,
    originals: &'a [String],
  ) -> BoxFuture<'a, Result<HashMap<String, String>, Refusal>>;
  /// The knowledge picked for a piece of work.
  fn knowledge<'a>(&'a self, asked: &'a KnowledgeAsked) -> BoxFuture<'a, Result<Vec<Piece>, Refusal>>;
  /// What a checkout's code graph names.
  fn graph_named<'a>(&'a self, checkout: &'a str) -> BoxFuture<'a, Result<Named, Refusal>>;
  /// What moving a checkout to a commit asks of the unit links into it.
  fn plan_units<'a>(
    &'a self,
    checkout: &'a Path,
    commit: &'a str,
    hold: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>>;
  /// Every unit link the checkout's present commit leaves dangling relinked.
  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>>;
  /// The screens told the queue moved; a server that does not take it is written to the log, never waited on.
  fn queue_moved(&self) -> BoxFuture<'_, ()>;
}

/// The calls made over the real ports, and the service's own server reached at its address where one is
/// given.
pub struct RunPorts<'a, A, G, Gr, T> {
  pub agency: &'a A,
  pub gate: &'a G,
  pub graph: &'a Gr,
  pub testbed: &'a T,
  pub server: Option<Url>,
}

impl<A: Agency, G: Gate, Gr: Graph + Sync, T: Testbed> Calls for RunPorts<'_, A, G, Gr, T> {
  fn prepare_turn<'a>(&'a self, asking: &'a TurnAsking) -> BoxFuture<'a, Result<Prepared, Refusal>> {
    Box::pin(async move { self.agency.prepare_turn(asking).await.map_err(agency_failed) })
  }

  fn send_turn_prompt<'a>(&'a self, turn: &'a str, prompt: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.send_turn_prompt(turn, prompt).await.map_err(agency_failed) })
  }

  fn work_turn<'a>(&'a self, turn: &'a str) -> BoxFuture<'a, Result<Option<WorkTurn>, Refusal>> {
    Box::pin(async move { self.agency.work_turn(turn).await.map_err(agency_failed) })
  }

  fn push_aside<'a>(
    &'a self,
    turn: &'a str,
    text: &'a str,
  ) -> BoxFuture<'a, Result<AsideOutcome, Refusal>> {
    Box::pin(async move { self.agency.push_aside(turn, text).await.map_err(agency_failed) })
  }

  fn answer_question<'a>(
    &'a self,
    turn: &'a str,
    question: &'a str,
    answer: &'a ConfirmAnswer,
  ) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(
      async move { self.agency.answer_question(turn, question, answer).await.map_err(agency_failed) },
    )
  }

  fn carry_out_reset<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.carry_out_reset(worker).await.map_err(agency_failed) })
  }

  fn record_session<'a>(
    &'a self,
    worker: &'a str,
    answered: &'a SessionAnswered,
    cwd: &'a str,
    job: i64,
  ) -> BoxFuture<'a, Result<SessionKept, Refusal>> {
    Box::pin(
      async move { self.agency.record_session(worker, answered, cwd, job).await.map_err(agency_failed) },
    )
  }

  fn hand_over_session<'a>(
    &'a self,
    worker: &'a str,
    handover: &'a Handover,
  ) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.hand_over_session(worker, handover).await.map_err(agency_failed) })
  }

  fn note_run<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.note_run(worker).await.map_err(agency_failed) })
  }

  fn record_usage<'a>(
    &'a self,
    worker: &'a str,
    reply: &'a Reply,
    run: Option<&'a str>,
  ) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.record_usage(worker, reply, run).await.map_err(agency_failed) })
  }

  fn wake_keepalive<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.resume_keepalive(worker).await.map_err(agency_failed) })
  }

  fn make_bench<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.make_bench(worker).await.map_err(agency_failed) })
  }

  fn sweep_servers<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    Box::pin(async move { self.agency.sweep_servers(worker).await.map_err(agency_failed) })
  }

  fn job_documents<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<JobDocuments, Refusal>> {
    Box::pin(async move { self.agency.job_documents(worker).await.map_err(agency_failed) })
  }

  fn prefix_touched<'a>(
    &'a self,
    worker: &'a str,
    files: &'a [String],
  ) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    Box::pin(async move { self.agency.prefix_touched(worker, files).await.map_err(agency_failed) })
  }

  fn model_health<'a>(&'a self, provider: &'a str, model: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.model_health(provider, model).await.map_err(agency_failed) })
  }

  fn rest_login<'a>(&'a self, account: &'a str, until: i64) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.rest_login(account, until).await.map_err(agency_failed) })
  }

  fn usage_rest<'a>(&'a self, account: &'a str, usage: &'a Value) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.usage_rest(account, usage).await.map_err(agency_failed) })
  }

  fn write_note<'a>(&'a self, note: &'a LogNote) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.write_note(note).await.map_err(agency_failed) })
  }

  fn node_copy<'a>(&'a self, node: &'a str, checkout: &'a str) -> BoxFuture<'a, Result<String, Refusal>> {
    Box::pin(async move { self.agency.node_copy(node, checkout).await.map_err(agency_failed) })
  }

  fn ssh_reach<'a>(&'a self, node: &'a str) -> BoxFuture<'a, Result<Option<SshReach>, Refusal>> {
    Box::pin(async move { self.agency.ssh_reach(node).await.map_err(agency_failed) })
  }

  fn sanitise<'a>(&'a self, asked: &'a Sanitise) -> BoxFuture<'a, Result<Sanitised, Refusal>> {
    Box::pin(async move { self.gate.sanitise(asked).await.map_err(gate_failed) })
  }

  fn kept<'a>(
    &'a self,
    language: &'a str,
    originals: &'a [String],
  ) -> BoxFuture<'a, Result<HashMap<String, String>, Refusal>> {
    Box::pin(async move { self.gate.kept(language, originals).await.map_err(gate_failed) })
  }

  fn knowledge<'a>(&'a self, asked: &'a KnowledgeAsked) -> BoxFuture<'a, Result<Vec<Piece>, Refusal>> {
    Box::pin(async move { self.gate.knowledge(asked).await.map_err(gate_failed) })
  }

  fn graph_named<'a>(&'a self, checkout: &'a str) -> BoxFuture<'a, Result<Named, Refusal>> {
    Box::pin(async move { self.graph.named(checkout).await.map_err(graph_failed) })
  }

  fn plan_units<'a>(
    &'a self,
    checkout: &'a Path,
    commit: &'a str,
    hold: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>> {
    Box::pin(async move { self.testbed.plan_units(checkout, commit, hold).await.map_err(testbed_failed) })
  }

  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    Box::pin(async move { self.testbed.relink_units(checkout).await.map_err(testbed_failed) })
  }

  fn queue_moved(&self) -> BoxFuture<'_, ()> {
    Box::pin(async move {
      let Some(server) = &self.server else { return };
      let Ok(url) = server.join("queue/moved") else { return };
      let told = reqwest::Client::new()
        .post(url)
        .json(&json!({ "detail": format!("bmo-job job-run[{}]", std::process::id()) }))
        .timeout(MOVED_WAIT)
        .send()
        .await;

      match told {
        Ok(answer) if answer.status().is_success() => {}
        Ok(answer) => eprintln!("bmo-job: the screens were not told the queue moved: {}", answer.status()),
        Err(why) => eprintln!("bmo-job: the screens were not told the queue moved: {why}"),
      }
    })
  }
}
