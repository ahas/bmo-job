//! The service's integration tests, one module to an area, built as one test binary, since every binary links
//! the whole service: the runner's ticks and steps, the queue's telling and stops, the verbs a job and a person
//! run, and the TODO's verbs, each on a migrated database of its own and the stand-ins of the units.

#[path = "../support/mod.rs"]
mod support;

mod ask_verb;
mod branch_places;
mod control;
mod final_turn;
mod job_close;
mod job_figures;
mod job_guard;
mod job_run;
mod job_verbs;
mod landing;
mod machine_verbs;
mod queue_steps;
mod research_turns;
mod stops;
mod telling;
mod todo_verb;
mod tool_calls;
mod verbs;
mod work_node_verbs;
mod worker_reading;
