# formality orchestration process

This is the _process_ — how work on this repo gets planned, dispatched,
reviewed, and merged. It replaces the old git-ignored `.artifacts/PLAN.md`.
State (what's ready, blocked, in review) lives entirely in GitHub issue
`status:*` labels, not in this file — this file only changes when the _process
itself_ changes.

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
- **Finding current work:** `gh issue list --label status:ready` is the _only_
  source of truth. There used to be a pinned master tracking issue generated
  from this same query for humans skimming on the web; it's gone (2026-08-24) —
  its regeneration was manual, went stale between sessions, and a second source
  of truth that can silently disagree with the real one is worse than no
  snapshot at all. Query labels directly instead of trusting any cached summary,
  tracking issue or otherwise.

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
- Progressive 2-tier quality gate:
  - **Tier 1 (Local pre-commit hook)**: `.githooks/pre-commit` (activated via
    `git config core.hooksPath .githooks`). Builds the fresh binary
    (`cargo build -q --bin fml`) and runs `fml fmt --staged` and
    `fml lint --staged` before every commit. This repo's root carries only
    `formality.toml` without generated native config files (`.rustfmt.toml`,
    `.prettierrc`, etc.), so `fml sync --check` is not run against this repo's
    root.
  - **Tier 2 (Parallel PR checks)**: `.github/workflows/pr-check.yml` runs 3
    parallel jobs on every PR:
    1. `Library Tests` (**required status check**):
       `cargo clippy --all-targets -- -D warnings` and full unit/integration
       test suite (`cargo test --verbose`).
    2. `Formality Dogfooding`: `fml fmt --check` and `fml lint` against this
       repo, plus schema-drift verification
       (`fml schema --output schema/formality.schema.json && fml fmt schema/formality.schema.json && git diff --exit-code`)
       and `SCHEMA_VERSION` progression enforcement in `src/config/schema.rs`.
    3. `Security Audit`: `cargo audit`.
- Before every commit: standard presubmit command suite:
  `cargo test --lib -q && cargo clippy --all-targets -- -D warnings`, dogfooded
  with the freshly built binary (`cargo run -q -- fmt`), never a stale global
  `fml`. The staged pre-commit gate enforces dogfooding on staged files — expect
  it to block a commit that fails its own dogfooding, and treat that as working
  as intended, not a bug to route around.

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
- **Scope triage — not just a QA-debate thing.** Applies whenever anyone
  (worker, QA reviewer, orchestrator) finds something out of scope: if it still
  fits the issue as filed, fold it in. If it implies new capability, a different
  part of the codebase, or a design decision the issue didn't ask for, don't
  scope-creep the current PR — file it as its own issue (`gh issue create`,
  topical label + `status:*` per §11, `Blocked-by:` if applicable, and a "Spun
  off from #N" line pointing back here).
- **Audit/survey-shaped issues fan out by nature, more than scoped
  implementation ones — expect it, don't treat it as scope creep when it
  happens.** An issue whose job is _reading across the codebase_ (`c4.2`'s
  style-guide authoring, `c10`'s sweep, `k1`'s docs backfill, or any future
  Wave-6-style audit) will routinely turn up more than one PR's worth of
  findings — that's the audit doing its job, not the worker going off-scope.
  Same in/out-of-scope split as above decides what's foldable vs. spun off; the
  difference is only that a survey issue should produce _several_ spinoffs as a
  matter of course, not zero or one.
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
3. Re-run the full presubmit
   (`cargo test --lib -q && cargo clippy --all-targets -- -D warnings`,
   dogfooded fmt/lint) on the resolved state before pushing — a conflict
   resolution that compiles is not the same as one that's correct.
4. Push, wait for CI to go green again, then merge per the steps above.

## 4.6. Post-merge cleanup — leave nothing behind, every time

A merge is not done until the local footprint it created is gone too. This is
what let 6 worker worktrees and 8GB of stale build cache accumulate before a
manual cleanup caught it (2026-08-22) — the process never said to close the
loop, only to open it. Every merge, immediately, no exceptions:

1. **Remove the worktree**: `git worktree remove <path>` (`--force` if it has
   ignorable residue — build artifacts, not real uncommitted work; check
   `git status` in it first if unsure). Creating a worktree (§1) and never
   removing it is the actual root cause of the mess this section exists to
   prevent.
2. **Delete the local branch**: `git branch -d <branch>` — the remote side is
   already handled by `--delete-branch` in §4.5's merge command; this is the
   local half of the same cleanup, easy to forget because it doesn't error
   loudly like a stale worktree eventually does.
3. **`git worktree prune`** periodically (start of a dispatch batch is a good
   moment) to catch anything removed by hand outside `git worktree remove`
   instead of through it.
4. **Target-dir growth is a design problem, not just a discipline problem** —
   each `git worktree` gets its own `target/` by default, so N concurrent
   worktrees means N independent multi-GB build caches. Prefer fixing this at
   the root over relying on step 1 alone: set a shared `CARGO_TARGET_DIR` (env
   var, or `[build] target-dir` in `.cargo/config.toml`) so every worktree
   incrementally shares one build cache instead of each growing its own. If
   that's not set up yet, step 1 still takes a worktree's `target/` with it when
   the worktree is removed — confirmed working this session — so it isn't
   silently leaked, just less efficient than sharing one cache would be.
5. **Dead code is a merge-gate check, not a follow-up sweep.** If a diff makes
   something unreachable (e.g. `c2`'s `lib.rs` split must delete the old inline
   code paths it replaces, not just add new modules alongside them), that's part
   of what §4's QA review checks before merge, same as any other acceptance
   criterion — don't let it slip through on the assumption `c10`'s sweep will
   catch it later. `c10` is a backstop for what's already there today, not a
   substitute for reviewing new dead code in on the way in.
6. **Local scratch tied to a closed issue** (design notes, one-off planning
   files outside version control) is safe to delete once that issue is closed —
   its content is now either implemented or superseded by real committed docs.
   Don't let local-only files outlive the work they were scratch for.

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
deliberate exception, and only for these two cases: pure bookkeeping with no
code-review surface (labeling issues, checking CI, merging an already-reviewed
PR), or resolving a merge conflict between two already-reviewed branches. Don't
extend this list by judgment call in the moment — "this one's trivial too" is
exactly the rationalization this section exists to block. Anything beyond these
two, however small, still gets the §4 QA gate even when the orchestrator wrote
it directly — being the same agent that would review it is not a shortcut around
review.

## 9. Design-phase stop rule

An issue whose scope requires a real UX/architecture decision _with the user_ —
not just a technical judgment call — gets `status:design-phase` + a
`Needs-user-design: yes` line in its body. Any agent about to write code against
such an issue stops and tells the user to open a fresh chat to architect it
first. It does not implement, and does not silently reinterpret the issue to
make it implementable without that conversation. (`#76` — eliminating `fml sync`
via editor-native config parsing — is the concrete case that produced this rule.
That citation predates the 2026-08-26 repo recreation and no longer resolves —
see [`docs/INDEX.md`](../docs/INDEX.md#note-on-pre-recreation-issuepr-numbers).)

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
body, set at filing time. **This goes stale easily — a `Blocked-by` target
closing doesn't automatically flip the label.** Before trusting a
`status:blocked` label, check whether its named blocker(s) are actually still
open; if not, unblock it (remove `status:blocked`, add `status:ready` or
whatever's next, and leave a comment explaining why) rather than leaving a label
that's lying about the real state. Spinoff issues carry a `Spun off from #N`
line.

### Why state lives only on per-issue labels, not a shared document

GitHub's issue-update API has no optimistic concurrency (no ETag/If-Match) — two
agents editing the same shared document around the same time is a real, silent
lost-update race, and a stale cached summary that looks authoritative is worse
than no summary at all (this repo used to keep one — a pinned tracking issue
regenerated from a `status:*` query — and it went stale between sessions with
nothing forcing a refresh; removed 2026-08-24). `status:*` labels on each
individual issue are narrow, low-contention writes: two sessions touching
different issues' labels can't collide, and there's no aggregate snapshot to
fall out of sync. The cost is that nothing pushes a stale `Blocked-by` to
correct itself — see above.
