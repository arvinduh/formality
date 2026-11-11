# formality (`fml`)

`fml` formats and lints every file in a repo with the best tool for each
language, in one style you set once.

## Install

On Linux (x64) and macOS (Apple Silicon):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/arvinduh/formality/releases/latest/download/fml-installer.sh | sh
```

On Windows (x64, or ARM64 under emulation):

```powershell
powershell -c "irm https://github.com/arvinduh/formality/releases/latest/download/fml-installer.ps1 | iex"
```

`fml update` replaces the running `fml` with the latest release. Set
`FML_GITHUB_TOKEN` to lift GitHub's anonymous rate limit.

fml runs the tools below but does not install them. A missing tool fails its
language with the command that installs it.

## Use

```sh
fml fmt            # format the workspace
fml fmt --check    # list the files that would change
fml lint           # report lint violations
fml fix            # apply lint autofixes, then format
fml fmt --staged   # only files in the git index
fml lint --changed # only files that differ from HEAD, plus untracked ones
fml fmt src a.py   # only these files and directories
```

When violations span more than one rule, `fml lint` also prints a count per
rule, most frequent first, to plan a cleanup by rule.

`-q` prints nothing on success and only the violations on failure. `-v` logs
each language's time and `-vv` each command; `RUST_LOG` overrides both.

Exit codes: `0` clean, `1` violations or files to format, `2` fml could not run,
such as a missing tool or a bad config.

## Languages

| Language          | Extensions                 | Formatter      | Linter              |
| :---------------- | :------------------------- | :------------- | :------------------ |
| C and C++ (`cpp`) | `c`, `cc`, `cpp`, `h`, ... | `clang-format` | `clang-tidy`        |
| JavaScript and TS | `js`, `jsx`, `ts`, ...     | `biome`        | `biome`             |
| JSON              | `json`, `jsonc`            | `prettier`     |                     |
| Markdown          | `md`                       | built in       | `markdownlint-cli2` |
| Python            | `py`, `pyi`                | `ruff`         | `ruff`              |
| Rust              | `rs`                       | `rustfmt`      | `cargo clippy`      |
| Shell             | `sh`, `bash`               | `shfmt`        | `shellcheck`        |
| TOML              | `toml`                     | `taplo`        |                     |
| YAML              | `yaml`, `yml`              | `prettier`     | `yamllint`          |

fml formats Markdown itself with
[dprint-plugin-markdown](https://crates.io/crates/dprint-plugin-markdown), so a
`.prettierrc` does not apply to it. A fenced code block tagged with one of the
languages above, such as `rust` or `json`, is formatted by that language's
formatter; other blocks, and blocks that do not parse, are kept as written.

Markdown prose and YAML strings wrap at the line length. A line with nowhere to
break, such as a long URL or a comment, is left for the linter to report.

`fml fix` does not run markdownlint's fixers, since some guess wrong, such as
turning a wrapped `#123` into a heading. fml fixes five rules itself, where the
intent is clear: a fence with no language gets `text` (MD040), a bare URL gets
`<>` (MD034), a heading drops a trailing `.`, `,`, `:` or `;` (MD026), a line
that is only bold becomes a heading one level down unless it reads as a label
such as `Note: unstable` (MD036), and `#Title` alone between blank lines gets
its space (MD018).

## Configure

The workspace root is the nearest directory with `formality.toml`,
`.formality.toml` or `.git`. For each tool, fml picks the style from the first
of these that exists:

1. The workspace's `formality.toml`: fml passes its settings to the tool.
2. The tool's own config, such as `ruff.toml` or `.prettierrc`: fml passes
   nothing, so the tool reads it.
3. fml's defaults: 2-space indents, 80 columns, double quotes, LF line endings
   and backslash hard breaks in Markdown.

```toml
[global]
indent_size = 2
line_length = 80
quote_style = "double"   # or "single"
end_of_line = "lf"       # or "crlf"
ignore = ["fixtures/**"] # gitignore syntax, appended after .gitignore

[python]
indent_size = 4
```

A language table, named as in the table above, overrides `[global]` for that
language. Lint rules always come from the tool's own config, never from
`formality.toml`. When a tool has no config, fml lints with its own rule set;
for Rust, a manifest without a `[lints]` or `[workspace.lints]` table gets
clippy's pedantic group plus lints for docs, `unwrap`, imports and `allow`.

Config layers merge in order: `~/.config/formality/config.toml`, the workspace's
file, then `-c FILE`. An unknown key is an error that names its file and line.
Files the tools' own ignore files exclude, those in `.gitignore`, and those
matching patterns in `ignore` are skipped.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/architecture.md](docs/architecture.md).
