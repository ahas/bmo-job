//! The calls the queue and the work-order runner make on other units (docs/queue.md, "Where the queue
//! stands"): one trait a test stands in for, and the real ports carrying it. A call that fails is refused
//! with 502 under the unit's code.

use std::path::Path;

use futures_util::future::BoxFuture;
use job_agency::{
  Agency, AgencyError, BranchSlots, Execution, JobLines, LaunchHeld, LogLine, Mapping, Node, PinChoice,
  PoolAsking, PoolRequest, PoolRole, Provider, Reservation, ReservationAsking, RouteAsk, Run, Tier,
  Workbench, WorkerRuntime,
};
use job_codes::{JobError, ProviderRefusal, UnitRefusal};
use job_gate::{Gate, GateError};
use job_receiver::{Answerers, Instruction, InstructionState, Notice, Receiver, Room};
use job_testbed::{Pending, PendingOf, Relinked, Testbed};
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
  /// Whether the deployment rests now.
  fn deployment_resting(&self) -> BoxFuture<'_, Result<bool, Refusal>>;
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
  /// The model tiers, in the ladder's order.
  fn tiers(&self) -> BoxFuture<'_, Result<Vec<Tier>, Refusal>>;
  /// The roles the pool knows.
  fn pool_roles(&self) -> BoxFuture<'_, Result<Vec<PoolRole>, Refusal>>;
  /// A request for a co-worker's place enqueued, or the one its key already names.
  fn enqueue_request<'a>(&'a self, asking: &'a PoolAsking) -> BoxFuture<'a, Result<PoolRequest, Refusal>>;
  /// A pool request by its token, none where there is no such request.
  fn pool_request<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>>;
  /// The pool requests waiting or active, an owner's or everyone's.
  fn open_requests<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<PoolRequest>, Refusal>>;
  /// A request let into its place where its turn has come; none while it waits on.
  fn lease_place<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>>;
  /// The unit a lease is carried into, bound to it.
  fn bind_child<'a>(
    &'a self,
    token: &'a str,
    child: &'a str,
  ) -> BoxFuture<'a, Result<PoolRequest, Refusal>>;
  /// Seats reserved, or the reservation its key already names moved to the count asked.
  fn reserve_seats<'a>(
    &'a self,
    asking: &'a ReservationAsking,
  ) -> BoxFuture<'a, Result<Reservation, Refusal>>;
  /// The reservations held, an owner's or everyone's.
  fn held_reservations<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<Reservation>, Refusal>>;
  /// A reservation given back; whether it was still held.
  fn release_reservation<'a>(
    &'a self,
    token: &'a str,
    reason: &'a str,
  ) -> BoxFuture<'a, Result<bool, Refusal>>;
  /// Every execution, ended or not.
  fn all_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>>;
  /// How long a place's prefix is worth counting on for a provider's model.
  fn place_cache<'a>(
    &'a self,
    provider: &'a str,
    model: &'a str,
    upstream: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Option<i64>, Refusal>>;
  /// The prefix a worker's run would open on in a directory now.
  fn prefix_digest<'a>(
    &'a self,
    cwd: &'a str,
    model: &'a str,
    effort: Option<&'a str>,
  ) -> BoxFuture<'a, Result<String, Refusal>>;
  /// What stands for a branch place taken down.
  fn retire_place<'a>(&'a self, pool: &'a str, slot: i64) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A pool's branch places.
  fn branch_slots<'a>(&'a self, pool: &'a str) -> BoxFuture<'a, Result<BranchSlots, Refusal>>;
  /// The installed units linked into a checkout relinked where its files moved.
  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>>;
  /// Texts put into English, in the order given; a text in English already comes back as it stands.
  fn english<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<String>, Refusal>>;
}

/// The language a worker reads and writes, as bmo-gate names it.
pub const WORKER_LANGUAGE: &str = "en";

/// A failed call on bmo-gate, refused under its code.
pub fn gate_failed(why: impl std::fmt::Display) -> Refusal {
  JobError::from(UnitRefusal::Gate {
    failure: why.to_string(),
  })
  .into()
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

/// The calls made over the real ports.
pub struct Ports<'a, A, R, W, T, G> {
  pub agency: &'a A,
  pub receiver: &'a R,
  pub web: &'a W,
  pub testbed: &'a T,
  pub gate: &'a G,
}

impl<A: Agency, R: Receiver, W: Web, T: Testbed, G: Gate> Units for Ports<'_, A, R, W, T, G> {
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

  fn deployment_resting(&self) -> BoxFuture<'_, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.resting().await.map_err(agency_failed) })
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

  fn tiers(&self) -> BoxFuture<'_, Result<Vec<Tier>, Refusal>> {
    Box::pin(async move { self.agency.tiers().await.map_err(agency_failed) })
  }

  fn pool_roles(&self) -> BoxFuture<'_, Result<Vec<PoolRole>, Refusal>> {
    Box::pin(async move { self.agency.pool_roles().await.map_err(agency_failed) })
  }

  fn enqueue_request<'a>(&'a self, asking: &'a PoolAsking) -> BoxFuture<'a, Result<PoolRequest, Refusal>> {
    Box::pin(async move { self.agency.enqueue_request(asking).await.map_err(agency_failed) })
  }

  fn pool_request<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>> {
    Box::pin(async move { self.agency.pool_request(token).await.map_err(agency_failed) })
  }

  fn open_requests<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<PoolRequest>, Refusal>> {
    Box::pin(async move { self.agency.open_requests(owner).await.map_err(agency_failed) })
  }

  fn lease_place<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>> {
    Box::pin(async move { self.agency.lease_place(token).await.map_err(agency_failed) })
  }

  fn bind_child<'a>(
    &'a self,
    token: &'a str,
    child: &'a str,
  ) -> BoxFuture<'a, Result<PoolRequest, Refusal>> {
    Box::pin(async move { self.agency.bind_child(token, child).await.map_err(agency_failed) })
  }

  fn reserve_seats<'a>(
    &'a self,
    asking: &'a ReservationAsking,
  ) -> BoxFuture<'a, Result<Reservation, Refusal>> {
    Box::pin(async move { self.agency.reserve_seats(asking).await.map_err(agency_failed) })
  }

  fn held_reservations<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<Reservation>, Refusal>> {
    Box::pin(async move { self.agency.held_reservations(owner).await.map_err(agency_failed) })
  }

  fn release_reservation<'a>(
    &'a self,
    token: &'a str,
    reason: &'a str,
  ) -> BoxFuture<'a, Result<bool, Refusal>> {
    Box::pin(async move { self.agency.release_reservation(token, reason).await.map_err(agency_failed) })
  }

  fn all_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>> {
    Box::pin(async move { self.agency.executions(false).await.map_err(agency_failed) })
  }

  fn place_cache<'a>(
    &'a self,
    provider: &'a str,
    model: &'a str,
    upstream: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Option<i64>, Refusal>> {
    Box::pin(async move { self.agency.place_cache(provider, model, upstream).await.map_err(agency_failed) })
  }

  fn prefix_digest<'a>(
    &'a self,
    cwd: &'a str,
    model: &'a str,
    effort: Option<&'a str>,
  ) -> BoxFuture<'a, Result<String, Refusal>> {
    Box::pin(async move { self.agency.prefix_digest(cwd, model, effort).await.map_err(agency_failed) })
  }

  fn retire_place<'a>(&'a self, pool: &'a str, slot: i64) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move { self.agency.retire_place(pool, slot).await.map_err(agency_failed) })
  }

  fn branch_slots<'a>(&'a self, pool: &'a str) -> BoxFuture<'a, Result<BranchSlots, Refusal>> {
    Box::pin(async move { self.agency.branch_slots(pool).await.map_err(agency_failed) })
  }

  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    Box::pin(async move { self.testbed.relink_units(checkout).await.map_err(testbed_failed) })
  }

  fn english<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    Box::pin(async move {
      let readings = self
        .gate
        .translate(WORKER_LANGUAGE, texts)
        .await
        .map_err(|why: GateError| gate_failed(why))?;

      // A text not wholly put into English stands under the mark a worker reads it by.
      Ok(
        readings
          .into_iter()
          .map(|reading| {
            if reading.whole {
              reading.text
            } else {
              format!("{}\n{}", queue_read::worker_lines::UNTRANSLATED_MARK, reading.text)
            }
          })
          .collect(),
      )
    })
  }
}
