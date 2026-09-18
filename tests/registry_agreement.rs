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
//!
//! ## Scope: `tool_info` declarations, plus a small named extension
//!
//! The binary set this test checks is built from every surface's
//! `tool_info()` (`checked_binaries`, below) — that's the vocabulary the
//! surfaces themselves declare, and it's what `fml doctor` and `fml
//! fmt`/`fml lint` actually install and invoke. A tool spawned by code
//! outside any surface's `tool_info` is invisible to `checked_binaries` by
//! construction; issue #276 names one such case explicitly — `typst`,
//! invoked directly by the LSP diagnostics path
//! (`src/commands/lsp_diagnostics.rs`), in neither registry — and asks for
//! it to be on the exemption list. To make that concrete rather than a
//! statement nothing exercises, `EXTRA_CHECKED_BINARIES` below adds `typst`
//! to the checked set by name so `EXEMPTIONS` actually covers it; there is
//! no general extraction of "every binary any command ever spawns" here,
//! nor a corresponding requirement in #276 for one.
//!
//! Left uncovered by design, not silently: any other binary reachable only
//! through a command's own direct `create_tool_command`/`check_binary_exists`
//! calls rather than through a surface's `tool_info` or the
//! `EXTRA_CHECKED_BINARIES` list — for example `go`, used as a
//! [`fml::engine::version::mstv::VersionProbe::ViaBinary`] probe target for
//! `gofmt`/`goimports` rather than as a tool with its own `ToolInfo` entry.
//! `go` is a probe vehicle, not a fleet tool with an install chain or an
//! MSTV floor of its own, so it was never a candidate for either registry
//! and adding it to `EXTRA_CHECKED_BINARIES` would just require exempting
//! it from both sides for no benefit.

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
  (
    "typst",
    ExemptSide::Both,
    "invoked directly by the LSP diagnostics path \
     (src/commands/lsp_diagnostics.rs), not declared via any surface's \
     tool_info, and in neither registry — issue #276's own named example \
     of a tool with no chain row; see EXTRA_CHECKED_BINARIES for how it \
     reaches this test's checked set at all",
  ),
];

fn exemption(binary: &str) -> Option<ExemptSide> {
  EXEMPTIONS
    .iter()
    .find(|(b, ..)| *b == binary)
    .map(|(_, side, _)| *side)
}

/// Binaries invoked outside any surface's `tool_info` that this test still
/// checks by name, because issue #276 names them explicitly as needing an
/// exemption (see the module doc's "Scope" section for why the set stops
/// here rather than growing to cover every command-spawned binary).
const EXTRA_CHECKED_BINARIES: &[&str] = &["typst"];

/// Every binary this test holds `ALL_CHAINS`/`TOOL_MSTV_REGISTRY` to
/// agreement over: every binary a surface declares via `tool_info`, plus
/// `EXTRA_CHECKED_BINARIES`.
fn checked_binaries() -> BTreeSet<&'static str> {
  let mut binaries: BTreeSet<&'static str> = all_surfaces()
    .iter()
    .flat_map(|s| {
      let config = ResolvedLangConfig::new(s.name());
      s.tool_info(&config).into_iter().map(|t| t.binary)
    })
    .collect();
  binaries.extend(EXTRA_CHECKED_BINARIES);
  binaries
}

#[test]
fn test_checked_binaries_have_chain_and_mstv_rows() {
  let mut missing_chain = Vec::new();
  let mut missing_mstv = Vec::new();

  for binary in checked_binaries() {
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
    "checked binaries with no ALL_CHAINS row and no matching exemption: \
     {missing_chain:?} (add a chain row in src/surfaces/tooling.rs, or a \
     ChainOnly/Both exemption in tests/registry_agreement.rs::EXEMPTIONS)"
  );
  assert!(
    missing_mstv.is_empty(),
    "checked binaries with no TOOL_MSTV_REGISTRY entry and no matching \
     exemption: {missing_mstv:?} (add an entry in \
     src/engine/version/mstv.rs, or a MstvOnly/Both exemption in \
     tests/registry_agreement.rs::EXEMPTIONS)"
  );
}

#[test]
fn test_mstv_entries_correspond_to_a_checked_binary() {
  let checked = checked_binaries();

  // A checked binary can resolve to an MSTV entry under a different
  // canonical name than the one it's written as (e.g. rust.rs declares
  // "clippy-driver", the MSTV entry is keyed "clippy") — so match by
  // resolving each checked binary through `get_tool_mstv_entry` rather than
  // comparing names directly, exactly as production lookups do.
  let reachable_mstv_binaries: BTreeSet<&'static str> = checked
    .iter()
    .filter_map(|b| mstv::get_tool_mstv_entry(b))
    .map(|entry| entry.binary)
    .collect();

  let mut orphans = Vec::new();
  for entry in mstv::all_mstv_entries() {
    // Any exemption at all (not just an MstvOnly/Both one) skips this
    // check: EXEMPTIONS documents that binary as a known special case
    // already, and none of today's rows both carry an MSTV entry *and*
    // fail to resolve back to a checked binary (each has been verified to
    // resolve normally, or to have no MSTV entry at all) — this guard only
    // matters if that ever changes.
    if exemption(entry.binary).is_some() {
      continue;
    }
    if !reachable_mstv_binaries.contains(entry.binary) {
      orphans.push(entry.binary);
    }
  }

  assert!(
    orphans.is_empty(),
    "TOOL_MSTV_REGISTRY entries with no checked binary resolving to them, \
     and no matching exemption: {orphans:?} (this is the class of bug \
     issue #276 was filed over -- see the ktfmt removal in #273/#268; \
     either wire the tool into a surface's tool_info, or add a \
     MstvOnly/Both exemption in tests/registry_agreement.rs::EXEMPTIONS)"
  );
}

// --- #294: MSTV advice content must agree with ALL_CHAINS -----------------
//
// `ToolMstvEntry.advice` used to be a second, hand-maintained copy of the
// same package-manager commands `ALL_CHAINS` already owns (the same drift
// shape #264 fixed for `ToolInfo.install_hint`) -- most visibly, its
// `taplo` row led with `cargo binstall` after `TAPLO_CHAIN` was
// deliberately reordered npm-first. `MstvAdvice::Derived` (see
// `src/engine/version/mstv.rs`) now sources the install portion from
// `install_hint_for`, the same `ALL_CHAINS` renderer `ToolInfo` hints use;
// `MstvAdvice::Bespoke` remains only for a binary with no `ALL_CHAINS` row
// at all to derive from (`gofmt`, exempt on the chain side above).
//
// This reuses this file's own `EXEMPTIONS`/`ExemptSide` mechanism rather
// than adding a third one: a binary legitimately exempt from the chain
// side (`ExemptSide::ChainOnly`/`Both`) has no `ALL_CHAINS` row to derive
// from, so `Bespoke` is its only option and is not a violation; any other
// binary reaching for `Bespoke` is the #264/#294 drift bug reappearing.

/// Whether `entry` hand-writes upgrade advice (`MstvAdvice::Bespoke`) for a
/// binary that a chain-side exemption does not excuse from having a real
/// `ALL_CHAINS` row -- i.e. a binary that has (or should have) a chain row
/// to derive its install guidance from instead.
fn hardcodes_bespoke_advice_for_a_chain_backed_binary(
  entry: &mstv::ToolMstvEntry,
) -> bool {
  let chain_exempt =
    exemption(entry.binary).is_some_and(ExemptSide::exempts_chain);
  matches!(entry.advice, mstv::MstvAdvice::Bespoke(_)) && !chain_exempt
}

#[test]
fn test_no_mstv_entry_hardcodes_advice_for_a_chain_backed_binary() {
  let violations: Vec<&str> = mstv::all_mstv_entries()
    .iter()
    .filter(|entry| hardcodes_bespoke_advice_for_a_chain_backed_binary(entry))
    .map(|entry| entry.binary)
    .collect();

  assert!(
    violations.is_empty(),
    "MSTV entries with hand-written (MstvAdvice::Bespoke) upgrade advice \
     for a binary not exempt from the ALL_CHAINS side: {violations:?} -- \
     this is the #264/#294 drift bug reappearing. Use \
     MstvAdvice::Derived {{ upgrade_note: .. }} instead, or add a \
     ChainOnly/Both exemption in EXEMPTIONS if the binary genuinely has no \
     ALL_CHAINS row."
  );
}

/// Demonstrates the guard above actually catches a regression: a fabricated
/// entry that reverts a chain-backed binary (`taplo`, which has no chain
/// exemption) to hand-written `Bespoke` advice must be flagged.
#[test]
fn test_hardcoded_advice_detector_catches_a_regression() {
  let regression = mstv::ToolMstvEntry {
    binary: "taplo",
    min_version: None,
    probe: mstv::DEFAULT_VERSION_PROBE,
    advice: mstv::MstvAdvice::Bespoke(
      "Run 'cargo binstall taplo-cli' or 'npm install -g @taplo/cli'",
    ),
  };

  assert!(
    hardcodes_bespoke_advice_for_a_chain_backed_binary(&regression),
    "detector failed to catch hand-written Bespoke advice reintroduced on \
     'taplo', a chain-backed binary with no ALL_CHAINS exemption"
  );

  // Sanity check on the other side: `gofmt` is legitimately chain-exempt
  // (see EXEMPTIONS), so the same detector must not flag it.
  let gofmt_entry = mstv::get_tool_mstv_entry("gofmt")
    .expect("gofmt is a real TOOL_MSTV_REGISTRY entry");
  assert!(
    !hardcodes_bespoke_advice_for_a_chain_backed_binary(gofmt_entry),
    "detector incorrectly flagged 'gofmt', which is legitimately exempt \
     from the ALL_CHAINS side"
  );
}
