//! The settings the policy's tests load it with.

// Each test file is its own crate and uses only part of this module.
#![allow(dead_code)]

use job_base::Settings;
use job_policy::{Policy, PolicyError};

/// The checked-in example settings file.
pub const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../settings.example.toml");

/// Settings naming the owner and the agent as given, with `words` written after them as they stand.
pub fn settings(owner: &str, agent: &str, words: &str) -> Settings {
  let text = format!(
    "[database]\nurl = \"sqlite::memory:\"\n\n[deployment]\nowner_name = {owner:?}\nagent_name = {agent:?}\n\
     agent_key = \"agent\"\nsender_name = \"Sender\"\nreceiver_unit = \"receiver\"\n\n{words}"
  );

  Settings::read(&text).unwrap_or_else(|error| panic!("the test settings read: {error}"))
}

/// The policy as a deployment named Owner and Agent with no words of its own reads it.
pub fn policy() -> Policy {
  load("")
}

/// The policy as a deployment named Owner and Agent with `words` reads it.
pub fn load(words: &str) -> Policy {
  Policy::load(&settings("Owner", "Agent", words))
    .unwrap_or_else(|error| panic!("the policy loads: {error}"))
}

/// Why the policy refuses `words`.
pub fn refusal(words: &str) -> PolicyError {
  Policy::load(&settings("Owner", "Agent", words)).expect_err("the policy refuses the words")
}
