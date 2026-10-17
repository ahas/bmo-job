//! A job's run (docs/queue.md): its checkout and what it holds open, a work order's shaping turn and the check
//! of its answer, what a failed run says of its road, and the words a run's ending and its record carry.

pub mod checkout;
pub mod plan_check;
pub mod recovery;
pub mod worktree;

/// The session word a builder turn of a co-work is launched with: the worker's own session carried on.
pub const WORK_SESSION_RESUME: &str = "resume";

/// The session word a co-work run that throws its session away is launched with.
pub const WORK_SESSION_FRESH: &str = "fresh";

/// The session word a stopped stage's retry is launched with: the worker's kept session handed over first,
/// so the turn opens a fresh one.
pub const WORK_SESSION_HANDOVER: &str = "handover";

/// The phase a step of a work order runs in, before its final turn.
pub const WORK_STEP_PHASE: &str = "work-step";

/// The phase a work order's final turn runs in.
pub const WORK_FINAL_PHASE: &str = "work-final";

/// The label the line naming what a run left running opens with, which a reader looks for in a record.
pub const LIVE_LABEL: &str = "Left running";

/// The label the line naming the Claude Workflows a run walked away from opens with.
pub const ABANDONED_LABEL: &str = "Workflows not finished";
