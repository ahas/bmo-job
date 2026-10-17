//! The calls the queue makes on other units (docs/queue.md, "Where the queue stands"): one trait a test
//! stands in for, and the real ports carrying it. A call that fails is refused with 502 under the unit's
//! code.

use futures_util::future::BoxFuture;
use job_agency::{
  Agency, AgencyError, Execution, JobLines, LaunchHeld, LogLine, Mapping, Node, PinChoice, Provider,
  RouteAsk, Run, Workbench, WorkerRuntime,
};
use job_codes::{JobError, ProviderRefusal, UnitRefusal};
use job_receiver::{Answerers, Instruction, InstructionState, Notice, Receiver, Room};
use job_testbed::{Pending, PendingOf, Testbed};
use job_web::{Checked, ObjectCheck, Web};

use crate::Refusal;

/// Every call the queue makes on other units.
pub trait Units: Sync {
  /// The providers, in the provider order.
  fn providers(&self) -> BoxFuture<'_, Result<Vec<Provider>, Refusal>>;
  /// The model a route resolves to, none where no route serves it.
  fn resolve_route<'a>(&'a self, asked: &'a RouteAsk) -> BoxFuture<'a, Result<Option<Mapping>, Refusal>>;
  /// A worker's runtime, with the pin in force.
  fn worker_runtime<'a>(&'a self, worker: &'a str)
  -> BoxFuture<'a, Result<Option<WorkerRuntime>, Refusal>>;
  /// What a pin's value chooses; a value bmo-agency refuses is refused `pin_wrong`.
  fn check_pin<'a>(&'a self, value: &'a str) -> BoxFuture<'a, Result<PinChoice, Refusal>>;
  /// The executions not yet ended.
  fn open_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>>;
  /// An execution asked cancelled, with every one it covers.
  fn cancel_execution<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), Refusal>>;
  /// The cancel asked of an execution and of every one it covers dropped; those dropped.
  fn drop_cancels<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>>;
  /// Whether the deployment's agent rests now.
  fn agent_resting(&self) -> BoxFuture<'_, Result<bool, Refusal>>;
  /// The work nodes.
  fn nodes(&self) -> BoxFuture<'_, Result<Vec<Node>, Refusal>>;
  /// A request for a co-worker's place finished with its outcome, cancelled where it never ran; false
  /// where it had already ended.
  fn finish_request<'a>(
    &'a self,
    token: &'a str,
    outcome: &'a str,
    cancel: bool,
  ) -> BoxFuture<'a, Result<bool, Refusal>>;
  /// An instruction queued for an agent's session, answering its id.
  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>>;
  /// How far an instruction has gone, none where bmo-receiver holds no such instruction.
  fn instruction_state(&self, id: i64) -> BoxFuture<'_, Result<Option<InstructionState>, Refusal>>;
  /// An instruction not yet handed to a turn withdrawn; whether it was.
  fn withdraw_instruction(&self, id: i64) -> BoxFuture<'_, Result<bool, Refusal>>;
  /// A fact voiced in a room by the agent that speaks for it, answering the instruction's id.
  fn voice_notice<'a>(&'a self, chat: i64, notice: &'a Notice) -> BoxFuture<'a, Result<i64, Refusal>>;
  /// What a room is to the deployment.
  fn room(&self, chat: i64) -> BoxFuture<'_, Result<Room, Refusal>>;
  /// The agents who answer in a room.
  fn answerers(&self, chat: i64) -> BoxFuture<'_, Result<Answerers, Refusal>>;
  /// A run of the ledger, none where there is no such run.
  fn run(&self, run: i64) -> BoxFuture<'_, Result<Option<Run>, Refusal>>;
  /// Whether a user holds an enabled Claude account.
  fn claude_account_held<'a>(&'a self, user: &'a str) -> BoxFuture<'a, Result<bool, Refusal>>;
  /// Whether the account model is in force.
  fn model_in_force(&self) -> BoxFuture<'_, Result<bool, Refusal>>;
  /// The user a sender is linked to, none where it is linked to none.
  fn user_of_sender(&self, sender: i64) -> BoxFuture<'_, Result<Option<String>, Refusal>>;
  /// Whether a user holds a relation on a worker or a repository.
  fn check_object<'a>(&'a self, check: &'a ObjectCheck) -> BoxFuture<'a, Result<Checked, Refusal>>;
  /// The room on the machine, with why launches are held where they are.
  fn machine_room(&self) -> BoxFuture<'_, Result<job_agency::Room, Refusal>>;
  /// bmo-agency told launches are held, and why; an empty list withdraws the notice.
  fn tell_launch_held<'a>(&'a self, notice: &'a LaunchHeld) -> BoxFuture<'a, Result<(), Refusal>>;
  /// The commits waiting to go live that a job landed.
  fn pending_deploys(&self, job: i64) -> BoxFuture<'_, Result<Vec<Pending>, Refusal>>;
  /// How many starts an agent's run has logged.
  fn run_starts<'a>(&'a self, agent: &'a str, run: &'a str) -> BoxFuture<'a, Result<u64, Refusal>>;
  /// The last line a job's run logged, none where it logged none.
  fn job_last_line(&self, job: i64) -> BoxFuture<'_, Result<Option<LogLine>, Refusal>>;
  /// What a job's run logged.
  fn job_lines(&self, job: i64) -> BoxFuture<'_, Result<JobLines, Refusal>>;
  /// A worker's workbench.
  fn workbench<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<Workbench, Refusal>>;
}

/// A failed call on bmo-testbed, refused under its code.
pub fn testbed_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Testbed {
    failure: why.to_string(),
  })
  .into()
}

/// A failed call on bmo-web, refused under its code.
pub fn web_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Web {
    failure: why.to_string(),
  })
  .into()
}

/// A failed call on bmo-receiver, refused under its code.
pub fn receiver_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Receiver {
    failure: why.to_string(),
  })
  .into()
}

/// A failed call on bmo-agency, refused under its code.
pub fn agency_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Agency {
    failure: why.to_string(),
  })
  .into()
}

/// The calls made over the real ports, the agent's own by its key.
pub struct Ports<'a, A, R, W, T> {
  pub agency: &'a A,
  pub receiver: &'a R,
  pub web: &'a W,
  pub testbed: &'a T,
  pub agent_key: &'a str,
}

impl<A: Agency, R: Receiver, W: Web, T: Testbed> Units for Ports<'_, A, R, W, T> {
  fn providers(&self) -> BoxFuture<'_, Result<Vec<Provider>, Refusal>> {
    Box::pin(async move { self.agency.providers().await.map_err(agency_failed) })
  }

  fn resolve_route<'a>(&'a self, asked: &'a RouteAsk) -> BoxFuture<'a, Result<Option<Mapping>, Refusal>> {
    Box::pin(async move { self.agency.resolve_route(asked).await.map_err(agency_failed) })
  }

  fn worker_runtime<'a>(
    &'a self,
    worker: &'a str,
  ) -> BoxFuture<'a, Result<Option<WorkerRuntime>, Refusal>> {
    Box::pin(async move { self.agency.worker_runtime(worker).await.map_err(agency_failed) })
  }

  fn check_pin<'a>(&'a self, value: &'a str) -> BoxFuture<'a, Result<PinChoice, Refusal>> {
    Box::pin(async move {
      self.agency.check_pin(value).await.map_err(|failed| match failed {
        AgencyError::Refused {
          status: 400, error, ..
        } => JobError::from(ProviderRefusal::PinWrong {
          value: value.to_owned(),
          said: error,
        })
        .into(),
        other => agency_failed(other),
      })
    })
  }

  fn open_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>> {
    Box::pin(async move { self.agency.executions(true).await.map_err(agency_failed) })
  }

  fn cancel_execution<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.cancel_execution(id).await.map_err(agency_failed) })
  }

  fn drop_cancels<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    Box::pin(async move { self.agency.drop_cancels(id).await.map_err(agency_failed) })
  }

  fn agent_resting(&self) -> BoxFuture<'_, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.resting(self.agent_key).await.map_err(agency_failed) })
  }

  fn nodes(&self) -> BoxFuture<'_, Result<Vec<Node>, Refusal>> {
    Box::pin(async move { self.agency.nodes().await.map_err(agency_failed) })
  }

  fn finish_request<'a>(
    &'a self,
    token: &'a str,
    outcome: &'a str,
    cancel: bool,
  ) -> BoxFuture<'a, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.finish_request(token, outcome, cancel).await.map_err(agency_failed) })
  }

  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>> {
    Box::pin(async move { self.receiver.queue_instruction(instruction).await.map_err(receiver_failed) })
  }

  fn instruction_state(&self, id: i64) -> BoxFuture<'_, Result<Option<InstructionState>, Refusal>> {
    Box::pin(async move { self.receiver.instruction_state(id).await.map_err(receiver_failed) })
  }

  fn withdraw_instruction(&self, id: i64) -> BoxFuture<'_, Result<bool, Refusal>> {
    Box::pin(async move { self.receiver.withdraw_instruction(id).await.map_err(receiver_failed) })
  }

  fn voice_notice<'a>(&'a self, chat: i64, notice: &'a Notice) -> BoxFuture<'a, Result<i64, Refusal>> {
    Box::pin(async move { self.receiver.voice_notice(chat, notice).await.map_err(receiver_failed) })
  }

  fn room(&self, chat: i64) -> BoxFuture<'_, Result<Room, Refusal>> {
    Box::pin(async move { self.receiver.room(chat).await.map_err(receiver_failed) })
  }

  fn answerers(&self, chat: i64) -> BoxFuture<'_, Result<Answerers, Refusal>> {
    Box::pin(async move { self.receiver.answerers(chat).await.map_err(receiver_failed) })
  }

  fn run(&self, run: i64) -> BoxFuture<'_, Result<Option<Run>, Refusal>> {
    Box::pin(async move { self.agency.run(run).await.map_err(agency_failed) })
  }

  fn claude_account_held<'a>(&'a self, user: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.claude_account_held(user).await.map_err(agency_failed) })
  }

  fn model_in_force(&self) -> BoxFuture<'_, Result<bool, Refusal>> {
    Box::pin(async move { self.web.model_in_force().await.map_err(web_failed) })
  }

  fn user_of_sender(&self, sender: i64) -> BoxFuture<'_, Result<Option<String>, Refusal>> {
    Box::pin(async move { self.web.user_of_sender(sender).await.map_err(web_failed) })
  }

  fn check_object<'a>(&'a self, check: &'a ObjectCheck) -> BoxFuture<'a, Result<Checked, Refusal>> {
    Box::pin(async move { self.web.check_object(check).await.map_err(web_failed) })
  }

  fn machine_room(&self) -> BoxFuture<'_, Result<job_agency::Room, Refusal>> {
    Box::pin(async move { self.agency.machine_room().await.map_err(agency_failed) })
  }

  fn tell_launch_held<'a>(&'a self, notice: &'a LaunchHeld) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.tell_launch_held(notice).await.map_err(agency_failed) })
  }

  fn pending_deploys(&self, job: i64) -> BoxFuture<'_, Result<Vec<Pending>, Refusal>> {
    Box::pin(async move { self.testbed.pending(PendingOf::Job(job)).await.map_err(testbed_failed) })
  }

  fn run_starts<'a>(&'a self, agent: &'a str, run: &'a str) -> BoxFuture<'a, Result<u64, Refusal>> {
    Box::pin(async move { self.agency.run_starts(agent, run).await.map_err(agency_failed) })
  }

  fn job_last_line(&self, job: i64) -> BoxFuture<'_, Result<Option<LogLine>, Refusal>> {
    Box::pin(async move { self.agency.job_last_line(job).await.map_err(agency_failed) })
  }

  fn job_lines(&self, job: i64) -> BoxFuture<'_, Result<JobLines, Refusal>> {
    Box::pin(async move { self.agency.job_lines(job).await.map_err(agency_failed) })
  }

  fn workbench<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<Workbench, Refusal>> {
    Box::pin(async move { self.agency.workbench(worker).await.map_err(agency_failed) })
  }
}
