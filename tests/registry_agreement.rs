//! Cross-registry agreement: the fleet's surfaces, the install-chain table
//! (`ALL_CHAINS`, keyed by binary name), and the MSTV table
//! (`TOOL_MSTV_REGISTRY`, also keyed by binary name) are three independent
//! side-tables that nothing forces to stay in sync with each other. Issue
//! #276: `ktfmt` sat in `TOOL_MSTV_REGISTRY` with no surface and no
//! `ALL_CHAINS` row, unreachable, until a manual audit (#268) caught it.
//!
//! This test pairs the tables in both directions:
//!   1. every binary a surface declares via `tool_info` has an `ALL_CHAINS`
//!      row *and* a `TOOL_MSTV_REGISTRY` entry, after each table's own
//!      canonicalisation, or is on `EXEMPTIONS` with a reason;
//!   2. every `TOOL_MSTV_REGISTRY` entry corresponds to some surface-declared
//!      binary (the direction that would have caught `ktfmt`), or is on
//!      `EXEMPTIONS`.
//!
//! Deliberately not checked: `ALL_CHAINS` entries with no surface (e.g.
//! `tinymist`) pairing back to a declared binary — issue #276 scopes the
//! keyset test to the two directions above, not a third.

use fml::config::ResolvedLangConfig;
use fml::engine::version::mstv;
use fml::surfaces::{all_surfaces, tooling};
use std::collections::BTreeSet;

/// Binaries exempt from the "must have both an `ALL_CHAINS` row and a
/// `TOOL_MSTV_REGISTRY` entry" rule, with a reason each. Per issue #276,
/// exemptions must be explicit here, not silent gaps in the test.
const EXEMPTIONS: &[(&str, &str)] = &[
  (
    "cargo",
    "ships with rustup, not a separately installed/versioned tool; no \
     install chain or MSTV floor applies",
  ),
  (
    "gofmt",
    "ships with the Go toolchain (no standalone install chain of its own — \
     see ALL_CHAINS's doc comment on gofmt/goimports); it does carry an MSTV \
     floor, so it is exempt only from the ALL_CHAINS side of the pairing",
  ),
  (
    "google-java-format",
    "no MSTV floor is enforced today; deciding one is a separate call, not \
     part of #276's scope, so this is a real gap on the MSTV side of the \
     pairing rather than a bug this test should surface",
  ),
];

fn is_exempt(binary: &str) -> bool {
  EXEMPTIONS.iter().any(|(b, _)| *b == binary)
}

/// All binaries any registered surface declares via `tool_info`.
fn declared_binaries() -> BTreeSet<&'static str> {
  all_surfaces()
    .iter()
    .flat_map(|s| {
      let config = ResolvedLangConfig::new(s.name());
      s.tool_info(&config).into_iter().map(|t| t.binary)
    })
    .collect()
}

#[test]
fn test_declared_binaries_have_chain_and_mstv_rows() {
  let mut missing_chain = Vec::new();
  let mut missing_mstv = Vec::new();

  for binary in declared_binaries() {
    if is_exempt(binary) {
      continue;
    }
    if tooling::install_chain_for(binary).is_none() {
      missing_chain.push(binary);
    }
    if mstv::get_tool_mstv_entry(binary).is_none() {
      missing_mstv.push(binary);
    }
  }

  assert!(
    missing_chain.is_empty(),
    "surface-declared binaries with no ALL_CHAINS row and no exemption: \
     {missing_chain:?} (add a chain row in src/surfaces/tooling.rs, or an \
     exemption in tests/registry_agreement.rs::EXEMPTIONS)"
  );
  assert!(
    missing_mstv.is_empty(),
    "surface-declared binaries with no TOOL_MSTV_REGISTRY entry and no \
     exemption: {missing_mstv:?} (add an entry in \
     src/engine/version/mstv.rs, or an exemption in \
     tests/registry_agreement.rs::EXEMPTIONS)"
  );
}

#[test]
fn test_mstv_entries_correspond_to_a_declared_binary() {
  let declared = declared_binaries();

  // A declared binary can resolve to an MSTV entry under a different
  // canonical name than the one the surface wrote (e.g. rust.rs declares
  // "clippy-driver", the MSTV entry is keyed "clippy") — so match by
  // resolving each declared binary through `get_tool_mstv_entry` rather than
  // comparing names directly, exactly as production lookups do.
  let reachable_mstv_binaries: BTreeSet<&'static str> = declared
    .iter()
    .filter_map(|b| mstv::get_tool_mstv_entry(b))
    .map(|entry| entry.binary)
    .collect();

  let mut orphans = Vec::new();
  for entry in mstv::all_mstv_entries() {
    if is_exempt(entry.binary) {
      continue;
    }
    if !reachable_mstv_binaries.contains(entry.binary) {
      orphans.push(entry.binary);
    }
  }

  assert!(
    orphans.is_empty(),
    "TOOL_MSTV_REGISTRY entries with no surface declaring a binary that \
     resolves to them, and no exemption: {orphans:?} (this is the class of \
     bug issue #276 was filed over -- see the ktfmt removal in #273/#268; \
     either wire the tool into a surface's tool_info, or add an exemption \
     in tests/registry_agreement.rs::EXEMPTIONS)"
  );
}
