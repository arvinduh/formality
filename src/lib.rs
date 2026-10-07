//! Formality (`fml`) library for formatting, linting, and syncing configurations across multiple language surfaces.
//!
//! Owns configuration, the execution engine, language surfaces, and errors.
//! The library returns data and never prints; argument parsing, rendering,
//! and process hosting belong to the `fml` binary (`src/main.rs`, `src/cli/`).

/// Configuration loading, parsing, and resolving.
pub mod config;
/// Execution engine for running formatters, linters, and version checks.
pub mod engine;
/// Language surface definitions and registry.
pub mod surfaces;
