//! A job's turn assembled and judged here and carried by bmo-agency (docs/runs.md, "The turns"): the turn's
//! prompt and the notes pushed into it, a question its program stops on answered or put to the owner, and what
//! a failed turn was.

pub mod confirm;
pub mod failure;
pub mod own_work;

pub mod turn;
