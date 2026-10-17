//! What the service tells the turns of a work order it runs: the line every turn that decides what gets
//! built is held to, and the design the planning turn a work order starts on is asked for, both the
//! policy's, filled with the deployment's names.

use job_policy::Policy;

/// The structural-fix line, in the deployment's names.
pub fn structural_fixes(policy: &Policy) -> String {
  policy.fill(&policy.work_turns.structural_fixes, &[])
}

/// What the planning turn is asked for: a design in the worker's own words, naming the roles a plan may
/// lend its tasks.
pub fn plan_order(policy: &Policy, roles: &[String]) -> String {
  let roles = roles.join(", ");
  let fixes = structural_fixes(policy);
  let marks = [("roles", roles.as_str()), ("structural_fixes", fixes.as_str())];

  policy
    .work_turns
    .plan
    .iter()
    .map(|paragraph| policy.fill(paragraph, &marks))
    .collect::<Vec<_>>()
    .join(" ")
}
