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
- **[table-spec.md](table-spec.md)** — What's the JSON schema `fml table`
  consumes, and what styling rules (`src/ui/table`) does it apply? Read this
  before generating table JSON from a script, or touching table rendering.
- **[style-guide.md](style-guide.md)** — Beyond `rustfmt`/`clippy`, what does
  this codebase itself require (module/file hierarchy, naming, doc-comment
  conventions, `ExecutionContext` Arc-sharing, error-handling patterns)? Read
  this before writing new code, and cite it by section number in review.
- **[release.md](release.md)** — How is a release actually cut — binary (`v*`)
  tags, schema (`s*`) tags, changelog generation via `git-cliff`? Read this
  before cutting a release, or when you need to know what a given tag prefix
  means.
- **[adr/](adr/README.md)** — Why was a specific non-obvious architectural or
  process decision made, and who/what PR made it? Read one when you're about to
  second-guess or rework something that was already a deliberate choice, before
  redoing that debate from scratch.

## Outside this index

These live outside `docs/`, so they're outside this index's scope, but they're
where several common questions actually get answered:

- `README.md` — project overview, installation, quick start. Its own "Further
  reading" section links every doc listed above.
- `COMPATIBILITY.md` — binary-version-to-schema-version matrix.
- `AGENTS.md` — the short agent-facing brief (commands, layout, conventions,
  ask-first list) that points at this index.
- `.agents/orchestrate.md` — the multi-agent orchestration process: worktree
  isolation, the maker-checker QA gate, dispatch order, issue/label conventions.
- `CONTRIBUTING.md`, `SECURITY.md` — contribution workflow and vulnerability
  reporting.
