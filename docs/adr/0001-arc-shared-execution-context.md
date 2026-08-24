# 0001 — Arc-shared `ExecutionContext` fields

**Status:** Accepted **Decided in:** `#50` ("perf(engine): memory & runtime
efficiency optimizer"), landed via PR `#105`.

## Context

`Runner::run` (`src/engine/runner/mod.rs`) builds one `ExecutionContext` per
matched `LanguageSurface` and dispatches all of them in parallel via
`rayon::par_iter`. Before `#105`, two of `ExecutionContext`'s fields —
`paths: Vec<PathBuf>` (the candidate file list) and
`global_config: ResolvedGlobalConfig` — were deep-cloned into every per-surface
context (`paths.to_vec()`, `global_config.clone()`), even though every surface
in that parallel dispatch sees the _same_ values. On a repo with N surfaces and
M candidate paths, that's `O(N * M)` redundant allocation on every single
`fml fmt`/`fml lint`/`fml sync` invocation.

## Decision

Wrap `paths` and `global_config` in `Arc` (`Arc<Vec<PathBuf>>`,
`Arc<ResolvedGlobalConfig>`), built once in `Runner::run` and shared via
`Arc::clone` (a refcount bump) per surface, instead of deep-cloning per surface.
`lang_config`, by contrast, stays a plain owned `ResolvedLangConfig` — it's
computed per-surface (`config.resolve_for_lang(surface.name())`), so there's
nothing shared to `Arc` there. The full architectural rationale (and the rule
for new per-invocation fields going forward) now lives in
[style-guide.md](../style-guide.md) §4 rather than being restated here — this
ADR exists to record that the decision was made and where, not to duplicate the
mechanism.

## Consequences

- All 12 surfaces' read sites were already borrow-only (`&ctx.paths`,
  `ctx.global_config.<field>`), so `Arc`'s `Deref` covered every call site with
  no logic changes.
- Measured impact (release build, 12 surfaces x 20,000 paths x 200 runs,
  isolated microbenchmark of the context-construction step): 8.61s before, 174µs
  after — roughly 49,000x on that step specifically. Real-world wall-clock
  impact scales with repo size and surface count, most visible on large
  monorepos running all 12 surfaces.
- Style-guide §4 promotes the underlying rule to a reviewer-checked (tier 3)
  convention: any new field on `ExecutionContext`, or a similarly fanned-out
  per-invocation struct, that holds a value shared identically across every
  parallel surface invocation gets `Arc`-wrapped rather than cloned per surface.
