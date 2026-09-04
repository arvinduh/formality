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
`classify_exit_one_as_violation`) from the tool's exit-code contract _as `fml`
invokes it_. An `extra_args` entry that reintroduces a "ran fine, and
found/changed something" exit code makes that classifier wrong, and the result
is a lint finding rendered as `[ERR] Execution error` with process exit 2.

Known cases at the time of writing:

- **javascript** — `--linter-enabled=true` undoes the `--linter-enabled=false`
  that `fml fmt` passes to `biome check --write`. Verified: a file with `a == b`
  plus `debugger;` exits 1 under `biome check --write` and 0 with the linter
  disabled.
- **python** — `--extend-select F` on the `ruff check --select I --fix` import
  pass reintroduces exit 1 for unfixed violations.
- **java** — `--set-exit-if-changed` turns "reformatted a file" into a non-zero
  exit from `google-java-format --replace`.

Three options were considered: document the hazard; guard against known
contract-altering flags; or re-derive each surface's classifier from the final
argv rather than from the surface's static choice.

## Decision

**Document the hazard generally, and guard only the flags `fml` itself passes
explicitly.**

- The general rule is documented in
  [language-surfaces.md](../language-surfaces.md#extra_args-and-exit-code-contracts),
  which names the specific flags per surface and says what the user will see.
- A surface refuses, with an actionable diagnostic, when `extra_args` sets a
  flag that surface passes itself to pin its exit-code contract. Today that is
  exactly one flag: biome's `--linter-enabled` on the javascript format path.
  `extra_args_set_flag` in `src/surfaces/tooling.rs` is the shared detector.

**A surface author adding a new guard must clear the same bar**: the flag has to
be one `fml` passes in that same argv, so "the user is overriding us" is
unambiguous and does not depend on knowing the tool's full flag vocabulary. A
flag that merely _happens_ to alter the contract (`--extend-select`,
`--set-exit-if-changed`) is documented, never guarded.

## Consequences

- The guard cannot go stale in the way a general blocklist would: it is derived
  from what the surface's own code passes, not from an enumeration of a tool's
  flags that grows every release. This is the same open-ended-enumeration
  problem [#171](https://github.com/arvinduh/formality/issues/171) has, and the
  reason a broader blocklist was rejected.
- Coverage is deliberately partial. `--extend-select F` and
  `--set-exit-if-changed` still produce a misleading `[ERR]`; the docs say so
  rather than the tool pretending otherwise.
- Re-deriving the classifier from the final argv (option 3) was rejected as
  over-engineering: it means teaching `fml` each tool's flag-to-exit-code
  semantics well enough to re-decide per invocation, which is a much larger
  standing maintenance burden than the failure mode justifies. Revisit only if
  the documented cases are actually reported as biting users.
- Refusing is a real behavior change for anyone who had `--linter-enabled` in
  `extra_args`. Previously the flag was passed through: `=true` produced a
  misclassified `[ERR]` on a lint finding, and `=false` produced an opaque
  `[ERR]` because biome rejects the flag given twice. Both were already broken;
  the change is that the diagnostic now explains itself.
