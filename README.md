# formality (`fml`)

> One CLI to format, lint, and sync configs across every language you touch.

`fml` orchestrates the best-in-class formatter and linter per language behind a
single canonical configuration (`formality.toml` or `.formality.toml`), running
every surface in parallel for near-instant feedback.

**Further reading**: [Documentation Index](docs/INDEX.md) (what each doc
answers, when to read it) · [Architecture](docs/architecture.md) (whole-repo
module map) · [Facet Rosetta](docs/facet-rosetta.md) (the canonical
cross-language config vocabulary) ·
[Language Surface Guides](docs/language-surfaces.md) (per-surface tools, config,
and behavior) · [Adding a New Surface](docs/new-surface-guide.md) ·
[`fml table` Spec](docs/table-spec.md) · [Style Guide](docs/style-guide.md) ·
[Release Procedure](docs/release.md) ·
[Compatibility Matrix](docs/compatibility.md) · [ADRs](docs/adr/README.md)

---

## Key features

- **Single canonical config (`formality.toml`)**: Define shared globals (indent
  size, line length, EOL, charset) once.
- **Zero-boilerplate defaults**: Embedded default tool mappings (`rustfmt`,
  `clippy`, `ruff`, `clang-format`, `prettier`, `taplo`, `typstyle`).
- **Explicit `languages` scope**: Specify `languages = ["rust", "toml", ...]` to
  declare active surfaces without boilerplate `[lang.x]` tables.
- **Config sync engine (`fml sync`)**: Generates and provably verifies native
  tool configs from canonical globals. Detects manually written config files and
  warns instead of overwriting them.
- **Automated tool installer (`fml install`)**: Detects missing binaries and
  auto-installs them via system package managers (`cargo`, `npm`, `pip`, `brew`,
  `rustup`). Pass `--install` / `-i` to `fml fmt` or `fml lint` to install
  on-demand before the run — no separate setup step needed.
- **Blazing parallel runner**: Runs independent language surfaces concurrently
  using multi-threaded execution (`rayon`).
- **Fine-grained targeting**: Target specific files, directories, Git staged
  (`--staged`), or modified files (`--changed`).
- **Deterministic exit codes**:
  - `0`: All clean / passed.
  - `1`: Formatting or lint violations found, or config drift detected.
  - `2`: Missing tool or underlying execution error.

---

## Supported surfaces

| Language / Surface  | Formatter                               | Linter                   | Managed native config                    |
| :------------------ | :-------------------------------------- | :----------------------- | :--------------------------------------- |
| **Rust**            | `cargo fmt` / `rustfmt`                 | `clippy`                 | `.rustfmt.toml`                          |
| **Python**          | `ruff check --fix` -> `ruff format`     | `ruff check`             | `ruff.toml`                              |
| **C / C++**         | `clang-format`                          | `clang-tidy`             | `.clang-format`                          |
| **Java**            | `google-java-format`                    | `checkstyle`             | `checkstyle.xml`                         |
| **Go**              | `goimports` / `gofmt -s`                | `golangci-lint`          | `.golangci.yml`                          |
| **JavaScript / TS** | `biome format` + organize imports       | `biome lint`             | `biome.json`                             |
| **Kotlin**          | `ktlint -F`                             | `ktlint`                 | `.editorconfig`                          |
| **Markdown**        | `markdownlint-cli2 --fix` -> `prettier` | `markdownlint-cli2`      | `.markdownlint.json`, `.prettierrc.json` |
| **YAML**            | `prettier`                              | `yamllint`               | `.prettierrc.json`                       |
| **JSON**            | `prettier`                              | `prettier`               | `.prettierrc.json`                       |
| **TOML**            | `taplo`                                 | `taplo`                  | `taplo.toml`                             |
| **Typst**           | `typstyle`                              | _(LSP diagnostics only)_ | CLI flags (`--column`)                   |

The full facet-by-facet breakdown of what's configurable, fixed, or unsupported
per surface lives in [docs/facet-rosetta.md](docs/facet-rosetta.md). Per-surface
tool details, Smart Format behavior, and `[lang.<name>]` options are documented
in [docs/language-surfaces.md](docs/language-surfaces.md). Want to add a 13th
surface? See [docs/new-surface-guide.md](docs/new-surface-guide.md).

---

## Installation

### 1-Line Quick Install

These download the matching prebuilt binary from the latest
[GitHub Release](https://github.com/arvinduh/formality/releases/latest), verify
its checksum, and put `fml` on your `PATH`. No Rust toolchain required. The
installer scripts are generated and published by
[cargo-dist](https://opensource.axo.dev/cargo-dist/).

#### Linux & macOS

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/arvinduh/formality/releases/latest/download/fml-installer.sh | sh
```

#### Windows (PowerShell)

```powershell
powershell -c "irm https://github.com/arvinduh/formality/releases/latest/download/fml-installer.ps1 | iex"
```

#### Windows (`.msi`)

Download and run
[`fml-x86_64-pc-windows-msvc.msi`](https://github.com/arvinduh/formality/releases/latest/download/fml-x86_64-pc-windows-msvc.msi)
from the latest release.

> The shorter
> `raw.githubusercontent.com/arvinduh/formality/main/install.{sh,ps1}` URLs
> still work — they now forward to the installers above.

### Build from source via `cargo`

`fml` is not published to crates.io; build it from the Git repository (needs a
Rust toolchain). The trailing `fml` package name is required — the repository
carries more than one binary-bearing manifest, so the bare form errors.

```bash
cargo install --git https://github.com/arvinduh/formality fml
```

### Direct Prebuilt Binaries

Prebuilt standalone binaries are attached to every
[GitHub Release](https://github.com/arvinduh/formality/releases/latest).

#### macOS (Apple Silicon / ARM64)

```bash
curl -fsSL https://github.com/arvinduh/formality/releases/latest/download/fml-aarch64-apple-darwin.tar.gz | tar -xz
mkdir -p ~/.local/bin && mv fml ~/.local/bin/fml
```

#### macOS (Intel / x86_64)

```bash
curl -fsSL https://github.com/arvinduh/formality/releases/latest/download/fml-x86_64-apple-darwin.tar.gz | tar -xz
mkdir -p ~/.local/bin && mv fml ~/.local/bin/fml
```

#### Linux (x86_64)

```bash
curl -fsSL https://github.com/arvinduh/formality/releases/latest/download/fml-x86_64-unknown-linux-gnu.tar.gz | tar -xz
mkdir -p ~/.local/bin && mv fml ~/.local/bin/fml
```

#### Linux (ARM64 / aarch64)

```bash
curl -fsSL https://github.com/arvinduh/formality/releases/latest/download/fml-aarch64-unknown-linux-gnu.tar.gz | tar -xz
mkdir -p ~/.local/bin && mv fml ~/.local/bin/fml
```

#### Windows (x86_64 / PowerShell)

```powershell
Invoke-WebRequest -Uri https://github.com/arvinduh/formality/releases/latest/download/fml-x86_64-pc-windows-msvc.zip -OutFile fml.zip
Expand-Archive fml.zip -DestinationPath $HOME\bin -Force
Remove-Item fml.zip
```

---

## Quickstart

### 1. Initialize in a project

```bash
# Auto-detects surfaces and generates formality.toml
fml init

# Or generate a hidden dotfile (.formality.toml)
fml init --hidden
```

### 2. Check and install toolchains

```bash
# Show which tools are installed and which are missing
fml doctor

# Auto-install missing tools for active surfaces
fml install
```

### 3. Sync tool configs

```bash
# Write / update native tool configs from formality.toml globals
fml sync

# In CI: verify that native configs have not drifted out of sync
fml sync --check
```

### 4. Format and lint

```bash
# Format all detected surfaces in parallel
fml fmt

# First run on a fresh clone? Install missing tools then format in one step
fml fmt --install

# Format only Git staged files (pre-commit hook)
fml fmt --staged

# Format a specific file or directory
fml fmt src/main.rs
fml fmt src/

# Check formatting in CI (exits 1 if changes would be made)
fml fmt --check

# Run linters across all active surfaces
fml lint

# Install missing tools then lint in one step
fml lint --install

# Run linters with auto-fix
fml lint --fix

# Composite pipeline: lint --fix, then reformat, across all active surfaces
# in one command (useful as a single "clean everything up" entrypoint)
fml fix
```

`fml fix` is a three-stage composite: it first runs `lint(fix: true)` (so
semantic autofixes like unused-import removal land first), then runs `format()`
(so the result is guaranteed to be in the canonical formatted state), then
re-lints (check-only) just the surfaces whose lint pass still reported
violations — so the status it prints reflects the tree _after_ formatting, and a
violation the format pass resolved (e.g. a long line prettier rewrapped) no
longer reports `[FAIL]` or forces a non-zero exit. See
[docs/language-surfaces.md](docs/language-surfaces.md) for which surfaces have a
real lint auto-fix mode (`supports_lint_fix()`) versus which only reformat under
`fml fix` because their linter is diagnostics-only (e.g. Java's `checkstyle`,
YAML's `yamllint`, TOML's `taplo lint`).

---

## Configuration (`formality.toml` or `.formality.toml`)

### Minimal setup (zero boilerplate)

```toml
# Always pin to a specific schema tag (e.g. s1.0) — the schema is a release
# asset, not a raw branch file. Tagged independently of the binary's v* release
# (major.minor: major = breaking schema change, minor = additive/compatible).
#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json

[global]
languages = ["rust", "toml", "markdown"]  # Explicit active surfaces
indent_size = 2
line_length = 80
```

Run `fml migrate schema` to rewrite an existing `#:schema` line to point at the
current release's schema tag (or insert one if it's missing). It only touches
that single line — it does not attempt to rewrite config content for a breaking
schema change, since that's a human decision.

```text
$ fml migrate schema
[OK] Updated formality.toml schema reference: s0.9 -> s1.0
```

### Full configuration with overrides

```toml
[global]
languages = ["rust", "python", "markdown", "toml"]
indent_size = 2
line_length = 80
end_of_line = "lf"
charset = "utf-8"
insert_final_newline = true
trim_trailing_whitespace = true
use_tabs = false

# Per-language overrides (only when you need to change defaults)
[lang.python]
indent_size = 4
line_length = 100

[lang.markdown]
prose_wrap = "always"
```

### Layered resolution

1. **Embedded binary defaults**
2. **User config**: `~/.config/formality/config.toml` (or
   `$XDG_CONFIG_HOME/formality/config.toml`; on macOS also
   `~/Library/Application Support/formality/config.toml`; on Windows
   `%APPDATA%\formality\config.toml`)
3. **Project config**: `formality.toml` or `.formality.toml` at repository root
4. **CLI flags**: `--lang <name>`, path arguments, `--config <file>`, etc.

---

## CLI reference

```text
Usage: fml [OPTIONS] <COMMAND>

Commands:
  fmt            Format source files across detected or specified surfaces
  lint           Lint source files across detected or specified surfaces
  fix            Composite pipeline: lint --fix, then fmt, across all active surfaces
  sync           Sync native tool configs from canonical globals
  doctor         Diagnose installed toolchains with install hints
  install        Auto-install missing toolchains using system package managers
  init           Scaffold a new formality.toml configuration
  list-surfaces  List all supported surfaces and detection status
  schema         Print the JSON Schema for formality.toml
  lsp            Start the formality LSP server (stdio transport)
  table          Render an opinionated semantic terminal table from JSON specification
  migrate        Migrate project files to match the current formality release
  help           Print this message or the help of the given subcommand(s)

Options:
  -c, --config <FILE>  Custom path to formality config
  -w, --root <DIR>     Target workspace root (defaults to cwd)
  -h, --help           Print help
  -V, --version        Print version
```

### Key flags

| Command       | Flag        | Description                                                                                    |
| :------------ | :---------- | :--------------------------------------------------------------------------------------------- |
| `fml fmt`     | `--check`   | Exit 1 if any file would be reformatted (CI safe)                                              |
| `fml fmt`     | `--install` | Auto-install missing tools for active surfaces, then format                                    |
| `fml fmt`     | `--staged`  | Operate only on `git diff --cached` files                                                      |
| `fml fmt`     | `--changed` | Operate only on `git diff` (unstaged) files                                                    |
| `fml fmt`     | `--lang`    | Filter to a specific surface, e.g. `--lang rust`                                               |
| `fml lint`    | `--fix`     | Apply auto-fixes where the tool supports it                                                    |
| `fml lint`    | `--install` | Auto-install missing tools for active surfaces, then lint                                      |
| `fml lint`    | `--staged`  | Operate only on `git diff --cached` files                                                      |
| `fml lint`    | `--changed` | Operate only on `git diff` (unstaged) files                                                    |
| `fml lint`    | `--lang`    | Filter to a specific surface                                                                   |
| `fml fix`     | `--staged`  | Operate only on `git diff --cached` files                                                      |
| `fml fix`     | `--changed` | Operate only on `git diff` (unstaged) files                                                    |
| `fml fix`     | `--lang`    | Filter to a specific surface                                                                   |
| `fml fix`     | `--install` | Auto-install missing tools for active surfaces, then fix                                       |
| `fml sync`    | `--check`   | Exit 1 if any native config is out of sync                                                     |
| `fml sync`    | `--lang`    | Filter to a specific surface                                                                   |
| `fml doctor`  | `--all`     | Show all surfaces, not just active ones                                                        |
| `fml doctor`  | `--install` | Auto-install all missing toolchains                                                            |
| `fml install` | `--all`     | Install tools for all supported language surfaces                                              |
| `fml init`    | `--force`   | Overwrite an existing config file                                                              |
| `fml init`    | `--hidden`  | Write `.formality.toml` instead of `formality.toml`                                            |
| `fml table`   | `--json`    | Table spec JSON string (reads stdin if omitted) — see [docs/table-spec.md](docs/table-spec.md) |
| `fml migrate` | `schema`    | Rewrite `#:schema` directive in config to match current release                                |

---

## Config sync and manual configs

`fml sync` generates native tool config files (`.rustfmt.toml`, `ruff.toml`,
`.clang-format`, etc.) from your canonical `formality.toml` settings. Every
generated file starts with a sentinel comment:

```toml
# ==============================================================================
# WARNING: DO NOT EDIT THIS FILE DIRECTLY!
# This file is automatically generated and managed by formality (fml).
# ...
```

### Manually written configs

If formality finds a native config file that does **not** contain the
auto-generation sentinel, it treats it as manually managed and reports a
`[MANUAL]` warning instead of overwriting it:

```text
  [MANUAL] rust         .rustfmt.toml is manually managed
```

The diagnostics section explains exactly how to resolve this:

#### Option A — Let formality manage the file

1. Back up your custom settings.
2. Delete the file and run `fml sync` to generate a clean copy.
3. Migrate your customizations into `formality.toml` using `[lang.<name>]`
   overrides (`indent_size`, `line_length`, `extra_args`, etc.).

#### Option B — Keep managing the file yourself

Add the auto-generation sentinel as the first comment block of your file. Once
formality sees the sentinel it will treat the file as managed and overwrite it
on the next `fml sync`. This option is for when you want to hand-craft the exact
generated output rather than derive it from `formality.toml`.

---

## CI / CD integration

### GitHub Actions

The only prerequisite is `fml` itself. Once it's on `PATH`, `fml install`
handles every downstream tool (`ruff`, `prettier`, `markdownlint-cli2`, `taplo`,
…) — no extra `setup-ruff`, `setup-node`, or `npm install` steps required.

```yaml
- name: Install fml
  run: |
    curl --proto '=https' --tlsv1.2 -LsSf https://github.com/arvinduh/formality/releases/latest/download/fml-installer.sh | sh
    echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"

- name: Install tool dependencies
  run: fml install

- name: Verify config sync
  run: fml sync --check

- name: Check formatting
  run: fml fmt --check

- name: Lint
  run: fml lint
```

Rust-heavy projects that already have a Rust toolchain step can combine
`fml install` and the format/lint check into a single flag:

```yaml
- name: Check formatting
  run: fml fmt --check --install

- name: Lint
  run: fml lint --install
```

> **Tip**: Set `FORMALITY_NO_UPDATE_CHECK=1` in your CI environment to suppress
> the update-check network request on every invocation.

### Pre-commit hook

formality ships a ready-to-use hook in `.githooks/`. Activate it with one
command; no extra tooling required:

```bash
git config core.hooksPath .githooks
```

The hook (`fmt --staged` → `lint --staged`) runs on every commit. Commit the
`.githooks/` directory so the whole team gets it on clone.

#### If your project uses the pre-commit framework

`.pre-commit-hooks.yaml` in the formality repo makes it available as a hook
source for other projects:

```yaml
# .pre-commit-config.yaml in your project
repos:
  - repo: https://github.com/arvinduh/formality
    rev: v0.2.1
    hooks:
      - id: fml-fmt
      - id: fml-lint
      # Optional escape hatch — only if your project also commits native tool
      # configs and you want them verified against formality.toml on commit.
      - id: fml-sync
```

---

## VS Code extension

### Extension installation

#### From a release `.vsix` file

1. Download `formality-<version>.vsix` from the
   [latest release](https://github.com/arvinduh/formality/releases/latest).
2. In VS Code: **Extensions** → `...` menu → **Install from VSIX…**
3. Select the downloaded `.vsix` file.

#### From the command line

```bash
code --install-extension formality-<version>.vsix
```

### What the extension does

- Registers `fml fmt` as the document formatter for all supported languages. Use
  **Format Document** (`Shift+Alt+F`) or enable **Format on Save** in VS Code
  settings.
- Watches `formality.toml` / `.formality.toml` and auto-runs `fml sync` whenever
  the file is saved or created (configurable).
- Exposes commands in the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`):

| Command                                | Description                            |
| :------------------------------------- | :------------------------------------- |
| `Formality: Format Entire Workspace`   | Run `fml fmt` on the workspace         |
| `Formality: Lint Entire Workspace`     | Run `fml lint` on the workspace        |
| `Formality: Lint Workspace (Auto-Fix)` | Run `fml lint --fix` on the workspace  |
| `Formality: Sync Native Configs`       | Run `fml sync` manually                |
| `Formality: Run Toolchain Doctor`      | Run `fml doctor --all` and show output |

### Extension settings

| Setting                          | Default | Description                                                                                                      |
| :------------------------------- | :------ | :--------------------------------------------------------------------------------------------------------------- |
| `formality.executablePath`       | `"fml"` | Path to the `fml` binary. Override if `fml` is not on `PATH`.                                                    |
| `formality.autoSyncOnConfigSave` | `true`  | Auto-run `fml sync` when `formality.toml` is saved or created. Set to `false` to manage native configs manually. |

**Example `.vscode/settings.json`**

```json
{
  "formality.executablePath": "/usr/local/bin/fml",
  "formality.autoSyncOnConfigSave": false,
  "[rust]": {
    "editor.defaultFormatter": "arvinduh.formality",
    "editor.formatOnSave": true
  }
}
```

---

## Editor setup

The whole point of `fml` is that you configure formatting **once** and stop
wiring up a formatter extension per language. In an editor that means a single
step: point it at **`fml lsp`** and let it format every language `fml` covers.

`fml lsp` is a Language Server that speaks over stdio, so any LSP-capable editor
can run it with no plugin. It provides exactly two things:

- **Formatting** (`textDocument/formatting`), routed through `fml fmt`
- **Lint diagnostics** (`fml lint` output), published on open and on save

It is **not** a replacement for `rust-analyzer`, `pyright`, `gopls`, `clangd`,
or any other language server, and it does not proxy or spawn them. Keep your
existing language servers attached for completion, hover, and go-to-definition;
`fml lsp` only owns formatting and lint diagnostics.

### `fml lsp` vs `fml sync` — pick one

Both keep formatting consistent with `formality.toml`. They are opposites, and
only one of them reduces how much you configure in your editor:

|                        | `fml lsp`                       | `fml sync`                                        |
| :--------------------- | :------------------------------ | :------------------------------------------------ |
| Who runs the formatter | `fml` does                      | your editor's own per-language extensions do      |
| Editor setup           | one formatter, pointed at `fml` | still one extension per language                  |
| What it writes to disk | nothing                         | `.rustfmt.toml`, `.prettierrc.json`, `biome.json` |
| Config lives in        | `formality.toml` only           | `formality.toml`, mirrored onto disk              |

Use **`fml lsp`** for the one-formatter workflow. Reach for **`fml sync`** only
as an escape hatch — when an editor, a teammate's setup, or another tool insists
on reading a native config file off disk. `fml sync` does not reduce the number
of formatters you configure; it only keeps the ones you already have in
agreement with `formality.toml`.

### VS Code

Install the [extension](#vs-code-extension) (below); it launches `fml lsp` for
you. Then set formality as the default formatter and turn on format on save —
see the example `.vscode/settings.json` in that section.

### Neovim

No plugin required — not even `nvim-lspconfig`. Copy
[`editors/nvim/formality.lua`](editors/nvim/formality.lua) to
`~/.config/nvim/plugin/formality.lua` (Neovim 0.10+, verified against 0.11).

It uses two autocommands: a `FileType` hook that runs `vim.lsp.start` with
`cmd = { "fml", "lsp" }` for every filetype `fml` formats (rooted at the nearest
`formality.toml`), and a `BufWritePost` hook that formats and reloads the
buffer. Format-on-save runs _after_ the write, not on `BufWritePre`, because
`fml lsp` declares `textDocumentSync = none` and formats by rewriting the
**saved file on disk** — doing it before the write makes Neovim warn that the
file "changed since reading it" on every save.

Edit the file if `fml` isn't on your `PATH`, or to trim the filetype list.

### Helix

Helix takes stdio language servers directly in `~/.config/helix/languages.toml`,
no plugin needed. Register `fml lsp`, then add it to each language and use
Helix's per-server feature filtering so `fml` owns formatting while the primary
server keeps everything else:

```toml
[language-server.formality]
command = "fml"
args = ["lsp"]

[[language]]
name = "rust"
auto-format = true
language-servers = [
  { name = "rust-analyzer", except-features = ["format"] },
  { name = "formality", only-features = ["format", "diagnostics"] },
]

[[language]]
name = "python"
auto-format = true
language-servers = [
  { name = "pyright", except-features = ["format"] },
  { name = "formality", only-features = ["format", "diagnostics"] },
]
```

This follows the documented Helix config format but was not run in this
environment.

### Zed

Out of scope for now: Zed cannot attach an arbitrary stdio language server from
`settings.json` alone — it requires a published Zed extension to register the
server for a language, which formality does not yet ship.

### Emacs, and other LSP clients

Any client that can launch a stdio server works the same way: run `fml lsp`, let
it handle `textDocument/formatting`, and keep your other servers for everything
else. The disk-rewrite behavior noted for Neovim applies anywhere — format
on/after save, then let the client re-read the file.

---

## Environment variables

| Variable                    | Description                                                           |
| :-------------------------- | :-------------------------------------------------------------------- |
| `FORMALITY_NO_UPDATE_CHECK` | Set to any value to skip the background version check.                |
| `CI`                        | Automatically suppresses the update check (set by most CI providers). |
| `GITHUB_ACTIONS`            | Also suppresses the update check.                                     |
| `XDG_CONFIG_HOME`           | Override the user-config search root (Linux / macOS).                 |

---

## License

MIT
