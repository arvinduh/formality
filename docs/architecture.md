# Architecture

fml is one binary over five library crates. Data flows one way, and only the
binary touches the terminal, the environment or the process exit code.

```text
fml (src/) ──► engine ──► surface ──► tool
   │                         │
   ├──► discover             └──► config
   └──► config
```

| Crate          | Owns                                                      |
| :------------- | :-------------------------------------------------------- |
| `fml` (`src/`) | Arguments, the workspace root and config layers, git file |
|                | selection, output, exit codes and `fml update`.           |
| `fml-engine`   | Grouping files by language and running languages in       |
|                | parallel.                                                 |
| `fml-surface`  | One module per language: its extensions, tools, arguments |
|                | and output parsing, and the formality.toml/native/default |
|                | precedence.                                               |
| `fml-tool`     | Finding a binary on `PATH`, running it, and finding a     |
|                | tool's own config file. Knows no languages.               |
| `fml-config`   | Parsing and merging config layers and reporting unknown   |
|                | keys with their file and line. Knows no languages.        |
| `fml-discover` | Walking the workspace for files, honoring ignore files.   |

## Rules

- Libraries take the data they need from their caller and do no ambient I/O:
  no environment variables, no current directory, no printing
  (`#![deny(clippy::print_stdout, clippy::print_stderr)]`).
- Each error lives in the crate that raises it; the binary maps them to exit
  codes: `0` clean, `1` violations, `2` operational failure.
- Adding a language is one module in `crates/surface/src` and one entry in
  `surface::SURFACES`.
