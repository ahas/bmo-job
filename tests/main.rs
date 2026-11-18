//! The crate's one test harness (bmo-chat's docs/conventions.md, 「Rust builds」): each file of tests a
//! module of it, sharing the process.

mod support;

mod access;
mod ask_words;
mod events;
mod hangul;
mod import;
mod item_changes;
mod items;
mod job_ask;
mod job_close;
mod job_close_checkouts;
mod layout;
mod leases;
mod migrate;
mod notes;
mod openapi;
mod plugin;
mod port;
mod questions;
mod reviews;
mod scopes;
mod serve;
mod subscriptions;
mod todo_cli;
mod work_orders;
