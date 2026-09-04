# Contributing to Formality (`fml`)

First, thank you for considering contributing to `formality`! We welcome
contributions, bug reports, feature requests, and documentation improvements.

`fml` is a polyglot format, lint, and config orchestrator supporting 12+
language surfaces. By adhering to these guidelines, you help keep the codebase
maintainable, consistent, and reliable.

---

## Table of Contents

- [Design Principles](#design-principles)
- [Local Setup](#local-setup)
- [Repository Structure](#repository-structure)
- [Testing & Presubmit Checks](#testing--presubmit-checks)
- [Git Pre-Commit Hook](#git-pre-commit-hook)
- [Pull Request Process](#pull-request-process)
- [Commit Message Guidelines](#commit-message-guidelines)
- [Rules & Guidelines](#rules--guidelines)

---

## Design Principles

When contributing code or new features to `fml`, keep these core principles in
mind:

1. **Single Canonical Config (`formality.toml`)**: Global options (indent size,
   line length, EOL, charset) are defined once in `formality.toml` or
   `.formality.toml` and propagated to native language tools.
2. **Zero-Boilerplate Defaults**: Sensible default tool mappings are embedded
   out of the box (`rustfmt`, `clippy`, `ruff`, `clang-format`, `prettier`,
   `taplo`, `typstyle`, etc.). Active language surfaces are declared via
   `languages = [...]`.
3. **Mechanical Formatting vs. Semantic Linting**:
   - `fml fmt` handles mechanical formatting fixes.
   - `fml lint` handles semantic checks and diagnostics.
4. **Config Sync Engine (`fml sync`)**: Generates native tool configurations
   from canonical globals while detecting configuration drift. Native configs
   are verified and never overwritten without explicit confirmation.
5. **Automated Tool Management (`fml install`)**: Missing binary dependencies
   are detected and can be auto-installed via package managers (`cargo`, `npm`,
   `pip`, `brew`, `rustup`). `fml fmt -i` and `fml lint -i` support on-demand
   installations.
6. **Blazing Parallel Runner**: Multi-threaded execution (`rayon`) runs
   independent language surfaces concurrently.
7. **Always Dogfood**: Always test and verify with the freshly built binary
   (`cargo run -q -- ...`), never rely on a stale global `fml` executable on
   your `PATH`.
8. **Deterministic Exit Codes**:
   - `0`: All clean / passed.
   - `1`: Formatting or lint violations found, or config drift detected.
   - `2`: Missing tool or underlying execution error.

---

## Local Setup

### Prerequisites

- **Rust toolchain**: Version pinned in `rust-toolchain.toml` (e.g., Rust
  1.98.1). Install via `rustup`:

  ```bash
  rustup show
  ```

- **Git**: Ensure Git is installed and configured.
- **GitHub CLI (`gh`)**: Recommended for creating and managing PRs.

### Initial Setup

1. **Fork and clone the repository**:

   ```bash
   git clone https://github.com/arvinduh/formality.git
   cd formality
   ```

2. **Build the project**:

   ```bash
   cargo build
   ```

3. **Install tool dependencies (optional/on-demand)**:

   ```bash
   cargo run -q -- install
   ```

---

## Repository Structure

- `src/config`: `formality.toml` parsing, resolution, and schema management.
- `src/engine`: Surface execution, diffing, and update/sync checking logic.
- `src/surfaces`: Per-language surface implementations (1 file per surface). See
  [`docs/new-surface-guide.md`](docs/new-surface-guide.md) to add a surface.
- `src/ui`: CLI table rendering and user interface formatting.
- `src/commands`: Subcommand implementations (`fmt`, `lint`, `sync`, `install`,
  `table`, etc.).
- `docs/`: In-depth specification docs
  ([`facet-rosetta.md`](docs/facet-rosetta.md),
  [`language-surfaces.md`](docs/language-surfaces.md),
  [`new-surface-guide.md`](docs/new-surface-guide.md),
  [`table-spec.md`](docs/table-spec.md), [`release.md`](docs/release.md)).

---

## Testing & Presubmit Checks

Before committing or submitting a pull request, you **must** run and pass the
presubmit suite:

```bash
cargo test --lib -q && cargo clippy --all-targets -- -D warnings
cargo run -q -- fmt
```

### Explanation of Presubmit Commands

1. `cargo test --lib -q`: Runs library unit tests silently.
2. `cargo clippy --all-targets -- -D warnings`: Ensures zero Clippy warnings
   across all targets (lib, bins, tests, examples).
3. `cargo run -q -- fmt`: Dogfoods the freshly built `fml` binary to format the
   repository.

> [!NOTE] This repository carries no generated native config files
> (`.rustfmt.toml`, `.prettierrc`, etc.) — formatting and linting resolve
> `formality.toml` inline, so `cargo run -q -- sync --check` is not run against
> this repository's root.

---

## Git Pre-Commit Hook

A pre-commit hook is provided in `.githooks/pre-commit` as **Tier 1** of our
progressive quality gate to automatically run fast formatting and linting sanity
checks on staged files before committing.

### Enabling the Hook

To activate the hook for your local clone, configure Git's hooks path:

```bash
git config core.hooksPath .githooks
```

### What It Runs

When triggered on `git commit`, the hook:

1. Builds the local binary fresh with `cargo build -q --bin fml` (exiting with
   failure if the build fails).
2. Runs mechanical formatting on staged files: `$FML fmt --staged`.
3. Runs semantic linting on staged files: `$FML lint --staged`.

---

## Pull Request Process

1. **Create a topic branch**: Branch off `main` with a descriptive name:

   ```bash
   git checkout -b feat/my-new-feature
   # or
   git checkout -b fix/issue-123
   ```

2. **Make your changes**: Ensure all existing comments and docstrings unrelated
   to your changes are preserved. Maintain documentation integrity.

3. **Run presubmit checks**: Run the presubmit suite commands above to verify
   code quality and dogfood formatting.

4. **Commit your changes**: Use Conventional Commits format (see below).

5. **Push your branch to GitHub**:

   ```bash
   git push -u origin <branch-name>
   ```

6. **Open a Pull Request**: Use `gh pr create`:

   ```bash
   gh pr create --title "type(scope): summary (#issue)" --body "Description of changes... Closes #issue"
   ```

7. **CI PR Checks (Tier 2 Quality Gate)**: GitHub Actions executes 3 parallel
   jobs on every PR:
   - **`Library Tests`** (_required branch protection status check_): Runs
     `cargo clippy --all-targets -- -D warnings` and the full unit/integration
     test suite (`cargo test --verbose`).
   - **`Formality Dogfooding`**: Runs `fml fmt --check` and `fml lint` against
     this repository's live tree, verifies schema drift (`fml schema` vs.
     `schema/formality.schema.json`), and enforces forward `SCHEMA_VERSION`
     progression in `src/config/schema.rs`.
   - **`Security Audit`**: Runs `cargo audit` against the Rust advisory
     database.

---

## Commit Message Guidelines

Commits **must** strictly follow the
[Conventional Commits](https://www.conventionalcommits.org/) specification:

```text
<type>(<scope>): <description> (Fixes #<issue>)
```

### Commit Types

- `feat`: A new feature or surface capability
- `fix`: A bug fix
- `docs`: Documentation updates (e.g.,
  `docs(community): add CONTRIBUTING.md (Fixes #129)`)
- `refactor`: Code restructuring without functional changes
- `test`: Adding or modifying unit/integration tests
- `chore`: Maintenance, dependencies, or workflow changes

### Example Commit Messages

- `feat(engine): add multi-threaded execution for typst surface (Fixes #42)`
- `fix(config): resolve drift check false positive in editorconfig (Fixes #88)`
- `docs(community): add CONTRIBUTING.md and issue templates (Fixes #129)`

---

## Rules & Guidelines

- **Never commit directly to `main`**: All changes must go through pull
  requests.
- **Ask before modifying**:
  - Branch protection rules or required CI status check names.
  - Project version bumps (managed by dedicated release automation, not manual
    edits).
- **Never rely on global binaries**: Always test with `cargo run -q -- ...`.
- **Preserve API contracts**: When modifying signatures, search and update all
  invocation sites across the repository.
