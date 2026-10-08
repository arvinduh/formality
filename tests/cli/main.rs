//! The few things only the `fml` process shows: exit codes, the LSP
//! server's stdio protocol, and `fml update` replacing its own binary.
//!
//! Everything else is tested through the library, in the `api` crate.

mod common;
mod exit_codes;
mod lsp;
mod update;
