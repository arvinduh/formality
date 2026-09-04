# 5. `extra_args` flags that alter a tool's exit-code contract

- **Status**: Accepted
- **Decided in**: [#173](https://github.com/arvinduh/formality/issues/173), spun
  off from the QA review of
  [#155](https://github.com/arvinduh/formality/issues/155) / PR #170.

## Context

Every surface appends the user's `[lang.<name>] extra_args` _after_ `fml`'s own
flags, so a user-supplied value wins. That is the point of the setting, and for
almost every flag it is exactly right.

A small number of flags are different: they change what a non-zero exit code
_means_. Each surface picks a classifier (`classify_all_nonzero_as_error` or
`classify_exit_one_as_violation`) statically, at the call site, from the tool's
exit-code contract _as `fml` invokes it_. An `extra_args` entry that
reintroduces a "ran fine, and found/changed something" exit code makes that
static choice wrong, and the result is a lint finding rendered as
`[ERR] Execution error` with process exit 2.

**The verified instance is python.** Reproduced against `ruff 0.16.4`:

```console
ruff check --select I --fix a.py                      # exit 0, "All checks passed!"
ruff check --select I --fix a.py --extend-select F    # exit 1, F821 Undefined name
```

`ruff` accepts `--select` and `--extend-select` together, and
`src/surfaces/python.rs` classifies every non-zero exit on that import pass as
`ExecutionError`. That is #173's failure mode, reproducible today, and it is
**not** guarded — see [Consequences](#consequences).

**java behaves the same way**, verified against the pinned
`google-java-format@2.3.0` (upstream 1.35.0):

```console
google-java-format --replace A.java                          # exit 0 (file rewritten)
google-java-format --replace B.java --set-exit-if-changed    # exit 1 (file rewritten)
google-java-format --replace C.java --set-exit-if-changed    # exit 0 (already formatted)
```

The flag is not confined to `--dry-run`: paired with `--replace` it still
rewrites the file and then reports "I changed something" as exit 1, which
`classify_all_nonzero_as_error` renders as a failed format.

**javascript is the case where the classifier cannot be subverted this way**,
and it is worth recording why, because it looks like an instance and is not.
`fml fmt` always passes `--linter-enabled=false` itself, and biome rejects a
duplicated flag before parsing its value — reproduced against the pinned
`@biomejs/biome@2.5.10`:

```console
$ biome check --write --linter-enabled=false --linter-enabled=true b.ts
Error: argument `--linter-enabled` cannot be used multiple times in this context   # exit 1
```

No spelling in `extra_args` ever re-enabled the linter, so no lint finding was
ever misclassified on that path. What users got was an accurate but opaque
`[ERR]` naming neither `extra_args` nor the flag.

Three options were considered:

1. Document the hazard, guard nothing.
2. Document the hazard, and guard the flags `fml` itself passes in the same
   argv.
3. Re-derive each surface's classifier from the final argv, rather than choosing
   it statically at the call site.

## Decision

**Option 3 is rejected: `fml` keeps a static, per-call-site classifier and does
not re-derive it from the final argv.**

This is the cross-surface decision this ADR exists to record, because it is the
one that will otherwise be re-litigated every time someone hits an `[ERR]` that
should have been a `[FAIL]`. Re-deriving the classifier means `fml` would have
to model each tool's flag-to-exit-code semantics well enough to re-decide the
meaning of every exit code per invocation: which flags exist, which take values,
which of them move the boundary between "found something" and "could not run",
and how all of that shifts across the tool's releases. That is a permanent
version-tracking obligation against a dozen third-party CLIs — strictly larger
than the enumeration problem
[#171](https://github.com/arvinduh/formality/issues/171) already documents,
because a blocklist only has to _name_ flags whereas a classifier has to
_interpret_ them. What it buys off is a confusing label on an already-visible
failure: not data loss, not a silent pass.

**Option 2 is adopted as the narrow, cheap half of that trade.** A surface may
refuse, with an actionable diagnostic, when `extra_args` sets a flag _that same
surface passes in that same argv_. The bar is deliberately strict: it needs no
knowledge of the tool's flag vocabulary, because the surface is comparing
against its own code, so it cannot go stale as a tool adds flags — exactly the
property a blocklist lacks. **A surface author adding a new guard must clear
that same bar.** A flag that merely _happens_ to alter the contract
(`--extend-select`, `--set-exit-if-changed`) is documented, never guarded.

## Consequences

- **Coverage is partial, and partial in an uncomfortable direction.** Option 2's
  bar selects for flags `fml` passes, not for flags that actually cause the
  misclassification — and on current evidence those sets do not overlap at all.
  Both verified instances of the hazard (`--extend-select`,
  `--set-exit-if-changed`) are unguarded, documented only, in
  [language-surfaces.md](../language-surfaces.md#extra_args-and-exit-code-contracts).
  The guard rule is not what protects users from this class of bug; option 3
  would have been, and it was rejected on cost. State that plainly rather than
  letting the guard imply coverage it does not have.
  [#208](https://github.com/arvinduh/formality/issues/208) tracks the python
  case.
- **Exactly one flag clears the bar today**: biome's `--linter-enabled` on the
  javascript format path, detected by `extra_args_set_flag` in
  `src/surfaces/tooling.rs`. What that guard buys is replacing an opaque tool
  error with an explanation naming the flag, `extra_args`, and `fml lint` as the
  way out. It is a **diagnostic-quality improvement, not a misclassification
  fix**: per the reproduction above the linter was never actually re-enabled,
  and the `[ERR]` it replaces was correct about biome having failed.
- **Refusing is a real behavior change** for anyone who had `--linter-enabled`
  in `extra_args`, but it breaks nothing that worked, because neither spelling
  ever worked — `=true` and `=false` alike were rejected by biome as a duplicate
  flag. The change is that the diagnostic now explains itself.
- **Revisit option 3** only if the documented, unguarded cases are reported as
  actually biting users, or if a cheaper approximation appears — for example a
  surface discriminating on the tool's output shape (a lint finding and an
  `E999` do not look alike on stdout) instead of on its flags. The rejection
  here is on cost, not on principle.
