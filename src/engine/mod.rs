//! Formatting/linting engine: subprocess orchestration, diffing, tool
//! version detection, and self-update checking.

pub mod diff;
pub mod runner;
pub mod update;
pub mod version;

pub use diff::render_diff;
pub use runner::{Runner, RunnerAction};
pub use update::{UpdateNotifier, print_update_notice, spawn_update_check};
