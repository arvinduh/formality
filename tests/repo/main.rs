//! Repository hygiene checks that guard files outside `src/`.
//!
//! Covers the committed JSON Schema, the release workflow's local edits, the
//! Rust toolchain pins, and the version lockstep with the VS Code extension.

mod release_workflow;
mod schema_drift;
mod source_rules;
mod toolchain_pins;
mod version_lockstep;
