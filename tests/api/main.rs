//! Tests of `fml`'s public library API.
//!
//! These call the library directly rather than spawning the binary: the
//! surface registry and its agreement with the install and MSTV tables,
//! `.editorconfig` generation, and the `fmt`/`lint`/`fix`/`sync` passes run
//! against synthetic repositories. Only process-level behaviour is tested
//! through the binary, in the `cli` crate.

mod common;
mod editorconfig;
mod fix;
mod fmt;
mod registry;
mod registry_agreement;
mod sync;
