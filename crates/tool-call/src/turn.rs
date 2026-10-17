//! The turn a call is made in, as its session's run variables carry it (docs/tools.md, "A call"): the room it
//! answers, its agent and its run.

use std::collections::BTreeMap;

use serde_json::Value;

/// The variable naming the room a turn answers.
pub const CHAT_VARIABLE: &str = "BMO_TURN_CHAT";

/// The variable naming a turn's agent.
pub const AGENT_VARIABLE: &str = "BMO_TURN_AGENT";

/// The variable naming a turn's run.
pub const RUN_NAME_VARIABLE: &str = "BMO_RUN_NAME";

/// The variable a middle step of a work order carries.
pub const WORK_ORDER_STEP_VARIABLE: &str = "BMO_WORK_ORDER_STEP";

/// The refusal of a call naming no room in a turn that answers none.
pub const NO_ROOM: &str = "no room is named: name one with chat, or call this inside a room's turn";

/// What a call's turn carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
  /// Every run variable the session handed, as it handed them.
  pub variables: BTreeMap<String, String>,
}

impl Turn {
  /// The turn a session carries: its `runVariables`, every one that is a string.
  pub fn of(session: &Value) -> Self {
    let variables = session
      .get("runVariables")
      .and_then(Value::as_object)
      .into_iter()
      .flatten()
      .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
      .collect();

    Self { variables }
  }

  fn set(&self, name: &str) -> Option<&str> {
    self.variables.get(name).map(|value| value.trim()).filter(|value| !value.is_empty())
  }

  /// The room the turn answers.
  pub fn chat(&self) -> Option<i64> {
    self.set(CHAT_VARIABLE).and_then(|chat| chat.parse().ok())
  }

  pub fn agent(&self) -> Option<&str> {
    self.set(AGENT_VARIABLE)
  }

  /// The room a call reads: the one it names, or the turn's own.
  pub fn room(&self, named: Option<i64>) -> Option<i64> {
    named.or_else(|| self.chat())
  }

  /// The variables the unit CLI is started with, so a verb works for the turn's agent and run: the agent, the
  /// run and the work-order step, each where the turn carries it.
  pub fn verb_variables(&self) -> Vec<(&'static str, String)> {
    [AGENT_VARIABLE, RUN_NAME_VARIABLE, WORK_ORDER_STEP_VARIABLE]
      .into_iter()
      .filter_map(|name| Some((name, self.set(name)?.to_owned())))
      .collect()
  }
}
