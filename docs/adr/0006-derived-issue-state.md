# 0006 — Issue state derived from GitHub, not stored in `status:*` labels

**Status:** Accepted. Supersedes
[0004](0004-status-label-tracking-not-shared-document.md).

## Context

[0004](0004-status-label-tracking-not-shared-document.md) moved workflow state
off a hand-edited tracking issue and onto exactly one `status:*` label per issue
(`ready`, `blocked`, `design-phase`, `in-progress`, `in-review`). That removed
the shared document, but kept the same failure in smaller form: a label is a
stored copy of a fact GitHub already holds.

- `status:blocked` went stale whenever its `Blocked-by: #N` target closed;
  nothing flipped it back.
- `status:in-progress` and `status:in-review` restate the assignee and the PR's
  draft state, and drift when an agent stops without cleaning up.
- Claiming needed a label write plus an assignee write plus a read-back to catch
  a racing claim.

GitHub now records each of these natively: issue dependencies (blocked by),
assignees, and draft pull requests.

## Decision

State is derived, never stored. Labels carry only what GitHub cannot express:

| State   | Is                                                      |
| ------- | ------------------------------------------------------- |
| triage  | label `triage`: an unverified lead                      |
| design  | label `design`: needs a conversation with the user      |
| ready   | label `ready`, no assignee, no open blocker             |
| blocked | an open native blocker                                  |
| doing   | an assignee plus a draft PR                             |
| review  | the PR is marked ready for review                       |
| done    | the issue is closed by the merge (`Fixes #N` in the PR) |

Order between issues is expressed as native blockers, never as `Blocked-by:`
prose. The full protocol is the global `orchestrate` skill; this ADR records
that this repo follows it.

## Consequences

- A blocker closing unblocks the issue with no label edit, so the stale
  `status:blocked` case from 0004 cannot recur.
- Claiming an issue is self-assignment; the assignee is visible to every other
  session without a separate label.
- No single label query lists in-progress work. "Doing" and "review" come from
  the PR list (draft vs. ready), not from issue labels.
- Migrated 2026-10-01: `status:ready` became `ready` and `status:design-phase`
  became `design`, each `Blocked-by: #N` line a native blocker, stale premises
  `triage`, and the `status:*` labels were deleted. Ready issues were rewritten
  into Goal, Done, Files, Not with the original body kept in a `<details>`
  block.
