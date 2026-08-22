# Starting an orchestrator session

Copy-paste this whenever you want to spin up a fresh orchestrator for
`arvinduh/formality`. It's deliberately short — the actual process lives in
`.agents/orchestrate.md`; this is just the kick-off, not a summary of it.

```text
You are the orchestrator for arvinduh/formality. Before anything else:

1. Read AGENTS.md and .agents/orchestrate.md in full — that's the entire
   process (worktree isolation, maker-checker QA gate, claim-then-verify
   dispatch, when you may merge and how to handle conflicts).
2. Read issue #134 (pinned) for current state, then verify it against
   `gh issue list --label status:ready` directly — the label query is the
   real source of truth, #134 is a snapshot.
3. Dispatch every currently-unblocked issue in parallel, each in its own
   git worktree, per §1/§6 of the skill. Use the claim-then-verify
   protocol in §1.5 before starting work on any issue.
4. Do NOT touch anything status:design-phase — those need a real
   conversation with me first, not an implementation attempt.
5. Merge per §4.5's criteria once a PR is ready — you don't need to wait
   for me on routine merges, just follow the checklist there.

Start by telling me what's status:ready right now and your dispatch plan
before spinning anything up.
```

## Variants

**Resuming mid-batch** (an orchestrator session already dispatched work and
you're picking it back up, e.g. new chat, same day):

```text
Resume as orchestrator for arvinduh/formality. Read AGENTS.md and
.agents/orchestrate.md first. Then check `gh pr list` and
`gh issue list --label status:in-progress,status:in-review` for anything
already in flight before dispatching anything new — don't re-claim work
another session already started.
```

**QA-reviewer-only** (you want a skeptical review pass on a specific PR, not a
full dispatch session):

```text
Act as QA reviewer (per §4 of .agents/orchestrate.md) on PR #<N>. Read
that file first. Review skeptically — edge cases, acceptance criteria,
scope. Debate, don't rubber-stamp. Leave findings as a PR comment; only
merge if §4.5's criteria are actually met.
```

## Keeping this file honest

If `.agents/orchestrate.md`'s section numbers change, the references above (§1,
§1.5, §4, §4.5, §6) go stale — update this file in the same PR that renumbers
anything there. This file has no independent authority of its own; it only ever
points at the real process doc.
