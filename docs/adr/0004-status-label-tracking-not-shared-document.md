# 0004 — Per-issue `status:*` labels instead of a shared hand-edited tracking document

> `#N` citations below predate the 2026-08-26 repo recreation and no longer
> resolve — see
> [`docs/INDEX.md`](../INDEX.md#note-on-pre-recreation-issuepr-numbers).

**Status:** Accepted **Decided in:** repo-original design (per-issue `status:*`
labels, `.agents/orchestrate.md` §11), reaffirmed and the last remnant of the
alternative removed via PR `#167` (closing tracking issue `#134`).

## Context

`fml`'s multi-agent orchestration process needs some way to represent "what's
ready, blocked, in review" so agents can find unclaimed work. Two designs were
live in this repo's history: per-issue `status:*` labels (the current and
now-only approach), and — for a period — _also_ a pinned master tracking issue
that regenerated a summary of those labels for humans skimming on GitHub's web
UI.

The pinned tracking issue was removed on 2026-08-24 (PR `#167`). Its
regeneration was manual, not automated, so it went stale between sessions with
nothing forcing a refresh. That made it a second, cacheable source of truth that
could — and did — silently disagree with the real one (the labels themselves).
The same PR also folded in a related finding: two issues (`#128`, `#55`) had
stale `status:blocked` labels whose named `Blocked-by` target had already
closed, with nothing catching the drift — the general version of the same
staleness problem, just narrower in blast radius.

## Decision

State lives _only_ on per-issue `status:*` labels.
`gh issue list --label status:ready` (or equivalent) is the sole source of truth
for what's available to work on — never a cached summary, and no pinned/tracking
issue regenerating one. `.agents/orchestrate.md` §11 documents the full labeling
convention (topical label + exactly one `status:*` label, `Blocked-by: #N` in
the body for blocked issues, `Spun off from #N` for spinoffs).

## Rationale

GitHub's issue-update API has no optimistic concurrency (no ETag/If-Match) — two
agents editing the same shared document around the same time is a real, silent
lost-update race. `status:*` labels on each _individual_ issue are narrow,
low-contention writes: two sessions touching different issues' labels can't
collide, and there's no aggregate snapshot to fall out of sync in the first
place. This is the same reasoning `.agents/orchestrate.md` §1.5 applies to the
dispatch race itself (claim-then-verify on `status:in-progress` +
self-assignment) — narrow per-issue writes over any shared aggregate.

## Consequences

- Nothing pushes a stale `Blocked-by` reference to correct itself — per §11,
  before trusting a `status:blocked` label, check whether its named blocker(s)
  are actually still closed, and unblock explicitly (remove `status:blocked`,
  add the next status, leave a comment explaining why) rather than leaving a
  label that's lying about real state. This issue's own `#131` history is a live
  example: it carried a stale `status:blocked` until a worker checked
  `#122`/`#128` and flipped it, per the comment on this issue.
- No single GitHub page gives a human a one-glance snapshot of repo state
  anymore — `gh issue list --label status:ready` (or the equivalent search in
  the web UI) is the only view, not a substitute human-readable digest.
- Any future proposal to reintroduce an aggregate summary needs to solve the
  staleness problem this ADR removed, not just recreate it — e.g. a summary
  that's mechanically regenerated on every label change rather than by hand, or
  that's clearly marked as a point-in-time snapshot rather than presented as
  live state.
