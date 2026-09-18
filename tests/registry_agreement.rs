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
//!      canonicalisation, or is exempt from one or both sides per
//!      `EXEMPTIONS`, with a reason;
//!   2. every `TOOL_MSTV_REGISTRY` entry corresponds to some surface-declared
//!      binary (the direction that would have caught `ktfmt`), or is exempt.
//!
//! Deliberately not checked: `ALL_CHAINS` entries with no surface (e.g.
//! `tinymist`) pairing back to a declared binary — issue #276 scopes the
//! keyset test to the two directions above, not a third. (Filed separately,
//! not part of this PR.)

use fml::config::ResolvedLangConfig;
use fml::engine::version::mstv;
use fml::surfaces::{all_surfaces, tooling};
use std::collections::BTreeSet;

/// Which side(s) of the "has an `ALL_CHAINS` row and a `TOOL_MSTV_REGISTRY`
/// entry" pairing a binary is exempt from. Exemptions are one-sided far more
/// often than not — a tool missing its install chain because it ships with
/// a parent toolchain is a completely different fact from a tool missing
/// its MSTV floor because nobody has picked one yet — so this is not a
/// single "exempt or not" flag; each `EXEMPTIONS` row states exactly which
/// side(s) it waives, and the test enforces the other side normally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExemptSide {
  /// Not required to have an `ALL_CHAINS` row; still must have a
  /// `TOOL_MSTV_REGISTRY` entry.
  ChainOnly,
  /// Not required to have a `TOOL_MSTV_REGISTRY` entry; still must have an
  /// `ALL_CHAINS` row.
  MstvOnly,
  /// Not required to have either.
  Both,
}

impl ExemptSide {
  fn exempts_chain(self) -> bool {
    matches!(self, Self::ChainOnly | Self::Both)
  }

  fn exempts_mstv(self) -> bool {
    matches!(self, Self::MstvOnly | Self::Both)
  }
}

/// Binaries exempt from one or both sides of the "must have an `ALL_CHAINS`
/// row and a `TOOL_MSTV_REGISTRY` entry" rule, with a reason each. Per issue
/// #276, exemptions must be explicit here, not silent gaps in the test —
/// and, per QA on the first version of this file, the side each exemption
/// actually waives must match what the test enforces, not just what the
/// comment claims.
const EXEMPTIONS: &[(&str, ExemptSide, &str)] = &[
  (
    "cargo",
    ExemptSide::Both,
    "ships with rustup, not a separately installed/versioned tool; no \
     install chain or MSTV floor applies",
  ),
  (
    "gofmt",
    ExemptSide::ChainOnly,
    "ships with the Go toolchain, so it has no standalone ALL_CHAINS \
     install-chain row of its own; it does carry a real MSTV floor \
     (MSTV_GOFMT / a TOOL_MSTV_REGISTRY entry), so only the ALL_CHAINS \
     side is exempt — the MSTV side is still enforced",
  ),
  (
    "google-java-format",
    ExemptSide::MstvOnly,
    "has an ALL_CHAINS install-chain row (a real gap on the MSTV side \
     would be masked if this exemption covered ALL_CHAINS too); no MSTV \
     floor is enforced today, and deciding one is a separate call outside \
     #276's scope, so only the MSTV side is exempt",
  ),
];

fn exemption(binary: &str) -> Option<ExemptSide> {
  EXEMPTIONS
    .iter()
    .find(|(b, ..)| *b == binary)
    .map(|(_, side, _)| *side)
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
    let side = exemption(binary);
    let exempt_chain = side.is_some_and(ExemptSide::exempts_chain);
    let exempt_mstv = side.is_some_and(ExemptSide::exempts_mstv);

    if !exempt_chain && tooling::install_chain_for(binary).is_none() {
      missing_chain.push(binary);
    }
    if !exempt_mstv && mstv::get_tool_mstv_entry(binary).is_none() {
      missing_mstv.push(binary);
    }
  }

  assert!(
    missing_chain.is_empty(),
    "surface-declared binaries with no ALL_CHAINS row and no matching \
     exemption: {missing_chain:?} (add a chain row in \
     src/surfaces/tooling.rs, or a ChainOnly/Both exemption in \
     tests/registry_agreement.rs::EXEMPTIONS)"
  );
  assert!(
    missing_mstv.is_empty(),
    "surface-declared binaries with no TOOL_MSTV_REGISTRY entry and no \
     matching exemption: {missing_mstv:?} (add an entry in \
     src/engine/version/mstv.rs, or a MstvOnly/Both exemption in \
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
    // Any exemption at all (not just an MstvOnly/Both one) skips this
    // check: EXEMPTIONS documents that binary as a known special case
    // already, and none of today's rows both carry an MSTV entry *and*
    // fail to resolve back to a declared binary (each has been verified
    // to resolve normally, or to have no MSTV entry at all) — this guard
    // only matters if that ever changes.
    if exemption(entry.binary).is_some() {
      continue;
    }
    if !reachable_mstv_binaries.contains(entry.binary) {
      orphans.push(entry.binary);
    }
  }

  assert!(
    orphans.is_empty(),
    "TOOL_MSTV_REGISTRY entries with no surface declaring a binary that \
     resolves to them, and no matching exemption: {orphans:?} (this is the \
     class of bug issue #276 was filed over -- see the ktfmt removal in \
     #273/#268; either wire the tool into a surface's tool_info, or add a \
     MstvOnly/Both exemption in tests/registry_agreement.rs::EXEMPTIONS)"
  );
}
