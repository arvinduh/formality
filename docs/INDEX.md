# Documentation Index

One line per doc: what it answers, and when to reach for it. Check here before
reading source to see whether the structure or convention you're about to
re-derive is already written down.

- **[architecture.md](architecture.md)** — What does the whole `src/` tree look
  like, module by module? Read this first when landing on the codebase cold, or
  when you need to know which subdirectory owns a piece of behavior before
  diving into source.
- **[facet-rosetta.md](facet-rosetta.md)** — What is a "facet," and how does
  `fml` map one canonical formatting/linting concept (indentation, line length,
  import sorting, ...) onto each language's own tool config? Read this before
  adding or reasoning about a cross-language setting.
- **[language-surfaces.md](language-surfaces.md)** — What does each of the 12
  language surfaces actually wrap — which tools, which Smart Format fixes, which
  native config file(s) `fml sync` manages, which `[lang.<name>]` options exist?
  Read this before touching an existing surface's behavior.
- **[new-surface-guide.md](new-surface-guide.md)** — How do I add a 13th
  language surface? Read this before implementing a new `LanguageSurface`.
- **[style-guide.md](style-guide.md)** — Where does `fml` deviate from the
  global `rust-guide`, and what does this codebase itself require (test layout,
  naming, schema doc comments, `ExecutionContext` Arc-sharing, error handling)?
  Read this before writing new code, and cite it by section number in review.
- **[release.md](release.md)** — How is a release actually cut — `v*` tags via
  cargo-dist, the JSON schema asset, GitHub `--generate-notes` release notes?
  Read this before cutting a release.
- **[adr/](adr/README.md)** — Why was a specific non-obvious architectural or
  process decision made, and who/what PR made it? Read one when you're about to
  second-guess or rework something that was already a deliberate choice, before
  redoing that debate from scratch. How issue workflow state is tracked (labels
  vs. assignees, draft PRs, native blockers) is
  [0006](adr/0006-derived-issue-state.md).

## Note on pre-recreation issue/PR numbers

This repository was deleted and recreated on **2026-08-26** to scrub a leaked
personal email from early git history. Issue and PR numbering restarted from
`#1` in the recreated repo, and the counter has now climbed past old issue
numbers, so those citations now resolve to real, unrelated new issues. Any `#N`
citation in source comments or docs that predates 2026-08-26 is a **historical
reference only** — it names the issue/PR where a decision was actually made in
the old repo, but the number does not resolve to that content anymore. To
prevent confusion with active issues, historical citations are explicitly
disambiguated (e.g. `[pre-recreation]`). Do not follow these as live links;
treat them the same as a citation to a commit hash from a previous history.
Files that cite pre-recreation numbers point back here instead of repeating this
explanation at every citation.

## Outside this index

These live outside `docs/`, so they're outside this index's scope, but they're
where several common questions actually get answered:

- `README.md` — project overview, installation, quick start. Its own "Further
  reading" section links every doc listed above.
- `AGENTS.md` — this repo's process facts: the gate, the pre-commit hook,
  required CI checks and merge rules, layout, ask-first list.
- `CONTRIBUTING.md` — contribution workflow.
