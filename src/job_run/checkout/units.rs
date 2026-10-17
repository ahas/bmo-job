//! The installed units linked into a checkout rather than into a deploy tree, kept following their files as
//! main moves there (docs/runs.md, "Landing on main"). bmo-testbed keeps the links and their rule: what a move
//! asks of them, and every link a commit left dangling pointed at the commit's one tracked file of the unit's
//! own name. A move it holds is refused here, and the links it planned are relinked once main has moved.

use std::path::Path;

use futures_util::future::BoxFuture;
use job_policy::Policy;
use job_testbed::{Relink, Relinked, Testbed, UnitPlan};

use super::Checkout;
use crate::Refusal;
use crate::job_run::calls::Calls;
use crate::queue::units::testbed_failed;

/// What a checkout's unit links are asked through: bmo-testbed's plan of a move, and its relink.
pub trait UnitLinks: Sync {
  fn plan_units<'a>(
    &'a self,
    checkout: &'a Path,
    commit: &'a str,
    hold: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>>;
  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>>;
}

impl UnitLinks for dyn Calls + '_ {
  fn plan_units<'a>(
    &'a self,
    checkout: &'a Path,
    commit: &'a str,
    hold: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>> {
    Calls::plan_units(self, checkout, commit, hold)
  }

  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    Calls::relink_units(self, checkout)
  }
}

/// The unit links asked of bmo-testbed through its port.
pub struct TestbedLinks<'t, T>(pub &'t T);

impl<T: Testbed> UnitLinks for TestbedLinks<'_, T> {
  fn plan_units<'a>(
    &'a self,
    checkout: &'a Path,
    commit: &'a str,
    hold: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>> {
    Box::pin(async move { self.0.plan_units(checkout, commit, hold).await.map_err(testbed_failed) })
  }

  fn relink_units<'a>(&'a self, checkout: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    Box::pin(async move { self.0.relink_units(checkout).await.map_err(testbed_failed) })
  }
}

/// Why a checkout may not move where units are linked into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitsHeld {
  /// Units running in a folder the commit no longer has.
  Running(Vec<String>),
  /// Links the commit would leave dangling with no one file of their unit's name to point at, or a plan
  /// bmo-testbed could not give.
  Unmatched(Vec<String>),
}

/// The unit links into a checkout, as bmo-testbed keeps them, and the words their following is said in.
pub struct CheckoutUnits<'a> {
  pub links: &'a dyn UnitLinks,
  pub policy: &'a Policy,
}

impl CheckoutUnits<'_> {
  /// What moving `checkout` to `commit` asks of the links into it: each one the commit would leave dangling,
  /// relinked to its file there. Where `hold` is set, a unit running in a folder the commit lacks refuses the
  /// move first, since its process would fail on its next relative path.
  pub async fn plan(
    &self,
    checkout: &Checkout,
    commit: &str,
    hold: bool,
  ) -> Result<Vec<Relink>, UnitsHeld> {
    let plan = self
      .links
      .plan_units(Path::new(&checkout.cwd), commit, hold)
      .await
      .map_err(|why| UnitsHeld::Unmatched(vec![why.error]))?;

    if hold && !plan.running.is_empty() {
      return Err(UnitsHeld::Running(plan.running));
    }

    if !plan.unmatched.is_empty() {
      return Err(UnitsHeld::Unmatched(plan.unmatched));
    }

    Ok(plan.relinks)
  }

  /// Every link the checkout's present commit left dangling relinked once main has moved, saying so; what could
  /// not be relinked, or still resolves nowhere, is said as well. Nothing is asked where no link was planned.
  pub async fn follow(&self, checkout: &Checkout, relinks: &[Relink]) {
    if relinks.is_empty() {
      return;
    }

    let said = &self.policy.landing.units;
    let names = relinks.iter().map(|one| one.name.as_str()).collect::<Vec<_>>().join(", ");

    match self.links.relink_units(Path::new(&checkout.cwd)).await {
      Ok(relinked) => {
        eprintln!("{}", self.policy.fill(&said.relinked, &[("units", &names), ("cwd", &checkout.cwd)]));

        if !relinked.dangling.is_empty() {
          eprintln!(
            "{}",
            self
              .policy
              .fill(&said.dangling, &[("units", &relinked.dangling.join(", ")), ("cwd", &checkout.cwd)])
          );
        }

        if !relinked.unmatched.is_empty() {
          eprintln!("{}", self.policy.fill(&said.unmatched, &[("faults", &relinked.unmatched.join("; "))]));
        }
      }
      Err(why) => {
        eprintln!("{}", self.policy.fill(&said.failed, &[("units", &names), ("why", &why.error)]))
      }
    }
  }

  /// Relinks whatever the commit `checkout` now stands on left dangling, holding nothing, since the files have
  /// already moved: a base checkout put back on main as its job ends.
  pub async fn follow_moved(&self, checkout: &Checkout) {
    match self.plan(checkout, "HEAD", false).await {
      Ok(relinks) => self.follow(checkout, &relinks).await,
      Err(UnitsHeld::Unmatched(faults) | UnitsHeld::Running(faults)) => {
        eprintln!(
          "{}",
          self
            .policy
            .fill(&self.policy.landing.units.unmatched, &[("faults", &faults.join("; "))])
        );
      }
    }
  }
}
