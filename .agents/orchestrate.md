# formality orchestration process

This is the _process_ — how work on this repo gets planned, dispatched,
reviewed, and merged. It replaces the old git-ignored `.artifacts/PLAN.md`.
State (what's ready, blocked, in review) lives in GitHub issue `status:*` labels
and the pinned master tracking issue, not in this file — this file only changes
when the _process itself_ changes.

## How to use this file

One file, one path, no per-tool discovery convention to keep in sync:
**`.agents/orchestrate.md`.** If your harness auto-discovers skills from some
other directory, it won't find this automatically — that trade-off is
deliberate. Point any agent at it directly ("read `.agents/orchestrate.md`
before starting") or rely on `AGENTS.md`'s own pointer, since `AGENTS.md` itself
_is_ read automatically by both Antigravity and Claude Code.

- **Read this before dispatching or picking up any formality work.** If you're
  an agent that just landed on this repo cold, this file plus `AGENTS.md` is
  everything you need — don't re-derive the process from git history.
- **Finding current work:** `gh issue list --label status:ready` is the live
  source of truth. The pinned master tracking issue is a generated snapshot of
  the same query for humans skimming on the web — if the two ever disagree,
  trust the label query, not the tracking issue's cached text (see "Tracking
  issue" below for why).
- **Automation honesty check, right now:** the tracking-issue regeneration is
  **not** yet automated (no GitHub Action wired up) — it's a manual
  `gh issue list` + edit whenever an orchestrator touches it. Don't assume it's
  always fresh. Automating it (Action on `issues: [labeled, unlabeled, closed]`,
  with a `concurrency:` group) is future work, not done yet — update this
  paragraph the day it lands instead of leaving it stale.

## 1. Worktree isolation

Every worker subagent operates in its own isolated git worktree
(`git worktree add`), never the shared primary working directory. Workers never
concurrently mutate another agent's active branch. This has been violated before
(an agent working directly in the shared checkout, 2026-08-22) — caught, not
automatic; verify a worktree is actually being used, don't assume. This rule
also protects **orchestrator-vs-orchestrator** collisions, not just
orchestrator-vs-worker: never run two live orchestrator sessions against the
same physical local clone — worktrees all share one `.git/`, and two processes
doing concurrent `git worktree add`/push against it can hit real
`.git/index.lock` contention. Separate clones, not just separate worktrees, if
you're running more than one orchestrator at once.

## 1.5. Multiple concurrent orchestrators — the dispatch race

The `status:*` label design (§11, "Tracking issue") fixes the race on the
_tracking view_ — regenerating it is idempotent, so concurrent regenerations
can't diverge. **It does not fix the race on _dispatch_ itself.** Two
orchestrators can both query `status:ready`, both see the same unclaimed issue,
and both start work on it before either writes `status:in-progress` — a plain
label write has no compare-and-swap, so this is a real check-then-act race, just
with a narrower blast radius (one issue) than the old shared-document problem.

**Claim, then verify, before doing any real work:**

1. Flip the issue to `status:in-progress` **and** self-assign
   (`gh issue edit --add-label status:in-progress --add-assignee @me`).
2. Immediately read the issue back. If you're not the sole assignee, someone
   else's write landed first — abort, don't proceed, pick a different issue.
3. Only after that readback succeeds: create the worktree and dispatch.

This shrinks the race window to milliseconds and makes a collision loud (visible
on readback) instead of silent — it doesn't eliminate the window entirely;
GitHub gives no true atomic claim primitive for issues. The branch-name
convention (`feat/issue-N-...`) is a cheap backstop on top of this: if the
claim-and-verify step somehow still double-dispatches, the second worker's
`git push -u` on an already-existing remote branch fails loudly rather than
silently duplicating work.

**Review/merge ownership follows the same pattern:** whoever self-assigns a PR
for QA review owns merging it. An orchestrator that sees a PR already
`status:in-review` with a different assignee skips it — it does not also review
or merge.

## 2. Commits & presubmit

- Conventional Commits: `<type>(<scope>): <description> (Fixes #<issue>)`.
- One logical change per commit — never bundle unrelated cleanups.
- Before every commit: `cargo test --lib -q && cargo clippy -q`, dogfooded with
  the freshly built binary (`cargo run -q -- fmt`), never a stale global `fml`.
  This repo's own commit gate runs `fml sync --check` / `fml fmt` / `fml lint`
  on staged files — expect it to block a commit that fails its own dogfooding,
  and treat that as working as intended, not a bug to route around.

## 3. CI / branch-protection changes need the orchestrator, not a worker

A worker in an isolated worktree can't see repo branch-protection settings. Any
PR renaming a CI job, changing which workflow produces a check, or changing
trigger conditions must be checked against current required status checks by the
orchestrator before merge — a rename that looks fine in isolation can silently
block all future merges. Changing branch protection itself needs the user's
sign-off, always — an "ask first" item per `AGENTS.md`.

## 4. Maker-checker QA gate (required for non-trivial changes)

- A separate QA reviewer subagent audits the worker's finished diff _before_
  merge, for anything touching shared/core code or introducing new behavior.
  Pure one-liners and typo fixes don't need this ceremony.
- The reviewer didn't write the code and reviews skeptically: edge cases,
  version-compat behavior, does the diff actually satisfy the issue's acceptance
  criteria.
- **Debate, don't rubber-stamp** — worker and reviewer go back and forth on
  concrete objections until they converge on the best solution, not just an
  acceptable one.
- **Scope triage mid-debate:** if a better solution surfaced in debate still
  fits the issue's scope, fold it into the current PR. If it implies new
  capability or unrelated work, file a new issue (`gh issue create`, proper
  labels, `status:*`, `Blocked-by:` if applicable) instead of scope-creeping the
  current PR.
- **Style-guide amendment obligation** (once `c4.2` exists): if a QA reviewer
  finds a violation `docs/style-guide.md` doesn't already cover, fixing the PR
  isn't sufficient sign-off — either promote the rule into the style guide (tier
  1/2 if mechanically checkable, in the same PR) or file a small, scoped
  follow-up issue to encode it. Never leave a newly-found standard undocumented,
  and never turn that follow-up into another open-ended sweep.
- **No real second identity:** every subagent here authenticates to GitHub as
  the same account. GitHub blocks a same-account PR self-approval, correctly. A
  QA reviewer hitting that block finishes its independent technical audit and
  leaves written findings — it does not work around the block (e.g. treating a
  "comment" review as an approval). Real sign-off in that case is the user's,
  not the orchestrator's.

## 4.5. When the orchestrator merges, and how it handles conflicts

**Checked directly (`gh api repos/.../branches/main/protection`):** this repo
requires the `Library Tests` status check and conversation resolution, but
`required_approving_review_count` is **0** — an approving review is not actually
gate-enforced by GitHub here, only the QA process above requires one. That means
the same-account self-approval block doesn't prevent merging; it only prevents
the QA step from being a formal GitHub "Approve." Don't conflate the two, and
don't let "GitHub didn't require it" become an excuse to skip the debate in §4 —
that's a process rule stronger than what branch protection enforces, kept as
insurance regardless of what GitHub demands.

**The orchestrator merges a PR once, in order:**

1. Required status checks are green (`Library Tests`, currently).
2. All review conversations are resolved.
3. Either: the change was trivial enough to skip §4's ceremony entirely, or §4's
   QA debate concluded with the reviewer's written sign-off as a PR comment (not
   a formal "Approve," per the identity limitation above).

Merge method matches this repo's existing convention:
`gh pr merge --squash --delete-branch` (confirmed from recent history — PR title
becomes the squash commit message with `(#N)` appended, branch deleted on
merge).

**Merge conflicts, resolved by the orchestrator, not a fresh worker:**
consistent with the "pure bookkeeping" exception in §8 — re-deriving context in
a new worker to resolve a conflict between two already-reviewed branches costs
more than it saves.

1. In the PR's own worktree: `git fetch origin main && git merge origin/main`
   (or rebase, whichever this repo's history favors for the branch in question —
   check recent examples rather than picking blind).
2. Resolve conflicts. **Classify the resolution before pushing it:**
   - _Textual/adjacent_ (both diffs touched nearby lines, no real semantic
     overlap) — resolve directly, this is orchestration, not new implementation.
   - _Semantic_ (both diffs changed the same behavior, a type both branches
     touch, or one branch's change no longer makes sense given the other's) — do
     **not** silently pick one side. This means two "already-reviewed" branches
     disagreed about something real; treat the resolution itself as new
     implementation subject to the full §4 gate again, and say so explicitly
     rather than quietly merging a guess.
3. Re-run the full presubmit (`cargo test --lib -q && cargo clippy -q`,
   dogfooded fmt/lint/sync) on the resolved state before pushing — a conflict
   resolution that compiles is not the same as one that's correct.
4. Push, wait for CI to go green again, then merge per the steps above.

## 5. Smart Format principle

`fml fmt` must leave files in a state that doesn't immediately fail trivial lint
checks. Mechanical fixes (import sorting, structural markdownlint fixes) belong
in `LanguageSurface::format()`. `fml lint` is semantic analysis only.

## 6. Maximize parallelism, respect real conflict risk

Dispatch every currently-unblocked issue simultaneously, each in its own
worktree — sequential dispatch of independent issues wastes wall-clock time for
no safety benefit. The one hard constraint: never run two workers concurrently
against branches whose diffs are likely to overlap (e.g. `c2` and `c3`, both
touching module layout, even though not a hard `Blocked-by`). When only one
issue is unblocked, that's a signal to wait, not to jump the dependency order to
find something else to parallelize.

## 7. Role-based routing, not model/vendor-based

Assume any agent is fully capable of any role, including dispatching its own
subagents. Route by **role**, not by which model happens to be running:

- **Orchestrator / QA-reviewer role** — dispatching, reviewing diffs
  skeptically, debating design (§4), scope-triage. Wants more reasoning depth:
  where the harness exposes a thinking-budget/effort control, run this role
  high. This is the role that has to catch subtle bugs and push back with
  concrete objections — under-thinking here is where bad merges get through.
- **Implementer / worker role** — following an already-scoped issue, writing
  code, running presubmit, opening a PR. Medium effort is normally enough — the
  hard judgment calls already happened when the issue was scoped and when QA
  reviews the result.
- **Concretely, on Claude Code:** the `Agent` tool takes an explicit
  `model`/effort parameter per subagent dispatch — set it higher for a
  QA-reviewer or orchestrator sub-dispatch than for a scoped implementation
  task.
- **On Antigravity or other harnesses:** check what thinking-budget/ model-tier
  control is actually exposed before assuming this maps 1:1 — don't guess at a
  knob that might not exist. If there's no equivalent, route by which _agent
  instance_ picks up which role instead, and update this section once known
  rather than leaving a stale assumption.

## 8. Default to orchestrator role

Whichever agent is prompted defaults to **dispatching/reviewing**, not executing
directly — even for a single non-parallelizable task. Before doing
implementation work in the current turn, ask: could this be handed to a worker
subagent in its own worktree instead? Default yes. Direct execution is the
deliberate exception: pure bookkeeping with no code-review surface (labeling
issues, checking CI, merging an already-reviewed PR), resolving a merge conflict
between two already-reviewed branches, or a task so trivial that spinning up a
worker costs more than it saves. Anything beyond a trivial one-liner still gets
the §4 QA gate even when the orchestrator wrote it directly — being the same
agent that would review it is not a shortcut around review.

## 9. Design-phase stop rule

An issue whose scope requires a real UX/architecture decision _with the user_ —
not just a technical judgment call — gets `status:design-phase` + a
`Needs-user-design: yes` line in its body. Any agent about to write code against
such an issue stops and tells the user to open a fresh chat to architect it
first. It does not implement, and does not silently reinterpret the issue to
make it implementable without that conversation. (`#76` — eliminating `fml sync`
via editor-native config parsing — is the concrete case that produced this
rule.)

## 10. Applied-feature checkpoint

An issue introducing new user-facing CLI surface (a command, flags, an output
format) requires presenting the concrete proposal — example invocation, example
output — to the user for confirmation before finalizing. Never
build-then-reveal. (`fml migrate schema`, `fml stat`.)

## 11. Issue conventions

Every issue: ≥1 topical label (`architecture`, `dx`, `documentation`, `rust`,
`ci`, `compatibility`, `surface`) + exactly one `status:*` label
(`status:ready`, `status:blocked`, `status:design-phase`, `status:in-progress`,
`status:in-review`). A blocked issue states `Blocked-by: #N` once in its own
body, set at filing time, never edited into a shared document afterward (see
"Tracking issue" below for why that distinction matters). Spinoff issues carry a
"Spun off from #N" line.

## Tracking issue — why it's generated, never hand-edited

GitHub's issue-update API has no optimistic concurrency (no ETag/ If-Match) —
two agents editing the same issue body around the same time is a real, silent
lost-update race. The fix isn't "only the orchestrator writes it" as a protocol
rule (nothing stops a second concurrent session, and the failure mode — silent
overwrite — is worse than a stale file, because it looks authoritative).
Instead: **state lives on `status:*` labels on each individual issue** (narrow,
low-contention writes); **the tracking issue's body is a full-overwrite
regeneration** of a `gh issue list --label status:X` query, never a
diff/incremental edit. Two concurrent regenerations computing the same answer
from the same labels can't diverge, so last-write-wins is harmless there even
though GitHub itself doesn't prevent it.
