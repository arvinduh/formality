//! The few things only the `fml` process shows: exit codes and the LSP
//! server's stdio protocol.
//!
//! Everything else is tested through the library, in the `api` crate.

mod common;
mod exit_codes;
mod lsp;
