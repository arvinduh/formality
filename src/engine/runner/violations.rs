//! Reading each tool's own "how much is left, and how much of it can I still
//! fix" signal out of a [`SurfaceStatus::ViolationsFound`] message (#119).
//!
//! `fml fix` used to render every failing surface as a bare `Violations
//! found`, which reads identically whether the fix passes failed to apply
//! fixes they should have applied or applied everything possible and what
//! remains is not mechanically fixable by any tool in the chain. A correct
//! run was indistinguishable from a broken one.
//!
//! ## What is parsed, and what is deliberately not
//!
//! Only signals a tool *emits itself* are read. There is no rule list
//! mapping, say, `MD033` to "unfixable" — that would be `fml` guessing on
//! the tool's behalf and would go stale the moment a tool learns a new fix
//! (#254 is exactly that happening to `MD036`). A tool that exposes no such
//! signal produces [`None`] here, and the caller reports the surface without
//! a fixability claim rather than inventing one.
//!
//! Two tools expose one today:
//!
//! | tool | count | fixability |
//! | --- | --- | --- |
//! | `ruff` | `Found N errors.` / `Found N errors (F fixed, R remaining).` | ``[*] K fixable with the `--fix` option.`` |
//! | `markdownlint-cli2` | `N issues in M files` | `Attempted: N fixes in M files` |
//!
//! Everything else — `prettier`, `eslint`, `gofmt`, the shell-shim path in
//! the golden tests — falls through to [`None`] and renders exactly as it
//! did before this module existed.
//!
//! ## Why the counts are not summed into a "N fixed" figure
//!
//! They are not commensurable. `Attempted: N fixes in M files` counts fixes
//! *applied*, not violations *resolved* — one fix can clear several, and an
//! attempt is not necessarily a success — while `ruff` marks individual
//! diagnostics fixable. Adding them would invent a unit neither tool
//! provides. Per the owner decision on #119 (2026-09-17) this module reports
//! only what remains; the run-level total is the sum of the per-surface
//! numbers rendered above it, never an independent tally.
//!
//! [`SurfaceStatus::ViolationsFound`]: crate::surfaces::SurfaceStatus::ViolationsFound

/// What one surface still reports after every pass a [`Plan`] ran.
///
/// [`Plan`]: super::Plan
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViolationTally {
  /// Violations the tool still reports. Always non-zero: a tally that would
  /// read `0 violations` on a `[FAIL]` row is not produced at all (see
  /// [`tally`]).
  pub remaining: usize,
  /// How many of `remaining` the tool says it could still fix by itself.
  ///
  /// [`None`] means the tool exposed no fixability signal — *not* zero. The
  /// caller must render the count without a fixability claim in that case.
  pub auto_fixable: Option<usize>,
}

/// Whether this surface's linter was driven to completion in fixing mode,
/// which is what lets `markdownlint-cli2`'s count be read as a count of
/// things it *could not* fix.
///
/// The evidence has to be carried because the signal and the count are
/// emitted by *different invocations*. `fml fix` runs `markdownlint-cli2
/// --fix` (which prints `Attempted: N fixes in M files`), then the format
/// pass, then a check-only re-lint whose verdict supersedes the fix pass's —
/// and that re-lint runs *without* `--fix`, so it never prints an
/// `Attempted:` line. Nothing survives to the row unless it is carried.
///
/// Two things establish it, and both are facts rather than guesses:
///
/// 1. **The tool said so.** An `Attempted:` line in any of this surface's
///    pass messages is markdownlint-cli2 stating it ran its fixer.
/// 2. **`fml` asked for it.** markdownlint-cli2 prints the `Attempted:` line
///    only when it attempted at least one fix — a `--fix` run with nothing
///    to fix prints nothing at all (verified against v0.23.2). Reading that
///    silence as "no signal" would make the row say `14 violations` on the
///    run that fixed something and `14 violations, 0 auto-fixable` on the
///    next, which is the run-to-run instability #119 exists to remove. So a
///    plan that actually drove this surface's linter in fixing mode counts
///    too: that is `fml` reporting its own invocation, not inferring the
///    tool's capabilities.
///
/// What is still never inferred is *which violations* a tool can fix. This
/// flag only ever says the fixer finished; the count of what it left behind
/// comes from the tool.
///
/// `false` is the safe default: it only ever downgrades a surface to "count
/// reported, no fixability claim".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FixerEvidence {
  /// The surface's linter ran its own fixer to completion.
  pub fixer_completed: bool,
}

impl FixerEvidence {
  /// Evidence of kind 1: read from a pass's own output.
  pub fn from_message(message: &str) -> Self {
    Self {
      fixer_completed: message
        .lines()
        .any(|l| parse_markdownlint_attempted(l.trim()).is_some()),
    }
  }

  /// Evidence of kind 2: read from the invocation `fml` made.
  ///
  /// `lint_fix_ran` is the caller's answer to "did a writing plan run the
  /// lint pass against a surface that takes `--fix`" — see
  /// [`Runner::run`](super::Runner::run), which is the only place that can
  /// answer it.
  pub const fn from_invocation(lint_fix_ran: bool) -> Self {
    Self {
      fixer_completed: lint_fix_ran,
    }
  }

  /// Folds in another pass's evidence. Evidence only ever accumulates: a
  /// later pass that says nothing cannot unsay what an earlier one proved.
  pub const fn merge(self, other: Self) -> Self {
    Self {
      fixer_completed: self.fixer_completed || other.fixer_completed,
    }
  }
}

/// One tool's contribution to a surface's tally.
#[derive(Debug, Clone, Copy)]
struct Group {
  remaining: usize,
  auto_fixable: Option<usize>,
}

/// Reads `message` — a `ViolationsFound` message, possibly the concatenation
/// of several passes' messages — into a tally, or [`None`] when no tool in it
/// exposed a count.
///
/// `evidence` carries what earlier passes proved about tools that report
/// their fix attempts separately from their findings; pass
/// [`FixerEvidence::default`] when there is none.
///
/// Returning [`None`] is the honest outcome and is not rare: it covers every
/// tool with no count signal, and it is what keeps a surface `fml` cannot
/// measure rendering as it always has.
pub(super) fn tally(
  message: &str,
  evidence: FixerEvidence,
) -> Option<ViolationTally> {
  let groups = parse_groups(message, evidence);
  if groups.is_empty() {
    return None;
  }

  let remaining: usize = groups.iter().map(|g| g.remaining).sum();
  if remaining == 0 {
    // The status says violations were found, so a `0 violations` row would
    // be self-contradicting. Fall back to the unmeasured wording.
    return None;
  }

  // Fixability is known for the surface only if it is known for *every*
  // tool that contributed to the count — a partial figure would read as
  // complete.
  //
  // Each group's figure is clamped to its own count, and the sum clamped
  // again to the surface's. Nothing should ever exceed it — a tool saying
  // more of its findings are fixable than it found would be nonsense — but
  // this is the number a reader cannot check by eye, and `7 violations, 9
  // auto-fixable` is the kind of line that costs the whole summary its
  // credibility. The clamp is unconditional so no future parser can put one
  // on screen.
  let auto_fixable = groups
    .iter()
    .map(|g| g.auto_fixable.map(|k| k.min(g.remaining)))
    .try_fold(0usize, |acc, f| f.map(|k| acc + k))
    .map(|k| k.min(remaining));

  Some(ViolationTally {
    remaining,
    auto_fixable,
  })
}

/// Splits `message` into per-tool groups.
///
/// One message can hold more than one group. [`combine_pass_results`] joins
/// two passes' `ViolationsFound` messages with a newline, and python's
/// widened-selection path puts a whole second `ruff` invocation's output
/// into one message, so *two `ruff` groups in one message is a real shape*,
/// not a hypothetical.
///
/// That is why a group takes its fixability from the hint **that follows
/// it**, not from the first hint anywhere in the message. `ruff` prints
/// ``[*] K fixable …`` directly under the `Found …` line it belongs to, so
/// the lines between one group's opener and the next opener are that
/// group's own. Resolving the hint once for the whole message attributed one
/// invocation's `K` to every group in it, which could render more
/// auto-fixable violations than there were violations.
///
/// [`combine_pass_results`]: super::combine_pass_results
fn parse_groups(message: &str, evidence: FixerEvidence) -> Vec<Group> {
  let evidence = evidence.merge(FixerEvidence::from_message(message));
  let lines: Vec<&str> = message.lines().map(str::trim).collect();

  let mut groups = Vec::new();
  for (i, line) in lines.iter().enumerate() {
    if let Some(remaining) = parse_ruff_found(line) {
      groups.push(Group {
        remaining,
        // `ruff` emits the ``[*] K fixable`` hint whenever any diagnostic it
        // just printed carries the `[*]` marker, so its *absence* below a
        // `Found …` line is ruff's own statement that none of them do — not
        // a missing signal. This is the one place absence is read as zero,
        // and only for the tool that guarantees it.
        auto_fixable: Some(ruff_fixable_after(&lines, i).unwrap_or(0)),
      });
    } else if let Some(remaining) = parse_markdownlint_summary(line) {
      groups.push(Group {
        remaining,
        // markdownlint-cli2 marks no individual violation fixable. What it
        // does say is whether it ran its fixer: once `Attempted:` proves the
        // fixer went through, everything it still reports is by construction
        // something it could not fix. With no such line the count stands on
        // its own and the fixability claim is withheld.
        auto_fixable: evidence.fixer_completed.then_some(0),
      });
    }
  }
  groups
}

/// The ``[*] K fixable …`` hint belonging to the group opened at
/// `lines[start]`: the first one between that line and the next line that
/// opens a group, or [`None`] if that stretch has none.
fn ruff_fixable_after(lines: &[&str], start: usize) -> Option<usize> {
  lines
    .iter()
    .skip(start + 1)
    .take_while(|l| !opens_group(l))
    .find_map(|l| parse_ruff_fixable_hint(l))
}

/// Whether `line` opens a group, i.e. is one of the two count lines
/// [`parse_groups`] recognizes.
fn opens_group(line: &str) -> bool {
  parse_ruff_found(line).is_some() || parse_markdownlint_summary(line).is_some()
}

/// ``Found 4 errors.`` → `4`; ``Found 4 errors (2 fixed, 2 remaining).`` →
/// `2`; ``Found 1 error.`` → `1`.
///
/// The parenthesised form is what `ruff check --fix` prints, and *remaining*
/// is the number this module is about — `4` there counts violations that no
/// longer exist.
fn parse_ruff_found(line: &str) -> Option<usize> {
  let rest = line.strip_prefix("Found ")?;
  let (count, rest) = split_leading_usize(rest)?;
  let rest = rest
    .strip_prefix(" errors")
    .or_else(|| rest.strip_prefix(" error"))?;

  if let Some(detail) = rest.strip_prefix(" (") {
    let remaining = detail.split(", ").find_map(|part| {
      let (n, tail) = split_leading_usize(part)?;
      tail.starts_with(" remaining").then_some(n)
    })?;
    return Some(remaining);
  }
  rest.starts_with('.').then_some(count)
}

/// ``[*] 2 fixable with the `--fix` option.`` → `2`.
///
/// Deliberately anchored on `fixable with` rather than the whole sentence:
/// ruff varies the tail (`--fix`, `--fix-only`), and `No fixes available (1
/// hidden fix can be enabled with the --unsafe-fixes option).` is *not*
/// matched — an unsafe fix `fml` does not ask for is not auto-fixable here.
fn parse_ruff_fixable_hint(line: &str) -> Option<usize> {
  let rest = line.strip_prefix("[*] ")?;
  let (count, rest) = split_leading_usize(rest)?;
  rest.starts_with(" fixable with").then_some(count)
}

/// ``2 issues in 1 file`` → `2`; ``Summary: 2 issues in 1 file`` → `2`.
///
/// Both spellings are accepted: `markdown`'s surface moves the `Summary:`
/// line to the tail as a bare count, but this module is not the place to
/// depend on that having happened.
fn parse_markdownlint_summary(line: &str) -> Option<usize> {
  let line = line.strip_prefix("Summary: ").unwrap_or(line);
  let (count, rest) = split_leading_usize(line)?;
  let rest = rest
    .strip_prefix(" issues in ")
    .or_else(|| rest.strip_prefix(" issue in "))?;
  split_leading_usize(rest).map(|_| count)
}

/// ``Attempted: 2 fixes in 1 file`` → `2`. The number is unused today — its
/// presence is the signal — but parsing it is what keeps the match anchored
/// to markdownlint's real line rather than any text starting `Attempted:`.
fn parse_markdownlint_attempted(line: &str) -> Option<usize> {
  let rest = line.strip_prefix("Attempted: ")?;
  let (count, rest) = split_leading_usize(rest)?;
  let rest = rest
    .strip_prefix(" fixes in ")
    .or_else(|| rest.strip_prefix(" fix in "))?;
  split_leading_usize(rest).map(|_| count)
}

/// Splits a leading run of ASCII digits off `s`, returning it and the rest.
fn split_leading_usize(s: &str) -> Option<(usize, &str)> {
  let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
  if end == 0 {
    return None;
  }
  s[..end].parse().ok().map(|n| (n, &s[end..]))
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  /// Verbatim `ruff 0.15.8` output over a file with two fixable and two
  /// unfixable findings, trimmed to the summary lines the parser reads.
  const RUFF_MIXED: &str = "\
F401 [*] `os` imported but unused
B904 Within an `except` clause, raise exceptions with `raise ... from err`
Found 4 errors.
[*] 2 fixable with the `--fix` option.";

  /// Verbatim `ruff check --fix` output: the count that matters is the
  /// parenthesised *remaining*, not the leading total.
  const RUFF_AFTER_FIX: &str = "\
B904 Within an `except` clause, raise exceptions with `raise ... from err`
Found 4 errors (2 fixed, 2 remaining).";

  /// `markdownlint-cli2 --fix`, after `filter_markdownlint_noise` has moved
  /// the `Summary:` line to the tail as a bare count.
  const MARKDOWNLINT_FIXED: &str = "\
Attempted: 2 fixes in 1 file
README.md:3:1 error MD033/no-inline-html Inline HTML [Element: p]
README.md:5 error MD036/no-emphasis-as-heading Emphasis used instead of a heading
2 issues in 1 file";

  #[test]
  fn ruff_reports_count_and_its_own_fixable_hint() {
    let t = tally(RUFF_MIXED, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 4);
    assert_eq!(t.auto_fixable, Some(2));
  }

  #[test]
  fn ruff_after_fix_counts_remaining_not_the_original_total() {
    let t = tally(RUFF_AFTER_FIX, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 2);
    // No `[*]` hint alongside the `Found` line: ruff says none of what is
    // left is fixable, which is the whole point of #119.
    assert_eq!(t.auto_fixable, Some(0));
  }

  #[test]
  fn ruff_singular_error_parses() {
    let t = tally("Found 1 error.", FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 1);
    assert_eq!(t.auto_fixable, Some(0));
  }

  #[test]
  fn ruff_unsafe_hidden_fixes_are_not_counted_as_auto_fixable() {
    let msg = "Found 1 error.\nNo fixes available (1 hidden fix can be \
               enabled with the --unsafe-fixes option).";
    let t = tally(msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 1);
    assert_eq!(t.auto_fixable, Some(0));
  }

  #[test]
  fn markdownlint_with_attempted_line_reports_zero_fixable() {
    let t = tally(MARKDOWNLINT_FIXED, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 2);
    assert_eq!(t.auto_fixable, Some(0));
  }

  #[test]
  fn markdownlint_without_attempted_line_withholds_the_claim() {
    let msg = "README.md:5 error MD036/no-emphasis-as-heading\n\
               1 issue in 1 file";
    let t = tally(msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 1);
    assert_eq!(t.auto_fixable, None);
  }

  #[test]
  fn a_fix_run_that_attempted_nothing_still_carries_evidence() {
    // markdownlint-cli2 prints no `Attempted:` line when it had nothing to
    // fix, so on the second `fml fix` of an unchanged tree the message alone
    // proves nothing. The invocation still does, and the row must not lose
    // its `0 auto-fixable` between two identical runs.
    let msg = "README.md:5 error MD036/no-emphasis-as-heading\n\
               1 issue in 1 file";
    assert_eq!(
      tally(msg, FixerEvidence::default()).unwrap().auto_fixable,
      None
    );
    let t = tally(msg, FixerEvidence::from_invocation(true)).unwrap();
    assert_eq!(t.auto_fixable, Some(0));
    // A read-only plan (`fml lint`, `fml fix --check`) drove no fixer, so
    // the claim stays withheld.
    assert_eq!(
      tally(msg, FixerEvidence::from_invocation(false))
        .unwrap()
        .auto_fixable,
      None
    );
  }

  #[test]
  fn invocation_evidence_does_not_touch_a_tool_with_its_own_signal() {
    // ruff answers for itself; nothing about how `fml` invoked it can
    // override the hint it printed.
    let t = tally(RUFF_MIXED, FixerEvidence::from_invocation(true)).unwrap();
    assert_eq!(t.auto_fixable, Some(2));
  }

  #[test]
  fn carried_evidence_restores_the_claim_a_recheck_dropped() {
    // The shape `fml fix` actually produces: the fix pass printed
    // `Attempted:`, the check-only recheck that supersedes it did not.
    let evidence = FixerEvidence::from_message(MARKDOWNLINT_FIXED);
    assert!(evidence.fixer_completed);
    let recheck = "README.md:5 error MD036/no-emphasis-as-heading\n\
                   1 issue in 1 file";
    let t = tally(recheck, evidence).unwrap();
    assert_eq!(t.remaining, 1);
    assert_eq!(t.auto_fixable, Some(0));
  }

  #[test]
  fn unrecognized_tool_output_produces_no_tally() {
    assert!(tally("", FixerEvidence::default()).is_none());
    assert!(
      tally("Command failed with exit code 1", FixerEvidence::default())
        .is_none()
    );
    assert!(
      tally(
        "src/x.js: error\n  2:1  Delete `;`",
        FixerEvidence::default()
      )
      .is_none()
    );
  }

  #[test]
  fn a_zero_count_is_not_rendered_as_a_tally() {
    // `[FAIL]` plus `0 violations` would contradict itself.
    assert!(tally("Found 0 errors.", FixerEvidence::default()).is_none());
  }

  #[test]
  fn two_ruff_groups_attribute_each_hint_to_its_own_group() {
    // The realistic multi-group shape: one surface, two `ruff` invocations,
    // joined by `combine_pass_results`. The second has no hint under it, so
    // it contributes 0 — resolving one hint for the whole message credited
    // the first group's 2 to both and could report more auto-fixable
    // violations than there were violations at all.
    let msg = format!("{RUFF_MIXED}\n{RUFF_AFTER_FIX}");
    let t = tally(&msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 6, "4 from the first group, 2 from the second");
    assert_eq!(t.auto_fixable, Some(2), "the second group has no hint");
  }

  #[test]
  fn a_hint_never_attaches_to_a_group_above_it() {
    // A group with no hint of its own must not borrow the *next* group's.
    let msg = "Found 1 error.\nFound 9 errors.\n[*] 9 fixable with the \
               `--fix` option.";
    let t = tally(msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 10);
    assert_eq!(t.auto_fixable, Some(9));
  }

  #[test]
  fn auto_fixable_is_clamped_to_the_count_it_qualifies() {
    // Belt and braces on the one number a reader cannot check: whatever a
    // parser produces, the row can never say more is fixable than is left.
    let msg = "Found 1 error.\n[*] 6 fixable with the `--fix` option.";
    let t = tally(msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 1);
    assert_eq!(t.auto_fixable, Some(1));
  }

  #[test]
  fn a_mixed_tool_message_keeps_the_weakest_claim() {
    // No surface runs both `ruff` and `markdownlint-cli2` today, so this
    // shape is hypothetical — but the rule it pins is not: one unmeasured
    // half must not leave the other half reading as a whole claim.
    let msg = format!("{RUFF_MIXED}\n1 issue in 1 file");
    let t = tally(&msg, FixerEvidence::default()).unwrap();
    assert_eq!(t.remaining, 5);
    assert_eq!(t.auto_fixable, None);
  }

  #[test]
  fn evidence_merge_only_accumulates() {
    let yes = FixerEvidence {
      fixer_completed: true,
    };
    let no = FixerEvidence::default();
    assert!(yes.merge(no).fixer_completed);
    assert!(no.merge(yes).fixer_completed);
    assert!(!no.merge(no).fixer_completed);
  }

  #[test]
  fn near_miss_lines_do_not_match() {
    // Prose that happens to start the same way must not be read as a count.
    assert!(tally("Found 3 problems.", FixerEvidence::default()).is_none());
    assert!(
      tally("Attempted: 2 fixes in 1 file", FixerEvidence::default()).is_none(),
      "evidence alone is not a count"
    );
    assert!(
      tally("2 issues in the tracker", FixerEvidence::default()).is_none()
    );
  }
}
