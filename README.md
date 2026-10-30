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
fml fmt              # format the workspace
fml fmt --check      # list the files that would change
fml lint             # report lint violations
fml fix              # apply lint autofixes, then format
fml fmt --staged     # only files in the git index
fml lint --changed   # only files that differ from HEAD, plus untracked ones
fml fmt src a.py     # only these files and directories
```

`-q` prints nothing on success and only the violations on failure. `-v` logs
each language's time and `-vv` each command; `RUST_LOG` overrides both.

Exit codes: `0` clean, `1` violations or files to format, `2` fml could not
run, such as a missing tool or a bad config.

## Languages

| Language          | Extensions                 | Formatter      | Linter              |
| :---------------- | :------------------------- | :------------- | :------------------ |
| C and C++ (`cpp`) | `c`, `cc`, `cpp`, `h`, ... | `clang-format` | `clang-tidy`        |
| JavaScript and TS | `js`, `jsx`, `ts`, ...     | `biome`        | `biome`             |
| JSON              | `json`, `jsonc`            | `prettier`     |                     |
| Markdown          | `md`                       | `prettier`     | `markdownlint-cli2` |
| Python            | `py`, `pyi`                | `ruff`         | `ruff`              |
| Rust              | `rs`                       | `rustfmt`      | `cargo clippy`      |
| Shell             | `sh`, `bash`               | `shfmt`        | `shellcheck`        |
| TOML              | `toml`                     | `taplo`        |                     |
| YAML              | `yaml`, `yml`              | `prettier`     | `yamllint`          |

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
quote_style = "double" # or "single"
end_of_line = "lf"     # or "crlf"

[python]
indent_size = 4
```

A language table, named as in the table above, overrides `[global]` for that
language. Lint rules always come from the tool's own config, never from
`formality.toml`.

Config layers merge in order: `~/.config/formality/config.toml`, the
workspace's file, then `-c FILE`. An unknown key is an error that names its
file and line. Files the tools' own ignore files exclude, and those in
`.gitignore`, are skipped.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/INDEX.md](docs/INDEX.md).
