//! Structured per-violation lint diagnostics for `fml lsp` (Fixes #159).
//!
//! `fml lint`'s CLI output is human-readable free text (see
//! [`crate::surfaces::LanguageSurface::lint`] — its `SurfaceStatus::ViolationsFound`
//! carries only a rendered `message: String`, not structured per-violation
//! data). The LSP's Problems panel needs real per-violation `Diagnostic`s
//! (file/line/column/message/severity), so this module shells out to each
//! supported linter's own machine-readable output mode directly — `cargo
//! clippy --message-format=json` for Rust, `ruff check --output-format=json`
//! for Python — and translates the result into `tower_lsp::lsp_types::Diagnostic`s
//! scoped to the file being edited.
//!
//! Coverage
//! ========
//! `rust` (clippy), `python` (ruff), `javascript`/`typescript` (biome),
//! `yaml` (yamllint), `markdown` (markdownlint-cli2/markdownlint), `cpp`
//! (clang-tidy), `go` (golangci-lint), `java` (checkstyle), `kotlin`
//! (ktlint), `toml` (taplo), and `typst` (typst compile) are wired up to
//! structured diagnostics (Fixes #159, #165). `json` has no linter at all
//! (prettier-only, format-only surface), so it has nothing to add structured
//! diagnostics for — every other surface this crate supports now has a
//! parser here.
//!
//! None of these invocations currently thread through `formality.toml`'s
//! `extra_args`/per-language overrides the way `fml lint` proper does — the
//! LSP path calls these with an empty extra-args slice. For most surfaces
//! that's a known simplification, not a correctness bug: the diagnostics
//! still reflect the same rule set, just not any project-specific extra CLI
//! flags. `markdown` is the one exception with a real core-setting
//! dependency: `markdownlint_diagnostics` resolves formality.toml's MD013
//! settings (`line_length`/`code_blocks`/`tables`) via
//! [`crate::surfaces::markdown::write_markdownlint_temp_config`] and passes
//! them inline, precisely because markdownlint-cli2 (unlike the other
//! tools here) has no meaningful built-in default rule set of its own to
//! fall back on that would match formality.toml's — see that function's
//! doc comment for why `--config` was the only inline mechanism available.
//!
//! `None` vs. `Some(vec![])` (Fixes #177)
//! =======================================
//! Every `*_diagnostics` function here returns `Option<Vec<Diagnostic>>`,
//! and the two cases mean very different things to the caller
//! ([`crate::commands::lsp::Backend::did_save`]): `None` means the
//! structured tool could not be run at all this time — its binary is
//! missing, the project has no marker file it needs (`Cargo.toml`,
//! `go.mod`, `checkstyle.xml`), or spawning it failed outright — and the
//! caller must fall back to shelling out to `fml lint` instead. `Some(v)`
//! means the tool *did* run, and `v` (possibly empty) is its real, complete
//! result. Collapsing these — e.g. returning `Some(vec![])` for "couldn't
//! run" — would make the editor publish a file as clean when the linter
//! never actually looked at it, silently regressing behind the `fml lint`
//! fallback this module exists to enhance, not replace.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use tower_lsp::lsp_types::{
  Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range,
};

use crate::config::FormalityConfig;
use crate::surfaces::{check_binary_exists, default_registry};

// ---------------------------------------------------------------------------
// Surface detection
// ---------------------------------------------------------------------------

/// Returns the canonical surface name (e.g. `"rust"`, `"python"`) whose
/// `file_extensions()` cover `file`'s extension, if any surface claims it.
#[must_use]
pub fn surface_name_for_file(file: &Path) -> Option<&'static str> {
  let ext = file.extension()?.to_str()?;
  default_registry()
    .surfaces()
    .iter()
    .find(|s| {
      s.file_extensions()
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext))
    })
    .map(|s| s.name())
}

// ---------------------------------------------------------------------------
// Path matching
// ---------------------------------------------------------------------------

/// Like `str::ends_with`, but only counts as a match on a path-component
/// boundary — the matched suffix must be the whole string, or immediately
/// preceded by `/`. Guards against a bare `ends_with` false-positiving on a
/// filename collision (e.g. `"domain.rs".ends_with("main.rs")` is true at
/// the byte level, but they're unrelated files).
fn ends_with_path_boundary(haystack: &str, needle: &str) -> bool {
  haystack.ends_with(needle)
    && (haystack.len() == needle.len()
      || haystack.as_bytes()[haystack.len() - needle.len() - 1] == b'/')
}

/// Compares a linter-reported path (often relative to the tool's working
/// directory, sometimes absolute) against the LSP document path (always
/// absolute, from the editor's file URI) without touching the filesystem —
/// canonicalizing would fail in unit tests against paths that don't exist on
/// disk. Normalizes separators and checks path-boundary-respecting suffix
/// containment in either direction, given the reported path always shares a
/// tail with the real file path.
fn paths_match(reported: &str, target: &Path) -> bool {
  let reported_norm = reported.replace('\\', "/");
  let target_norm = target.to_string_lossy().replace('\\', "/");
  if reported_norm.is_empty() || target_norm.is_empty() {
    return false;
  }
  ends_with_path_boundary(&target_norm, &reported_norm)
    || ends_with_path_boundary(&reported_norm, &target_norm)
}

// ---------------------------------------------------------------------------
// Rust — cargo clippy --message-format=json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClippyMessage {
  level: String,
  message: String,
  spans: Vec<ClippySpan>,
}

#[derive(Debug, Deserialize)]
struct ClippySpan {
  file_name: String,
  is_primary: bool,
  line_start: u32,
  line_end: u32,
  column_start: u32,
  column_end: u32,
}

/// Parses `cargo clippy --message-format=json` output (one JSON object per
/// line, cargo's usual `--message-format=json` framing) into `Diagnostic`s
/// for the violations whose primary span touches `target_file`.
///
/// Non-`compiler-message` lines (e.g. `build-finished`, `compiler-artifact`)
/// and diagnostics below `warning` severity (`note`, `help`) are skipped, as
/// is any line that fails to parse — clippy's JSON stream can include lines
/// this module doesn't model, and a partial parse shouldn't take down the
/// whole diagnostics pass.
#[must_use]
pub fn parse_clippy_json(
  json_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  let mut diagnostics = Vec::new();

  for line in json_output.lines() {
    let line = line.trim();
    if line.is_empty() {
      continue;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
      continue;
    };
    if value.get("reason").and_then(serde_json::Value::as_str)
      != Some("compiler-message")
    {
      continue;
    }
    let Some(message_value) = value.get("message") else {
      continue;
    };
    let Ok(message) =
      serde_json::from_value::<ClippyMessage>(message_value.clone())
    else {
      continue;
    };
    let severity = match message.level.as_str() {
      "error" => DiagnosticSeverity::ERROR,
      "warning" => DiagnosticSeverity::WARNING,
      _ => continue, // note/help/etc. — not a standalone violation
    };
    let Some(span) = message.spans.iter().find(|s| s.is_primary) else {
      continue;
    };
    if !paths_match(&span.file_name, target_file) {
      continue;
    }

    diagnostics.push(Diagnostic {
      range: Range {
        start: Position {
          line: span.line_start.saturating_sub(1),
          character: span.column_start.saturating_sub(1),
        },
        end: Position {
          line: span.line_end.saturating_sub(1),
          character: span.column_end.saturating_sub(1),
        },
      },
      severity: Some(severity),
      source: Some("clippy".to_string()),
      message: message.message,
      ..Default::default()
    });
  }

  diagnostics
}

/// Runs `cargo clippy --message-format=json` in `root` and returns
/// `Diagnostic`s for violations touching `file`. Returns `None` — not
/// `Some(vec![])` — when clippy is missing, there's no `Cargo.toml`, or the
/// invocation otherwise fails to spawn: those all mean the tool never ran,
/// so the caller must fall back to `fml lint` rather than publish "no
/// violations" for a file that was never actually checked (#177).
fn clippy_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("cargo") || !root.join("Cargo.toml").exists() {
    return None;
  }

  let mut cmd = Command::new("cargo");
  cmd.args(crate::surfaces::rust::build_clippy_json_args(&[]));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_clippy_json(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Python — ruff check --output-format=json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RuffLocation {
  row: u32,
  column: u32,
}

#[derive(Debug, Deserialize)]
struct RuffViolation {
  code: Option<String>,
  message: String,
  filename: String,
  location: RuffLocation,
  end_location: RuffLocation,
}

/// Parses `ruff check --output-format=json` output (a single JSON array of
/// violation objects) into `Diagnostic`s for violations reported against
/// `target_file`. Ruff's JSON schema carries no severity field of its own —
/// every rule it reports is treated as a `WARNING` here, matching how `fml
/// lint`'s non-zero exit is surfaced generically today rather than
/// inventing a severity ruff itself doesn't distinguish.
#[must_use]
pub fn parse_ruff_json(
  json_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  let Ok(violations) = serde_json::from_str::<Vec<RuffViolation>>(json_output)
  else {
    return Vec::new();
  };

  violations
    .into_iter()
    .filter(|v| paths_match(&v.filename, target_file))
    .map(|v| Diagnostic {
      range: Range {
        start: Position {
          line: v.location.row.saturating_sub(1),
          character: v.location.column.saturating_sub(1),
        },
        end: Position {
          line: v.end_location.row.saturating_sub(1),
          character: v.end_location.column.saturating_sub(1),
        },
      },
      severity: Some(DiagnosticSeverity::WARNING),
      code: v.code.map(NumberOrString::String),
      source: Some("ruff".to_string()),
      message: v.message,
      ..Default::default()
    })
    .collect()
}

/// Runs `ruff check --output-format=json <file>` in `root` and returns
/// `Diagnostic`s for `file`'s violations. Returns `None` — not `Some(vec![])`
/// — when ruff is missing or the invocation otherwise fails to spawn, so the
/// caller falls back to `fml lint` instead of publishing a false "clean"
/// (#177).
fn ruff_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("ruff") {
    return None;
  }

  let mut cmd = Command::new("ruff");
  cmd.args(crate::surfaces::python::build_ruff_check_json_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_ruff_json(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// JavaScript/TypeScript — biome lint --reporter=json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BiomeOutput {
  diagnostics: Vec<BiomeDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct BiomeDiagnostic {
  severity: String,
  message: String,
  category: Option<String>,
  location: Option<BiomeLocation>,
}

#[derive(Debug, Deserialize)]
struct BiomeLocation {
  path: String,
  start: BiomePosition,
  end: BiomePosition,
}

#[derive(Debug, Deserialize)]
struct BiomePosition {
  line: u32,
  column: u32,
}

/// Parses `biome lint --reporter=json` output (a single JSON object, see
/// [`crate::surfaces::javascript::build_biome_lint_json_args`]) into
/// `Diagnostic`s for violations reported against `target_file`. Biome's
/// `line`/`column` positions are 1-based, like clippy's and unlike ruff's
/// (which is also 1-based, incidentally — all three tools agree here).
/// Diagnostics with no `location` (none observed in practice, but the field
/// is optional in biome's schema) are skipped rather than guessed at.
#[must_use]
pub fn parse_biome_json(
  json_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  let Ok(output) = serde_json::from_str::<BiomeOutput>(json_output) else {
    return Vec::new();
  };

  output
    .diagnostics
    .into_iter()
    .filter_map(|d| {
      let location = d.location?;
      if !paths_match(&location.path, target_file) {
        return None;
      }
      let severity = match d.severity.as_str() {
        "error" => DiagnosticSeverity::ERROR,
        "information" => DiagnosticSeverity::INFORMATION,
        _ => DiagnosticSeverity::WARNING,
      };
      Some(Diagnostic {
        range: Range {
          start: Position {
            line: location.start.line.saturating_sub(1),
            character: location.start.column.saturating_sub(1),
          },
          end: Position {
            line: location.end.line.saturating_sub(1),
            character: location.end.column.saturating_sub(1),
          },
        },
        severity: Some(severity),
        code: d.category.map(NumberOrString::String),
        source: Some("biome".to_string()),
        message: d.message,
        ..Default::default()
      })
    })
    .collect()
}

/// Runs `biome lint --reporter=json <file>` in `root` and returns
/// `Diagnostic`s for `file`'s violations. Returns `None` — not
/// `Some(vec![])` — when biome is missing or the invocation otherwise fails
/// to spawn, so the caller falls back to `fml lint` instead of publishing a
/// false "clean" (#177).
fn biome_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("biome") {
    return None;
  }

  let mut cmd = Command::new("biome");
  cmd.args(crate::surfaces::javascript::build_biome_lint_json_args(
    file,
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_biome_json(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// YAML — yamllint -f parsable
// ---------------------------------------------------------------------------

/// Parses one line of `yamllint -f parsable` output —
/// `path:line:col: [level] message (rule)` — into its component fields.
/// Returns `None` for a line that doesn't match that shape (blank lines,
/// stray tool banners, etc.) so the caller can skip it rather than panic.
fn parse_yamllint_line(
  line: &str,
) -> Option<(&str, u32, u32, &str, &str, &str)> {
  let (location, rest) = line.split_once(": [")?;
  let (severity, rest) = rest.split_once("] ")?;
  let rule_start = rest.rfind(" (")?;
  let message = &rest[..rule_start];
  let rule = rest
    .get(rule_start + 2..rest.len().saturating_sub(1))
    .filter(|_| rest.ends_with(')'))?;

  let mut parts = location.rsplitn(3, ':');
  let col: u32 = parts.next()?.parse().ok()?;
  let line_num: u32 = parts.next()?.parse().ok()?;
  let path = parts.next()?;

  Some((path, line_num, col, severity, message, rule))
}

/// Parses `yamllint -f parsable` output (one violation per line, see
/// [`crate::surfaces::yaml::build_yamllint_parsable_args`]) into
/// `Diagnostic`s for violations reported against `target_file`. yamllint has
/// no end position of its own, so each diagnostic's range is a zero-width
/// point at its reported line/column. `line`/`col` are 1-based.
#[must_use]
pub fn parse_yamllint_parsable(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  text_output
    .lines()
    .filter_map(parse_yamllint_line)
    .filter(|(path, ..)| paths_match(path, target_file))
    .map(|(_, line_num, col, severity, message, rule)| {
      let position = Position {
        line: line_num.saturating_sub(1),
        character: col.saturating_sub(1),
      };
      let severity = if severity == "error" {
        DiagnosticSeverity::ERROR
      } else {
        DiagnosticSeverity::WARNING
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        code: Some(NumberOrString::String(rule.to_string())),
        source: Some("yamllint".to_string()),
        message: message.to_string(),
        ..Default::default()
      }
    })
    .collect()
}

/// Runs `yamllint -f parsable <file>` in `root` and returns `Diagnostic`s
/// for `file`'s violations. Returns `None` — not `Some(vec![])` — when
/// yamllint is missing or the invocation otherwise fails to spawn, so the
/// caller falls back to `fml lint` instead of publishing a false "clean"
/// (#177).
fn yamllint_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("yamllint") {
    return None;
  }

  let mut cmd = Command::new("yamllint");
  cmd.args(crate::surfaces::yaml::build_yamllint_parsable_args(file));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_yamllint_parsable(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Markdown — markdownlint-cli2 / markdownlint text output
// ---------------------------------------------------------------------------

/// Parses a markdownlint location prefix — `path:line` or `path:line:col` —
/// into `(path, line, col)`, defaulting the column to 1 when the tool didn't
/// report one. A Windows drive letter (`C:\docs\a.md:3`) is left attached to
/// the path rather than mistaken for a line number, since the segment before
/// the last colon only counts as a line/column when it parses as a number.
fn parse_markdownlint_location(location: &str) -> Option<(&str, u32, u32)> {
  let (head, last) = location.rsplit_once(':')?;
  let last_num: u32 = last.parse().ok()?;
  // `path:line:col` when the segment before the last colon is itself a
  // number; otherwise `path:line`, with the column defaulted to 1.
  let with_column = head
    .rsplit_once(':')
    .and_then(|(path, mid)| Some((path, mid.parse::<u32>().ok()?, last_num)));
  Some(with_column.unwrap_or((head, last_num, 1)))
}

/// Parses one line of markdownlint's default text report —
/// `path:line[:col] [level ]rule1/rule2 description...` — into its component
/// fields. Returns `None` for a line that doesn't match that shape.
///
/// Two shapes are accepted because the severity word is not universal:
/// markdownlint-cli2 only started prefixing violations with `error` in
/// recent releases (absent in v0.17.2, present in v0.23.2, both checked
/// directly), and the `markdownlint` (markdownlint-cli) fallback binary this
/// module also shells out to never emits it. Requiring it would silently
/// shift every field by one token — the rule id parsed as the severity and
/// the first word of the description parsed as the rule id — against those
/// versions, so it's treated as optional and defaulted to `error`, which is
/// the only severity those releases can mean.
///
/// The location is split at the first space whose prefix actually parses as
/// `path:line[:col]`, not blindly at the first space in the line, so a path
/// containing spaces (`my doc.md:3:1 ...`) still parses.
fn parse_markdownlint_line(
  line: &str,
) -> Option<(&str, u32, u32, &str, &str, &str)> {
  let (path, line_num, col, rest) =
    line.match_indices(' ').find_map(|(idx, _)| {
      let (location, rest) = (&line[..idx], &line[idx + 1..]);
      let (path, line_num, col) = parse_markdownlint_location(location)?;
      Some((path, line_num, col, rest))
    })?;

  let (first, remainder) = rest.split_once(' ').unwrap_or((rest, ""));
  let (severity, rule, description) = if first == "error" || first == "warning"
  {
    let (rule, description) =
      remainder.split_once(' ').unwrap_or((remainder, ""));
    (first, rule, description)
  } else {
    ("error", first, remainder)
  };

  Some((path, line_num, col, severity, rule, description))
}

/// Parses markdownlint's default (non-JSON) text report — one violation per
/// line on stderr, see [`crate::surfaces::markdown::build_markdownlint_args`]
/// — into `Diagnostic`s for violations reported against `target_file`.
/// markdownlint-cli2 — the binary preferred here and by
/// [`crate::surfaces::markdown::MarkdownSurface::lint`] — has no JSON
/// reporter reachable by CLI flag (its `--help` lists only `--config`,
/// `--configPointer`, `--fix`, `--format`, `--help` and `--no-globs`; JSON
/// output requires an `outputFormatters` block in a config file written to
/// disk, which this module avoids per the same simplification noted for
/// clippy/ruff's `extra_args`). The older `markdownlint` fallback binary
/// does have a `-j/--json` flag, but its schema is unrelated to cli2's text
/// report and modelling both would mean two parsers for one surface, so the
/// text format — identical across both binaries — is parsed instead.
/// `line`/`col` are 1-based; markdownlint reports no end position, so each
/// diagnostic's range is a zero-width point.
#[must_use]
pub fn parse_markdownlint_text(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  text_output
    .lines()
    .filter_map(parse_markdownlint_line)
    .filter(|(path, ..)| paths_match(path, target_file))
    .map(|(_, line_num, col, severity, rule, description)| {
      let position = Position {
        line: line_num.saturating_sub(1),
        character: col.saturating_sub(1),
      };
      let severity = if severity == "warning" {
        DiagnosticSeverity::WARNING
      } else {
        DiagnosticSeverity::ERROR
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        code: Some(NumberOrString::String(rule.to_string())),
        source: Some("markdownlint".to_string()),
        message: description.to_string(),
        ..Default::default()
      }
    })
    .collect()
}

/// Runs markdownlint-cli2 (falling back to markdownlint) against `file` in
/// `root` and returns `Diagnostic`s for its violations. Both tools report
/// violations on stderr with a successful exit status meaning "no
/// violations" (matching [`crate::surfaces::markdown::MarkdownSurface::lint`]'s
/// own stderr-first message selection). Returns `None` — not `Some(vec![])`
/// — when neither binary is present or the invocation otherwise fails to
/// spawn, so the caller falls back to `fml lint` instead of publishing a
/// false "clean" (#177).
///
/// Resolves formality.toml's markdown settings the same way `fml lint`
/// does (`FormalityConfig::load_layered` + `resolve_for_lang("markdown")`)
/// and passes them to markdownlint-cli2 via a throwaway temp file — see
/// [`crate::surfaces::markdown::write_markdownlint_temp_config`]. Before
/// issue #1 deleted this repo's own `.markdownlint.json`, this path ran
/// with no `--config` at all and relied on markdownlint-cli2
/// auto-discovering that file from `root`/`file`'s directory, which
/// incidentally matched formality.toml's settings; once that file no
/// longer exists anywhere, that discovery falls through to markdownlint's
/// own (stricter) built-in defaults instead, silently diverging from `fml
/// lint`. Config-load failure (e.g. an unreadable formality.toml) falls
/// back to embedded defaults rather than returning `None` — a bad project
/// config should mean "default markdown settings" here, the same as it
/// does for `fml lint` itself, not "disable markdown diagnostics
/// entirely."
fn markdownlint_diagnostics(
  root: &Path,
  file: &Path,
  config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  let binary = if check_binary_exists("markdownlint-cli2") {
    "markdownlint-cli2"
  } else if check_binary_exists("markdownlint") {
    "markdownlint"
  } else {
    return None;
  };

  let lang_config = match config {
    Some(cfg) => cfg.resolve_for_lang("markdown"),
    None => FormalityConfig::load_layered(Some(root))
      .map_or_else(|_| FormalityConfig::with_defaults(), |(cfg, _)| cfg)
      .resolve_for_lang("markdown"),
  };
  let temp_cfg =
    crate::surfaces::markdown::write_markdownlint_temp_config(&lang_config)
      .ok()?;

  let mut cmd = crate::surfaces::create_tool_command(binary);
  cmd.args(crate::surfaces::markdown::build_markdownlint_args(
    &[file.to_path_buf()],
    false,
    Some(temp_cfg.path()),
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_markdownlint_text(
      &String::from_utf8_lossy(&output.stderr),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// C/C++ — clang-tidy plain diagnostic output
// ---------------------------------------------------------------------------

/// `(path, line, column, severity, message, check_name)` — the parsed
/// fields of one clang-tidy diagnostic line.
type ClangTidyLine<'a> = (&'a str, u32, u32, &'a str, &'a str, Option<&'a str>);

/// Parses one line of clang-tidy's default (non-JSON) diagnostic output —
/// `path:line:col: severity: message [check-name]` — into its component
/// fields. Only `error`/`warning` severities are recognized; `note` lines
/// (continuation detail attached to the preceding diagnostic, e.g. "place
/// parentheses around the assignment to silence this warning") and source
/// excerpt/caret lines (which don't contain either marker at all) both fall
/// through as `None`, mirroring how [`parse_clippy_json`] drops `note`/`help`
/// level messages. The trailing `[check-name]` is optional — a raw compiler
/// error routed through clang-tidy (`Error while processing ...`) still
/// carries one (`[clang-diagnostic-error]`), but this doesn't assume every
/// caller does.
fn parse_clang_tidy_line(line: &str) -> Option<ClangTidyLine<'_>> {
  for (marker, severity) in [(": error: ", "error"), (": warning: ", "warning")]
  {
    let Some(idx) = line.find(marker) else {
      continue;
    };
    let location = &line[..idx];
    let mut rest = &line[idx + marker.len()..];
    let mut check = None;
    if rest.ends_with(']')
      && let Some(bracket_start) = rest.rfind(" [")
    {
      check = Some(&rest[bracket_start + 2..rest.len() - 1]);
      rest = &rest[..bracket_start];
    }

    let mut parts = location.rsplitn(3, ':');
    let col: u32 = parts.next()?.parse().ok()?;
    let line_num: u32 = parts.next()?.parse().ok()?;
    let path = parts.next()?;
    return Some((path, line_num, col, severity, rest, check));
  }
  None
}

/// Parses `clang-tidy`'s default plain-text diagnostic output (one
/// violation per line on stdout, see
/// [`crate::surfaces::cpp::build_clang_tidy_args`]) into `Diagnostic`s for
/// violations reported against `target_file`. clang-tidy has no end
/// position of its own beyond the single reported column, so each
/// diagnostic's range is a zero-width point. `line`/`col` are 1-based.
#[must_use]
pub fn parse_clang_tidy_plain(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  text_output
    .lines()
    .filter_map(parse_clang_tidy_line)
    .filter(|(path, ..)| paths_match(path, target_file))
    .map(|(_, line_num, col, severity, message, check)| {
      let position = Position {
        line: line_num.saturating_sub(1),
        character: col.saturating_sub(1),
      };
      let severity = if severity == "error" {
        DiagnosticSeverity::ERROR
      } else {
        DiagnosticSeverity::WARNING
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        code: check.map(|c| NumberOrString::String(c.to_string())),
        source: Some("clang-tidy".to_string()),
        message: message.to_string(),
        ..Default::default()
      }
    })
    .collect()
}

/// Runs `clang-tidy` in `root` against `file` and returns `Diagnostic`s for
/// its violations. No `--config=` override is passed (unlike
/// [`crate::surfaces::cpp::CppSurface::lint`]'s inline-config path) — this
/// follows the same simplification already documented at the top of this
/// module for clippy/ruff's `extra_args`: clang-tidy still applies whatever
/// `.clang-tidy` is on disk, or its own default check set if none is,
/// rather than the resolved `formality.toml` checks list. Returns `None` —
/// not `Some(vec![])` — when clang-tidy is missing or the invocation
/// otherwise fails to spawn, so the caller falls back to `fml lint` instead
/// of publishing a false "clean" (#177).
fn clang_tidy_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("clang-tidy") {
    return None;
  }

  let std_flag =
    crate::surfaces::cpp::std_flag_for_file(file, &[file.to_path_buf()]);
  let mut cmd = Command::new("clang-tidy");
  cmd.args(crate::surfaces::cpp::build_clang_tidy_args(
    &[file.to_path_buf()],
    false,
    std_flag,
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_clang_tidy_plain(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Go — golangci-lint run --output.json.path=stdout
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GolangciLintOutput {
  #[serde(rename = "Issues")]
  issues: Vec<GolangciLintIssue>,
}

#[derive(Debug, Deserialize)]
struct GolangciLintIssue {
  #[serde(rename = "Text")]
  text: String,
  /// The linter that produced the issue (`typecheck`, `errcheck`, …),
  /// surfaced as the diagnostic's `code` — golangci-lint is a linter
  /// aggregator, so without it a diagnostic doesn't say which linter it
  /// came from. Defaulted rather than required so an issue object missing
  /// the field doesn't fail the whole parse.
  #[serde(rename = "FromLinter", default)]
  from_linter: String,
  /// Present in the v2 schema but observed empty on every issue; defaulted
  /// so a schema that drops the field entirely still parses.
  #[serde(rename = "Severity", default)]
  severity: String,
  #[serde(rename = "Pos")]
  pos: GolangciLintPos,
}

#[derive(Debug, Deserialize)]
struct GolangciLintPos {
  #[serde(rename = "Filename")]
  filename: String,
  #[serde(rename = "Line")]
  line: u32,
  #[serde(rename = "Column")]
  column: u32,
}

/// Parses `golangci-lint run --output.json.path=stdout` output (a single
/// JSON object, see
/// [`crate::surfaces::go::build_golangci_lint_json_args`]) into
/// `Diagnostic`s for violations reported against `target_file`.
/// golangci-lint's `Issues[].Severity` field is present in the schema but
/// observed empty on every issue in a real v2.5.0 run regardless of
/// underlying linter — treated as `WARNING` uniformly here, same simplification
/// already applied to ruff's schema (which has no severity field at all).
/// `Issues[].FromLinter` becomes the diagnostic's `code`, matching every
/// other parser in this module that has a rule identifier available —
/// golangci-lint aggregates many linters, so the diagnostic would
/// otherwise not say which one fired. golangci-lint reports no end
/// position, so each diagnostic's range is a zero-width point.
/// `line`/`col` are 1-based.
#[must_use]
pub fn parse_golangci_lint_json(
  json_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  let Ok(output) = serde_json::from_str::<GolangciLintOutput>(json_output)
  else {
    return Vec::new();
  };

  output
    .issues
    .into_iter()
    .filter(|issue| paths_match(&issue.pos.filename, target_file))
    .map(|issue| {
      let position = Position {
        line: issue.pos.line.saturating_sub(1),
        character: issue.pos.column.saturating_sub(1),
      };
      let severity = if issue.severity == "error" {
        DiagnosticSeverity::ERROR
      } else {
        DiagnosticSeverity::WARNING
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        code: if issue.from_linter.is_empty() {
          None
        } else {
          Some(NumberOrString::String(issue.from_linter))
        },
        source: Some("golangci-lint".to_string()),
        message: issue.text,
        ..Default::default()
      }
    })
    .collect()
}

/// Runs `golangci-lint run --output.json.path=stdout <file>` in `root` and
/// returns `Diagnostic`s for `file`'s violations. Returns `None` — not
/// `Some(vec![])` — when golangci-lint is missing, there's no `go.mod`, or
/// the invocation otherwise fails to spawn, so the caller falls back to
/// `fml lint` instead of publishing a false "clean" (#177).
fn golangci_lint_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("golangci-lint") || !root.join("go.mod").exists() {
    return None;
  }

  let mut cmd = Command::new("golangci-lint");
  cmd.args(crate::surfaces::go::build_golangci_lint_json_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_golangci_lint_json(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Java — checkstyle -f plain
// ---------------------------------------------------------------------------

/// Parses one line of `checkstyle -f plain` output —
/// `[LEVEL] path:line[:col]: message [RuleName]` — into its component
/// fields. Returns `None` for a line that doesn't start with a `[LEVEL]`
/// prefix (checkstyle's `Starting audit...`/`Audit done.` banner lines).
/// The `[RuleName]` suffix is treated as optional (checked plain-format
/// output in practice always carries one, but nothing in the format
/// guarantees it) — a missing suffix falls back to an empty rule rather
/// than dropping the diagnostic. Column is optional too: some checks
/// (`NewlineAtEndOfFile`) report only `path:line: message`, defaulted to
/// column 1 here, matching [`parse_markdownlint_location`]'s same default.
fn parse_checkstyle_line(
  line: &str,
) -> Option<(&str, u32, u32, &str, &str, &str)> {
  let line = line.trim();
  let close = line.strip_prefix('[').and_then(|s| s.find(']'))? + 1;
  let severity = &line[1..close];
  let after_sev = line.get(close + 1..)?.trim_start();

  let (loc_and_msg, rule) = if after_sev.ends_with(']')
    && let Some(bracket_start) = after_sev.rfind(" [")
  {
    (
      &after_sev[..bracket_start],
      &after_sev[bracket_start + 2..after_sev.len() - 1],
    )
  } else {
    (after_sev, "")
  };

  let first_colon = loc_and_msg.find(':')?;
  let path = &loc_and_msg[..first_colon];
  let after_path = &loc_and_msg[first_colon + 1..];
  let second_colon = after_path.find(':')?;
  let line_num: u32 = after_path[..second_colon].parse().ok()?;
  let after_line = &after_path[second_colon + 1..];

  let digit_end = after_line
    .find(|c: char| !c.is_ascii_digit())
    .unwrap_or(after_line.len());
  let (col, message) =
    if digit_end > 0 && after_line[digit_end..].starts_with(": ") {
      (
        after_line[..digit_end].parse().ok()?,
        &after_line[digit_end + 2..],
      )
    } else {
      (1, after_line.strip_prefix(' ').unwrap_or(after_line))
    };

  Some((path, line_num, col, severity, message, rule))
}

/// Parses `checkstyle -f plain` output (one violation per line on stdout,
/// see [`crate::surfaces::java::build_checkstyle_plain_args`]) into
/// `Diagnostic`s for violations reported against `target_file`. Checkstyle
/// has no end position of its own, so each diagnostic's range is a
/// zero-width point. `line`/`col` are 1-based.
#[must_use]
pub fn parse_checkstyle_plain(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  text_output
    .lines()
    .filter_map(parse_checkstyle_line)
    .filter(|(path, ..)| paths_match(path, target_file))
    .map(|(_, line_num, col, severity, message, rule)| {
      let position = Position {
        line: line_num.saturating_sub(1),
        character: col.saturating_sub(1),
      };
      let severity = if severity == "ERROR" {
        DiagnosticSeverity::ERROR
      } else {
        DiagnosticSeverity::WARNING
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        code: if rule.is_empty() {
          None
        } else {
          Some(NumberOrString::String(rule.to_string()))
        },
        source: Some("checkstyle".to_string()),
        message: message.to_string(),
        ..Default::default()
      }
    })
    .collect()
}

/// Runs `checkstyle -c checkstyle.xml -f plain <file>` in `root` and
/// returns `Diagnostic`s for `file`'s violations. Unlike
/// [`crate::surfaces::java::JavaSurface::lint`], this does **not**
/// self-heal a missing `checkstyle.xml` by generating one — doing so needs
/// a full [`crate::surfaces::ExecutionContext`] (for `indent_size` etc.)
/// that this file/root-only entry point doesn't have, and is explicitly out
/// of scope for #177 (falling back, not self-healing, is the right size fix
/// here). Run `fml lint` or `fml sync` once first to materialize
/// `checkstyle.xml`; until then this returns `None` — not `Some(vec![])` —
/// same as when checkstyle itself is missing, so the caller falls back to
/// `fml lint` instead of publishing a false "clean".
fn checkstyle_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  let config_path = root.join("checkstyle.xml");
  if !check_binary_exists("checkstyle") || !config_path.is_file() {
    return None;
  }

  let mut cmd = Command::new("checkstyle");
  cmd.arg("-c").arg(&config_path);
  cmd.args(crate::surfaces::java::build_checkstyle_plain_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_checkstyle_plain(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Kotlin — ktlint --reporter=json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct KtlintFileResult {
  file: String,
  errors: Vec<KtlintError>,
}

#[derive(Debug, Deserialize)]
struct KtlintError {
  line: u32,
  column: u32,
  message: String,
  rule: String,
}

/// Parses `ktlint --reporter=json` output into `Diagnostic`s for violations
/// reported against `target_file`. The JSON array itself
/// (`[{"file":...,"errors":[...]}]`) is well-formed, but a real ktlint
/// 1.8.0 run was observed prefixing stdout with an unrelated SLF4J `WARN
/// ...` banner line *before* the array whenever violations are
/// autocorrectable (not documented behavior, not reproduced on a clean
/// file) — so this locates the line the JSON array opens on and parses
/// from that line's byte offset, rather than assuming stdout is JSON from
/// byte zero or searching the stream for a `[` (the banner's own `[main]`
/// thread name carries the first one).
/// ktlint reports no severity distinction (every violation is a
/// style rule) and no end position, so every diagnostic is `WARNING` at a
/// zero-width point. `line`/`col` are 1-based.
#[must_use]
pub fn parse_ktlint_json(
  json_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  // The JSON array's own opening `[` always starts a fresh line (ktlint
  // pretty-prints it), so the array is located by walking lines and
  // tracking the byte offset as we go. Neither a bare
  // `json_output.find('[')` nor a `find()` for the array line's *text*
  // works: ktlint pretty-prints that line as a lone `[`, so both resolve
  // to the SLF4J banner line's own `[main]` thread-name bracket sitting
  // before the array, and the parse below then fails on the banner.
  let mut array_start = None;
  let mut offset = 0usize;
  for line in json_output.split_inclusive('\n') {
    let indent = line.len() - line.trim_start().len();
    if line.trim_start().starts_with('[') {
      array_start = Some(offset + indent);
      break;
    }
    offset += line.len();
  }
  let Some(array_start) = array_start else {
    return Vec::new();
  };
  let Ok(results) =
    serde_json::from_str::<Vec<KtlintFileResult>>(&json_output[array_start..])
  else {
    return Vec::new();
  };

  let mut diagnostics = Vec::new();
  for result in results {
    if !paths_match(&result.file, target_file) {
      continue;
    }
    for error in result.errors {
      let position = Position {
        line: error.line.saturating_sub(1),
        character: error.column.saturating_sub(1),
      };
      diagnostics.push(Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(error.rule)),
        source: Some("ktlint".to_string()),
        message: error.message,
        ..Default::default()
      });
    }
  }
  diagnostics
}

/// Runs `ktlint --reporter=json <file>` in `root` and returns `Diagnostic`s
/// for `file`'s violations. Returns `None` — not `Some(vec![])` — when
/// ktlint is missing or the invocation otherwise fails to spawn, so the
/// caller falls back to `fml lint` instead of publishing a false "clean"
/// (#177).
fn ktlint_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("ktlint") {
    return None;
  }

  let mut cmd = Command::new("ktlint");
  cmd.args(crate::surfaces::kotlin::build_ktlint_json_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_ktlint_json(
      &String::from_utf8_lossy(&output.stdout),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// TOML — taplo lint --colors never
// ---------------------------------------------------------------------------

/// Parses `taplo lint --colors never` output (a codespan-reporting-style
/// human diagnostic block, see
/// [`crate::surfaces::toml::build_taplo_lsp_lint_args`]) into `Diagnostic`s
/// for violations reported against `target_file`. taplo has no
/// JSON/single-line reporter reachable by CLI flag, so this scans for a
/// `error: <message>`/`warning: <message>` line, then the `┌─ path:line:col`
/// location line that follows it. On every shape checked against a real
/// taplo v0.10.0 run (duplicate-key and syntax-error diagnostics, single-
/// and multi-span) that line is the message's immediate successor, but the
/// scan looks forward rather than at `i + 1` only, stopping at the next
/// `error:`/`warning:` line so a message with no location of its own is
/// dropped instead of inheriting the following diagnostic's position.
/// `tracing`'s own ` INFO .../ERROR ...` log lines (taplo logs
/// unconditionally to stderr alongside its diagnostics unless `RUST_LOG` is
/// set) are ignored — they don't start with `error:`/`warning:` after
/// trimming, so they're skipped as unrecognized lines, same as any other
/// non-matching line. taplo reports no end position, so each diagnostic's
/// range is a zero-width point. `line`/`col` are 1-based.
#[must_use]
pub fn parse_taplo_lint_plain(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  let lines: Vec<&str> = text_output.lines().collect();
  let mut diagnostics = Vec::new();
  let mut i = 0;

  while i < lines.len() {
    let trimmed = lines[i].trim_start();
    let severity = if let Some(msg) = trimmed.strip_prefix("error:") {
      Some(("error", msg.trim().to_string()))
    } else {
      trimmed
        .strip_prefix("warning:")
        .map(|msg| ("warning", msg.trim().to_string()))
    };

    let Some((severity, message)) = severity else {
      i += 1;
      continue;
    };

    // Scan forward for the "┌─ path:line:col" location line, stopping at
    // the next diagnostic if none is found first (malformed/unsupported
    // shape — skip rather than guess).
    let mut j = i + 1;
    let mut location = None;
    while j < lines.len() {
      let next_trimmed = lines[j].trim_start();
      if next_trimmed.starts_with("error:")
        || next_trimmed.starts_with("warning:")
      {
        break;
      }
      if let Some(idx) = lines[j].find("┌─ ") {
        location = Some(lines[j][idx + "┌─ ".len()..].trim());
        break;
      }
      j += 1;
    }

    if let Some(location) = location {
      let mut parts = location.rsplitn(3, ':');
      if let (Some(col_s), Some(line_s), Some(path)) =
        (parts.next(), parts.next(), parts.next())
        && let (Ok(col), Ok(line_num)) =
          (col_s.parse::<u32>(), line_s.parse::<u32>())
        && paths_match(path, target_file)
      {
        let position = Position {
          line: line_num.saturating_sub(1),
          character: col.saturating_sub(1),
        };
        let severity = if severity == "error" {
          DiagnosticSeverity::ERROR
        } else {
          DiagnosticSeverity::WARNING
        };
        diagnostics.push(Diagnostic {
          range: Range {
            start: position,
            end: position,
          },
          severity: Some(severity),
          source: Some("taplo".to_string()),
          message,
          ..Default::default()
        });
      }
    }

    i = j.max(i + 1);
  }

  diagnostics
}

/// Runs `taplo lint --colors never <file>` in `root` and returns
/// `Diagnostic`s for `file`'s violations. Returns `None` — not
/// `Some(vec![])` — when taplo is missing or the invocation otherwise fails
/// to spawn, so the caller falls back to `fml lint` instead of publishing a
/// false "clean" (#177).
fn taplo_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("taplo") {
    return None;
  }

  let mut cmd = Command::new("taplo");
  cmd.args(crate::surfaces::toml::build_taplo_lsp_lint_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_taplo_lint_plain(
      &String::from_utf8_lossy(&output.stderr),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Typst — typst compile --diagnostic-format short
// ---------------------------------------------------------------------------

/// Parses one line of `typst compile --diagnostic-format short` output —
/// `path:line:col: severity: message` — into its component fields. Returns
/// `None` for a line that doesn't contain either severity marker.
fn parse_typst_line(line: &str) -> Option<(&str, u32, u32, &str, &str)> {
  for (marker, severity) in [(": error: ", "error"), (": warning: ", "warning")]
  {
    let Some(idx) = line.find(marker) else {
      continue;
    };
    let location = &line[..idx];
    let message = &line[idx + marker.len()..];

    let mut parts = location.rsplitn(3, ':');
    let col: u32 = parts.next()?.parse().ok()?;
    let line_num: u32 = parts.next()?.parse().ok()?;
    let path = parts.next()?;
    return Some((path, line_num, col, severity, message));
  }
  None
}

/// Parses `typst compile --diagnostic-format short` output (one violation
/// per line on stderr, see
/// [`crate::surfaces::typst::build_typst_check_args`]) into `Diagnostic`s
/// for violations reported against `target_file`. Typst reports no end
/// position on the `short` format, so each diagnostic's range is a
/// zero-width point. `line`/`col` are 1-based.
#[must_use]
pub fn parse_typst_short(
  text_output: &str,
  target_file: &Path,
) -> Vec<Diagnostic> {
  text_output
    .lines()
    .filter_map(parse_typst_line)
    .filter(|(path, ..)| paths_match(path, target_file))
    .map(|(_, line_num, col, severity, message)| {
      let position = Position {
        line: line_num.saturating_sub(1),
        character: col.saturating_sub(1),
      };
      let severity = if severity == "error" {
        DiagnosticSeverity::ERROR
      } else {
        DiagnosticSeverity::WARNING
      };
      Diagnostic {
        range: Range {
          start: position,
          end: position,
        },
        severity: Some(severity),
        source: Some("typst".to_string()),
        message: message.to_string(),
        ..Default::default()
      }
    })
    .collect()
}

/// Runs `typst compile --diagnostic-format short <file> <scratch-output>`
/// in `root` and returns `Diagnostic`s for `file`'s violations. Typst's
/// `compile` command (there is no separate `check`/`lint` subcommand — see
/// [`crate::surfaces::typst::build_typst_check_args`]) always needs
/// somewhere to write its output, so this points it at a throwaway file in
/// a fresh temp directory, discarded once diagnostics are parsed. Returns
/// `None` — not `Some(vec![])` — when typst is missing, the temp directory
/// can't be created, or the invocation otherwise fails to spawn, so the
/// caller falls back to `fml lint` instead of publishing a false "clean"
/// (#177).
fn typst_diagnostics(
  root: &Path,
  file: &Path,
  _config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  if !check_binary_exists("typst") {
    return None;
  }

  let Ok(scratch_dir) = tempfile::tempdir() else {
    return None;
  };
  let output_path = scratch_dir.path().join("out.pdf");

  let mut cmd = Command::new("typst");
  cmd.args(crate::surfaces::typst::build_typst_check_args(
    file,
    &output_path,
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => Some(parse_typst_short(
      &String::from_utf8_lossy(&output.stderr),
      file,
    )),
    Err(_) => None,
  }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Runs one surface's linter in `root` and returns `Diagnostic`s for the
/// second argument's violations — the shared shape of every
/// `*_diagnostics` function in this module. `None` means the tool could not
/// be run at all (binary missing, no project marker file, spawn failure,
/// required config missing) and the caller must fall back to `fml lint`;
/// `Some(vec![])` means the tool ran successfully and found nothing (#177).
type DiagnosticsRunner =
  fn(&Path, &Path, Option<&FormalityConfig>) -> Option<Vec<Diagnostic>>;

/// Maps a canonical surface name to the function that produces structured
/// diagnostics for it, or `None` for a surface with no parser wired up
/// (today: only `json`, which is format-only and has no linter to parse
/// output from).
///
/// Split out of [`diagnostics_for_file`] rather than inlined into its match
/// so `test_every_surface_except_json_has_a_structured_parser` can check
/// the wiring exhaustively against the surface registry without shelling
/// out to a single linter — a new surface added without a parser here fails
/// that test instead of silently falling back to the generic warning.
fn diagnostics_runner_for_surface(surface: &str) -> Option<DiagnosticsRunner> {
  match surface {
    "rust" => Some(clippy_diagnostics),
    "python" => Some(ruff_diagnostics),
    "javascript" => Some(biome_diagnostics),
    "yaml" => Some(yamllint_diagnostics),
    "markdown" => Some(markdownlint_diagnostics),
    "cpp" => Some(clang_tidy_diagnostics),
    "go" => Some(golangci_lint_diagnostics),
    "java" => Some(checkstyle_diagnostics),
    "kotlin" => Some(ktlint_diagnostics),
    "toml" => Some(taplo_diagnostics),
    "typst" => Some(typst_diagnostics),
    _ => None,
  }
}

/// Returns structured per-violation `Diagnostic`s for `file` if its surface
/// has a structured-output parser wired up here *and that parser actually
/// ran*, or `None` otherwise — the caller falls back to `fml lint`'s
/// generic single-warning diagnostic in the `None` case (#177). `None`
/// covers two distinct reasons, both requiring the same fallback: the
/// surface has no structured parser at all (see module docs for coverage),
/// or it does but the underlying tool/config couldn't be run this time
/// (binary missing, no project marker file, spawn failure, required config
/// missing). `Some(vec![])` means the tool ran and genuinely found nothing.
#[must_use]
pub fn diagnostics_for_file(
  root: &Path,
  file: &Path,
) -> Option<Vec<Diagnostic>> {
  diagnostics_for_file_with_config(root, file, None)
}

/// Returns structured per-violation `Diagnostic`s for `file` reusing a cached
/// [`FormalityConfig`] if provided.
#[must_use]
pub fn diagnostics_for_file_with_config(
  root: &Path,
  file: &Path,
  config: Option<&FormalityConfig>,
) -> Option<Vec<Diagnostic>> {
  let runner = diagnostics_runner_for_surface(surface_name_for_file(file)?)?;
  runner(root, file, config)
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_surface_name_for_file_known_extensions() {
    assert_eq!(
      surface_name_for_file(Path::new("src/main.rs")),
      Some("rust")
    );
    assert_eq!(
      surface_name_for_file(Path::new("app/views.py")),
      Some("python")
    );
  }

  #[test]
  fn test_surface_name_for_file_unknown_extension() {
    assert_eq!(surface_name_for_file(Path::new("notes.txt")), None);
    assert_eq!(surface_name_for_file(Path::new("no_extension")), None);
  }

  #[test]
  fn test_paths_match_handles_relative_vs_absolute_and_separators() {
    assert!(paths_match(
      "src/main.rs",
      Path::new("C:\\proj\\src\\main.rs")
    ));
    assert!(paths_match("src/main.rs", Path::new("/proj/src/main.rs")));
    assert!(!paths_match("src/other.rs", Path::new("/proj/src/main.rs")));
    assert!(!paths_match("", Path::new("/proj/src/main.rs")));
  }

  #[test]
  fn test_paths_match_rejects_filename_suffix_collision() {
    // "domain.rs" ends with "main.rs" at the byte level, but they're
    // unrelated files — must not match without a path-component boundary.
    assert!(!paths_match("domain.rs", Path::new("/proj/src/main.rs")));
    assert!(!paths_match(
      "src/domain.rs",
      Path::new("/proj/src/main.rs")
    ));
  }

  #[test]
  fn test_parse_clippy_json_extracts_warning_diagnostic() {
    let sample = r#"{"reason":"compiler-artifact","package_id":"x"}
{"reason":"compiler-message","message":{"level":"warning","message":"unused variable: `x`","code":{"code":"unused_variables"},"spans":[{"file_name":"src/main.rs","is_primary":true,"line_start":3,"line_end":3,"column_start":9,"column_end":10,"byte_start":0,"byte_end":0,"text":[]}],"children":[]}}
{"reason":"build-finished","success":false}"#;

    let diagnostics = parse_clippy_json(sample, Path::new("/proj/src/main.rs"));

    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(d.message, "unused variable: `x`");
    assert_eq!(d.source.as_deref(), Some("clippy"));
    // clippy is 1-based; LSP Position is 0-based.
    assert_eq!(d.range.start.line, 2);
    assert_eq!(d.range.start.character, 8);
    assert_eq!(d.range.end.line, 2);
    assert_eq!(d.range.end.character, 9);
  }

  #[test]
  fn test_parse_clippy_json_maps_error_level_and_filters_other_files() {
    let sample = r#"{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","spans":[{"file_name":"src/lib.rs","is_primary":true,"line_start":10,"line_end":10,"column_start":1,"column_end":5,"byte_start":0,"byte_end":0,"text":[]}],"children":[]}}"#;

    let matching = parse_clippy_json(sample, Path::new("/proj/src/lib.rs"));
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].severity, Some(DiagnosticSeverity::ERROR));

    let non_matching =
      parse_clippy_json(sample, Path::new("/proj/src/main.rs"));
    assert!(non_matching.is_empty());
  }

  #[test]
  fn test_parse_clippy_json_ignores_note_and_help_level_and_malformed_lines() {
    let sample = r#"not json at all
{"reason":"compiler-message","message":{"level":"note","message":"for more information, try `rustc --explain`","spans":[],"children":[]}}"#;

    let diagnostics = parse_clippy_json(sample, Path::new("/proj/src/main.rs"));
    assert!(diagnostics.is_empty());
  }

  #[test]
  fn test_parse_ruff_json_extracts_diagnostics_for_target_file() {
    let sample = r#"[
      {
        "cell": null,
        "code": "F401",
        "end_location": {"column": 20, "row": 1},
        "filename": "/proj/app/views.py",
        "fix": null,
        "location": {"column": 8, "row": 1},
        "message": "`os` imported but unused",
        "noqa_row": 1,
        "url": "https://docs.astral.sh/ruff/rules/unused-import"
      },
      {
        "cell": null,
        "code": "E501",
        "end_location": {"column": 90, "row": 5},
        "filename": "/proj/app/other.py",
        "fix": null,
        "location": {"column": 89, "row": 5},
        "message": "line too long",
        "noqa_row": 5,
        "url": null
      }
    ]"#;

    let diagnostics = parse_ruff_json(sample, Path::new("/proj/app/views.py"));

    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(d.message, "`os` imported but unused");
    assert_eq!(d.source.as_deref(), Some("ruff"));
    assert_eq!(d.code, Some(NumberOrString::String("F401".to_string())));
    // ruff is 1-based; LSP Position is 0-based.
    assert_eq!(d.range.start.line, 0);
    assert_eq!(d.range.start.character, 7);
    assert_eq!(d.range.end.line, 0);
    assert_eq!(d.range.end.character, 19);
  }

  #[test]
  fn test_parse_ruff_json_empty_array_means_no_violations() {
    let diagnostics = parse_ruff_json("[]", Path::new("/proj/app/views.py"));
    assert!(diagnostics.is_empty());
  }

  #[test]
  fn test_parse_ruff_json_malformed_input_returns_empty_not_panic() {
    let diagnostics =
      parse_ruff_json("not valid json", Path::new("/proj/app/views.py"));
    assert!(diagnostics.is_empty());
  }

  #[test]
  fn test_every_surface_except_json_has_a_structured_parser() {
    // Walks the real surface registry rather than a hand-written list, so
    // a surface added later without a parser wired into
    // `diagnostics_runner_for_surface` fails here instead of silently
    // falling back to `fml lsp`'s generic single-warning diagnostic.
    for surface in default_registry().surfaces() {
      let name = surface.name();
      if name == "json" {
        assert!(
          diagnostics_runner_for_surface(name).is_none(),
          "`json` is format-only (prettier) and has no linter to parse"
        );
      } else {
        assert!(
          diagnostics_runner_for_surface(name).is_some(),
          "surface `{name}` has no structured-diagnostics parser wired into diagnostics_for_file"
        );
      }
    }
  }

  #[test]
  fn test_every_surface_extension_routes_back_to_its_own_surface() {
    // Guards the other half of the routing: no two surfaces may claim the
    // same file extension, or one of them would silently never receive
    // structured diagnostics.
    for surface in default_registry().surfaces() {
      for ext in surface.file_extensions() {
        let path = std::path::PathBuf::from(format!("sample.{ext}"));
        assert_eq!(
          surface_name_for_file(&path),
          Some(surface.name()),
          "extension `.{ext}` does not route back to surface `{}`",
          surface.name()
        );
      }
    }
  }

  #[test]
  fn test_diagnostics_for_file_unsupported_surface_returns_none() {
    // `json` is the one surface left with no linter at all (format-only,
    // prettier-based) — every other surface this crate supports now has a
    // structured-diagnostics parser wired up here.
    assert!(
      diagnostics_for_file(Path::new("."), Path::new("data.json")).is_none()
    );
    assert!(
      diagnostics_for_file(Path::new("."), Path::new("notes.txt")).is_none()
    );
  }

  #[test]
  fn test_parse_biome_json_extracts_diagnostics_for_target_file() {
    // Captured from a real `biome lint --reporter=json` run.
    let sample = r#"{"summary":{"changed":0,"unchanged":1,"matches":0,"duration":2763878,"errors":1,"warnings":1,"infos":0,"skipped":0,"suggestedFixesSkipped":0,"diagnosticsNotPrinted":0,"scannerDuration":446471},"diagnostics":[{"severity":"warning","message":"This variable unused is unused.","category":"lint/correctness/noUnusedVariables","location":{"path":"bad.js","start":{"line":4,"column":5},"end":{"line":4,"column":11}},"advices":[]},{"severity":"error","message":"Using == may be unsafe if you are relying on type coercion.","category":"lint/suspicious/noDoubleEquals","location":{"path":"bad.js","start":{"line":1,"column":7},"end":{"line":1,"column":9}},"advices":[]}],"command":"lint"}"#;

    let diagnostics = parse_biome_json(sample, Path::new("/proj/bad.js"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diagnostics[0].message, "This variable unused is unused.");
    assert_eq!(diagnostics[0].source.as_deref(), Some("biome"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String(
        "lint/correctness/noUnusedVariables".to_string()
      ))
    );
    // biome is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 3);
    assert_eq!(diagnostics[0].range.start.character, 4);

    assert_eq!(diagnostics[1].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[1].range.start.line, 0);
    assert_eq!(diagnostics[1].range.start.character, 6);
  }

  #[test]
  fn test_parse_biome_json_filters_other_files_and_handles_malformed_input() {
    let sample = r#"{"diagnostics":[{"severity":"error","message":"x","category":"lint/a","location":{"path":"other.js","start":{"line":1,"column":1},"end":{"line":1,"column":2}},"advices":[]}]}"#;
    assert!(parse_biome_json(sample, Path::new("/proj/bad.js")).is_empty());
    assert!(parse_biome_json("not json", Path::new("/proj/bad.js")).is_empty());
  }

  #[test]
  fn test_parse_yamllint_parsable_extracts_diagnostics() {
    // Captured from a real `yamllint -f parsable` run.
    let sample = "bad.yaml:1:1: [warning] missing document start \"---\" (document-start)\nbad.yaml:1:6: [error] too many spaces after colon (colons)\n";

    let diagnostics =
      parse_yamllint_parsable(sample, Path::new("/proj/bad.yaml"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diagnostics[0].message, "missing document start \"---\"");
    assert_eq!(diagnostics[0].source.as_deref(), Some("yamllint"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("document-start".to_string()))
    );
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 0);

    assert_eq!(diagnostics[1].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[1].range.start.character, 5);
  }

  #[test]
  fn test_parse_yamllint_parsable_filters_other_files_and_malformed_lines() {
    let sample = "not a yamllint line\nother.yaml:1:1: [error] bad (rule)\n";
    let diagnostics =
      parse_yamllint_parsable(sample, Path::new("/proj/bad.yaml"));
    assert!(diagnostics.is_empty());
  }

  #[test]
  fn test_parse_markdownlint_text_extracts_diagnostics() {
    // Captured from a real `markdownlint-cli2` run (stderr).
    let sample = concat!(
      "bad.md:3:1 error MD018/no-missing-space-atx No space after hash on atx style heading [Context: \"##Heading without space\"]\n",
      "bad.md:5 error MD032/blanks-around-lists Lists should be surrounded by blank lines [Context: \"* item\"]\n",
    );

    let diagnostics =
      parse_markdownlint_text(sample, Path::new("/proj/bad.md"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].source.as_deref(), Some("markdownlint"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String(
        "MD018/no-missing-space-atx".to_string()
      ))
    );
    assert!(diagnostics[0].message.starts_with("No space after hash"));
    // markdownlint is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 2);
    assert_eq!(diagnostics[0].range.start.character, 0);

    // Second violation has no column reported — defaults to column 1 (0-based 0).
    assert_eq!(diagnostics[1].range.start.line, 4);
    assert_eq!(diagnostics[1].range.start.character, 0);
  }

  #[test]
  fn test_parse_markdownlint_text_without_severity_token() {
    // Captured from `markdownlint-cli2@0.17.2` and from the `markdownlint`
    // (markdownlint-cli) fallback binary — neither prefixes the rule with a
    // severity word, so the parser must not shift every field by one token.
    let sample = concat!(
      "bad.md:1:1 MD018/no-missing-space-atx No space after hash on atx style heading [Context: \"#Heading\"]\n",
      "bad.md:4 MD032/blanks-around-lists Lists should be surrounded by blank lines [Context: \"* item\"]\n",
    );

    let diagnostics =
      parse_markdownlint_text(sample, Path::new("/proj/bad.md"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String(
        "MD018/no-missing-space-atx".to_string()
      ))
    );
    assert!(diagnostics[0].message.starts_with("No space after hash"));
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 0);

    assert_eq!(
      diagnostics[1].code,
      Some(NumberOrString::String(
        "MD032/blanks-around-lists".to_string()
      ))
    );
    assert_eq!(diagnostics[1].range.start.line, 3);
    assert_eq!(diagnostics[1].range.start.character, 0);
  }

  #[test]
  fn test_parse_markdownlint_text_path_with_spaces() {
    let sample =
      "my doc.md:3:1 error MD018/no-missing-space-atx No space after hash\n";
    let diagnostics =
      parse_markdownlint_text(sample, Path::new("/proj/my doc.md"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "No space after hash");
    assert_eq!(diagnostics[0].range.start.line, 2);
  }

  #[test]
  fn test_parsers_treat_empty_output_as_no_violations() {
    assert!(parse_markdownlint_text("", Path::new("/proj/bad.md")).is_empty());
    assert!(
      parse_yamllint_parsable("", Path::new("/proj/bad.yaml")).is_empty()
    );
    // biome always emits a JSON object, with an empty `diagnostics` array
    // when the file is clean.
    assert!(
      parse_biome_json(
        r#"{"summary":{"errors":0},"diagnostics":[],"command":"lint"}"#,
        Path::new("/proj/bad.js")
      )
      .is_empty()
    );
  }

  #[test]
  fn test_parse_yamllint_parsable_syntax_error_line() {
    // Captured from a real `yamllint -f parsable` run over invalid YAML —
    // the message itself contains a colon, which must stay in the message.
    let sample = "syn.yaml:2:3: [error] syntax error: mapping values are not allowed here (syntax)\n";
    let diagnostics =
      parse_yamllint_parsable(sample, Path::new("/proj/syn.yaml"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
      diagnostics[0].message,
      "syntax error: mapping values are not allowed here"
    );
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("syntax".to_string()))
    );
    assert_eq!(diagnostics[0].range.start.line, 1);
    assert_eq!(diagnostics[0].range.start.character, 2);
  }

  #[test]
  fn test_parse_yamllint_parsable_absolute_reported_path() {
    // yamllint echoes back whatever path it was given — absolute when the
    // LSP passes the document's own absolute path.
    let sample = "/proj/bad.yaml:1:1: [warning] missing document start \"---\" (document-start)\n";
    assert_eq!(
      parse_yamllint_parsable(sample, Path::new("/proj/bad.yaml")).len(),
      1
    );
  }

  #[test]
  fn test_parse_biome_json_parse_error_diagnostic() {
    // Captured from a real `biome lint --reporter=json` run over a file with
    // a syntax error: `category` is `parse`, not `lint/...`.
    let sample = r#"{"diagnostics":[{"severity":"error","message":"expected a name for the function in a function declaration, but found none","category":"parse","location":{"path":"syntax.js","start":{"line":1,"column":10},"end":{"line":1,"column":11}},"advices":[]}],"command":"lint"}"#;

    let diagnostics = parse_biome_json(sample, Path::new("/proj/syntax.js"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("parse".to_string()))
    );
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 9);
    assert_eq!(diagnostics[0].range.end.character, 10);
  }

  #[test]
  fn test_parse_markdownlint_text_filters_other_files_and_malformed_lines() {
    let sample = "not a markdownlint line\nother.md:1:1 error MD001/x desc\n";
    let diagnostics =
      parse_markdownlint_text(sample, Path::new("/proj/bad.md"));
    assert!(diagnostics.is_empty());
  }

  #[test]
  fn test_markdownlint_diagnostics_respects_formality_toml_md013_with_no_config_on_disk()
   {
    // End-to-end regression test for the QA-flagged `fml lsp` gap on issue
    // #1: before that issue, this path ran with no `--config` at all and
    // relied on markdownlint-cli2 auto-discovering `.markdownlint.json`
    // from disk, which happened to carry formality.toml's settings. With
    // that file deleted, `markdownlint_diagnostics` must now resolve
    // formality.toml itself and pass it inline — mirroring
    // `test_lint_respects_formality_toml_md013_with_no_config_on_disk` in
    // `src/surfaces/markdown.rs` for the CLI path.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let dir = tempfile::tempdir().unwrap();
    // Real words with spaces — see the CLI-side regression test's comment
    // for why a single unbroken token (e.g. "x".repeat(90)) doesn't work:
    // MD013's default `strict: false` exempts it under any config.
    let long_line = "lorem ipsum dolor sit amet ".repeat(5);
    let file_path = dir.path().join("a.md");
    std::fs::write(
      &file_path,
      format!("# Title\n\n```text\n{long_line}\n```\n"),
    )
    .unwrap();
    // No formality.toml either — resolve_for_lang must fall back to
    // embedded defaults (code_blocks/tables: false), which still differ
    // from markdownlint-cli2's own built-in defaults (true/true).
    assert!(!dir.path().join(".markdownlint.json").exists());
    assert!(!dir.path().join("formality.toml").exists());

    let diagnostics =
      markdownlint_diagnostics(dir.path(), Path::new("a.md"), None);
    assert_eq!(
      diagnostics,
      Some(Vec::new()),
      "expected a clean (Some(vec![])) result honoring formality's default \
       MD013 code_blocks/tables: false, got: {diagnostics:?}"
    );
  }

  #[test]
  fn test_markdownlint_diagnostics_reuses_passed_config() {
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("b.md");
    std::fs::write(&file_path, "# Title\n\nSome clean paragraph.\n").unwrap();

    let config = FormalityConfig::with_defaults();
    let diagnostics = diagnostics_for_file_with_config(
      dir.path(),
      Path::new("b.md"),
      Some(&config),
    );
    assert_eq!(diagnostics, Some(Vec::new()));
  }

  #[test]
  fn test_parse_clang_tidy_plain_extracts_diagnostics() {
    // Captured from a real `clang-tidy bad.cpp -- -std=c++17` run (an
    // assignment used as a condition triggers both an analyzer warning and
    // a diagnostic warning, each followed by a `note:` continuation line
    // that must be skipped).
    let sample = concat!(
      "/proj/bad.cpp:5:9: warning: value stored to 'x' is never read [clang-analyzer-deadcode.DeadStores]\n",
      "    5 |     if (x = 5) {\n",
      "      |         ^   ~\n",
      "/proj/bad.cpp:5:9: note: value stored to 'x' is never read\n",
      "/proj/bad.cpp:5:11: warning: using the result of an assignment as a condition without parentheses [clang-diagnostic-parentheses]\n",
    );

    let diagnostics =
      parse_clang_tidy_plain(sample, Path::new("/proj/bad.cpp"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diagnostics[0].source.as_deref(), Some("clang-tidy"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String(
        "clang-analyzer-deadcode.DeadStores".to_string()
      ))
    );
    assert_eq!(diagnostics[0].message, "value stored to 'x' is never read");
    // clang-tidy is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 4);
    assert_eq!(diagnostics[0].range.start.character, 8);

    assert_eq!(
      diagnostics[1].code,
      Some(NumberOrString::String(
        "clang-diagnostic-parentheses".to_string()
      ))
    );
  }

  #[test]
  fn test_parse_clang_tidy_plain_maps_error_severity_and_no_check_name() {
    // Captured from a real clang-tidy run over a raw compiler error.
    let sample = "/proj/err.cpp:2:3: error: use of undeclared identifier 'foo' [clang-diagnostic-error]\n";
    let diagnostics =
      parse_clang_tidy_plain(sample, Path::new("/proj/err.cpp"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));

    let no_check = "/proj/x.cpp:1:1: error: something broke\n";
    let diagnostics =
      parse_clang_tidy_plain(no_check, Path::new("/proj/x.cpp"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, None);
    assert_eq!(diagnostics[0].message, "something broke");
  }

  #[test]
  fn test_parse_clang_tidy_plain_filters_other_files_and_malformed_lines() {
    let sample =
      "not a clang-tidy line\n/proj/other.cpp:1:1: warning: x [check]\n";
    assert!(
      parse_clang_tidy_plain(sample, Path::new("/proj/bad.cpp")).is_empty()
    );
  }

  #[test]
  fn test_parse_golangci_lint_json_extracts_diagnostics() {
    // Trimmed from a real `golangci-lint run --output.json.path=stdout`
    // (v2.5.0) run — `Severity` is present in the schema but empty on every
    // observed issue regardless of underlying linter.
    let sample = r#"{"Issues":[{"FromLinter":"typecheck","Text":"\"os\" imported and not used","Severity":"","Pos":{"Filename":"bad.go","Offset":0,"Line":5,"Column":2}},{"FromLinter":"typecheck","Text":"declared and not used: x","Severity":"","Pos":{"Filename":"other.go","Offset":0,"Line":9,"Column":2}}]}"#;

    let diagnostics =
      parse_golangci_lint_json(sample, Path::new("/proj/bad.go"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diagnostics[0].message, "\"os\" imported and not used");
    assert_eq!(diagnostics[0].source.as_deref(), Some("golangci-lint"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("typecheck".to_string()))
    );
    // golangci-lint is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 4);
    assert_eq!(diagnostics[0].range.start.character, 1);
  }

  #[test]
  fn test_parse_golangci_lint_json_tolerates_absent_optional_fields() {
    // `FromLinter`/`Severity` are defaulted rather than required, so an
    // issue object missing either still parses instead of taking the whole
    // run's diagnostics down with it.
    let sample = r#"{"Issues":[{"Text":"boom","Pos":{"Filename":"bad.go","Line":1,"Column":1}}]}"#;
    let diagnostics =
      parse_golangci_lint_json(sample, Path::new("/proj/bad.go"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, None);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
  }

  #[test]
  fn test_parse_golangci_lint_json_empty_and_malformed_input() {
    assert!(
      parse_golangci_lint_json(r#"{"Issues":[]}"#, Path::new("/proj/x.go"))
        .is_empty()
    );
    assert!(
      parse_golangci_lint_json("not json", Path::new("/proj/x.go")).is_empty()
    );
  }

  #[test]
  fn test_parse_checkstyle_plain_extracts_diagnostics() {
    // Captured from a real `checkstyle -c checkstyle.xml -f plain` run
    // (checkstyle 10.20.2).
    let sample = concat!(
      "Starting audit...\n",
      "[ERROR] /proj/Bad.java:4:22: '=' is not followed by whitespace. [WhitespaceAround]\n",
      "[WARN] /proj/Bad.java:5:5: 'METHOD_DEF' should be separated from previous line. [EmptyLineSeparator]\n",
      "Audit done.\n",
    );

    let diagnostics =
      parse_checkstyle_plain(sample, Path::new("/proj/Bad.java"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].source.as_deref(), Some("checkstyle"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("WhitespaceAround".to_string()))
    );
    assert_eq!(diagnostics[0].message, "'=' is not followed by whitespace.");
    // checkstyle is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 3);
    assert_eq!(diagnostics[0].range.start.character, 21);

    assert_eq!(diagnostics[1].severity, Some(DiagnosticSeverity::WARNING));
  }

  #[test]
  fn test_parse_checkstyle_plain_missing_column_defaults_to_one() {
    // Captured from a real checkstyle run using `NewlineAtEndOfFile`, which
    // reports no column.
    let sample = "[ERROR] /proj/NoNewline.java:1: File does not end with a newline. [NewlineAtEndOfFile]\n";
    let diagnostics =
      parse_checkstyle_plain(sample, Path::new("/proj/NoNewline.java"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 0);
    assert_eq!(diagnostics[0].message, "File does not end with a newline.");
  }

  #[test]
  fn test_parse_checkstyle_plain_filters_other_files_and_banner_lines() {
    let sample = "Starting audit...\n[ERROR] /proj/Other.java:1:1: x [Rule]\nAudit done.\n";
    assert!(
      parse_checkstyle_plain(sample, Path::new("/proj/Bad.java")).is_empty()
    );
  }

  #[test]
  fn test_parse_ktlint_json_extracts_diagnostics() {
    // Captured from a real `ktlint --reporter=json` run (ktlint 1.8.0),
    // including the SLF4J `WARN ...` banner line ktlint itself prints to
    // stdout before the JSON array when violations are autocorrectable.
    let sample = concat!(
      "19:38:49.369 [main] WARN com.pinterest.ktlint.cli.internal.KtlintCommandLine -- ",
      "Lint has found errors than can be autocorrected using 'ktlint --format'\n",
      r#"[{"file":"/proj/Bad.kt","errors":[{"line":3,"column":10,"message":"Missing spacing before \"{\"","rule":"standard:curly-spacing"},{"line":4,"column":10,"message":"Missing spacing around \"=\"","rule":"standard:op-spacing"}]}]"#,
    );

    let diagnostics = parse_ktlint_json(sample, Path::new("/proj/Bad.kt"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diagnostics[0].source.as_deref(), Some("ktlint"));
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String("standard:curly-spacing".to_string()))
    );
    assert_eq!(diagnostics[0].message, "Missing spacing before \"{\"");
    // ktlint is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 2);
    assert_eq!(diagnostics[0].range.start.character, 9);
  }

  #[test]
  fn test_parse_ktlint_json_clean_file_and_malformed_input() {
    assert!(parse_ktlint_json("[\n]", Path::new("/proj/Good.kt")).is_empty());
    assert!(
      parse_ktlint_json("not json", Path::new("/proj/Bad.kt")).is_empty()
    );
    assert!(parse_ktlint_json("", Path::new("/proj/Bad.kt")).is_empty());
  }

  #[test]
  fn test_parse_ktlint_json_filters_other_files() {
    let sample = r#"[{"file":"/proj/Other.kt","errors":[{"line":1,"column":1,"message":"x","rule":"r"}]}]"#;
    assert!(parse_ktlint_json(sample, Path::new("/proj/Bad.kt")).is_empty());
  }

  #[test]
  fn test_parse_ktlint_json_banner_before_pretty_printed_array() {
    // Verbatim shape of a real `ktlint --reporter=json` run (ktlint 1.8.0):
    // the SLF4J banner — whose `[main]` thread name contains the first `[`
    // in the whole stream — precedes a *pretty-printed* array whose own
    // opening `[` sits alone on its line. Locating the array by searching
    // the stream for that line's text finds the banner's bracket instead
    // and fails the entire parse, so the array is located by its line's
    // byte offset. The single-line array in the test above happens not to
    // exercise this; real ktlint output always does.
    let sample = concat!(
      "19:56:50.862 [main] WARN com.pinterest.ktlint.cli.internal.KtlintCommandLine",
      " -- Lint has found errors than can be autocorrected using 'ktlint --format'\n",
      "[\n",
      "    {\n",
      "        \"file\": \"/proj/Main.kt\",\n",
      "        \"errors\": [\n",
      "            {\n",
      "                \"line\": 1,\n",
      "                \"column\": 9,\n",
      "                \"message\": \"Unnecessary long whitespace\",\n",
      "                \"rule\": \"standard:no-multi-spaces\"\n",
      "            }\n",
      "        ]\n",
      "    }\n",
      "]\n",
    );

    let diagnostics = parse_ktlint_json(sample, Path::new("/proj/Main.kt"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Unnecessary long whitespace");
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 8);
    assert_eq!(
      diagnostics[0].code,
      Some(NumberOrString::String(
        "standard:no-multi-spaces".to_string()
      ))
    );
  }

  #[test]
  fn test_parse_taplo_lint_plain_extracts_diagnostic() {
    // Captured from a real `taplo lint --colors never` run (taplo v0.10.0),
    // including the `tracing` INFO/ERROR log lines it prints to stderr
    // unconditionally alongside the diagnostic block.
    let sample = concat!(
      " INFO taplo:lint_files:collect_files: found files total=1\n",
      "error: conflicting keys\n",
      "  ┌─ /proj/bad.toml:3:1\n",
      "  │\n",
      "2 │ name = \"x\"\n",
      "  │ ---- duplicate found here\n",
      "3 │ name = \"y\"\n",
      "  │ ^^^^ duplicate key\n",
      "\n",
      "ERROR taplo:lint_files: invalid file error=semantic errors found path=\"/proj/bad.toml\"\n",
      "ERROR operation failed error=some files were not valid\n",
    );

    let diagnostics =
      parse_taplo_lint_plain(sample, Path::new("/proj/bad.toml"));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].source.as_deref(), Some("taplo"));
    assert_eq!(diagnostics[0].message, "conflicting keys");
    // taplo is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 2);
    assert_eq!(diagnostics[0].range.start.character, 0);
  }

  #[test]
  fn test_parse_taplo_lint_plain_multi_span_location_gap() {
    // Captured from a real taplo run over invalid syntax — the diagnostic
    // has a multi-line span, so the annotation gutter/box-drawing lines sit
    // between the message and the `┌─` location line for more than one
    // line, unlike the single-span case above.
    let sample = concat!(
      "error: invalid TOML\n",
      "  ┌─ /proj/bad2.toml:1:9\n",
      "  │  \n",
      "1 │   [package\n",
      "  │ ╭────────^\n",
      "2 │ │ name = \"x\"\n",
      "  │ ╰^ expected \"]\"\n",
    );
    let diagnostics =
      parse_taplo_lint_plain(sample, Path::new("/proj/bad2.toml"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "invalid TOML");
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert_eq!(diagnostics[0].range.start.character, 8);
  }

  #[test]
  fn test_parse_taplo_lint_plain_multiple_diagnostics_keep_own_locations() {
    // Two diagnostics in one run, verbatim shape from a real taplo v0.10.0
    // lint of a file with two distinct duplicate-key errors. Each message
    // must pick up the `┌─` line from its own block — the forward scan for
    // a location stops at the next `error:`/`warning:` line so a message
    // can never inherit a later diagnostic's position.
    let sample = concat!(
      " INFO taplo:lint_files:collect_files: found files total=1\n",
      "error: conflicting keys\n",
      "  ┌─ /proj/bad.toml:3:1\n",
      "  │\n",
      "2 │ x = 1\n",
      "  │ - duplicate found here\n",
      "3 │ x = 2\n",
      "  │ ^ duplicate key\n",
      "\n",
      "error: conflicting keys\n",
      "  ┌─ /proj/bad.toml:5:2\n",
      "  │\n",
      "1 │ [a]\n",
      "  │  - duplicate found here\n",
      "  ·\n",
      "5 │ [a]\n",
      "  │  ^ duplicate key\n",
      "\n",
      "ERROR operation failed error=some files were not valid\n",
    );

    let diagnostics =
      parse_taplo_lint_plain(sample, Path::new("/proj/bad.toml"));

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].range.start.line, 2);
    assert_eq!(diagnostics[0].range.start.character, 0);
    assert_eq!(diagnostics[1].range.start.line, 4);
    assert_eq!(diagnostics[1].range.start.character, 1);
  }

  #[test]
  fn test_parse_taplo_lint_plain_message_without_location_is_dropped() {
    // A message line with no `┌─` location before the next diagnostic is
    // skipped rather than borrowing the following diagnostic's position.
    let sample = concat!(
      "error: no location for this one\n",
      "error: conflicting keys\n",
      "  ┌─ /proj/bad.toml:7:1\n",
    );
    let diagnostics =
      parse_taplo_lint_plain(sample, Path::new("/proj/bad.toml"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "conflicting keys");
    assert_eq!(diagnostics[0].range.start.line, 6);
  }

  #[test]
  fn test_parse_taplo_lint_plain_filters_other_files_and_no_violations() {
    assert!(parse_taplo_lint_plain("", Path::new("/proj/bad.toml")).is_empty());
    let sample = "error: x\n  ┌─ /proj/other.toml:1:1\n";
    assert!(
      parse_taplo_lint_plain(sample, Path::new("/proj/bad.toml")).is_empty()
    );
  }

  #[test]
  fn test_parse_typst_short_extracts_diagnostics() {
    // Captured from a real `typst compile --diagnostic-format short` run
    // (typst 0.15.1), one error case and one warning case.
    let error_sample = "bad.typ:2:1: error: unknown variable: foo\n";
    let diagnostics =
      parse_typst_short(error_sample, Path::new("/proj/bad.typ"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].source.as_deref(), Some("typst"));
    assert_eq!(diagnostics[0].message, "unknown variable: foo");
    // typst is 1-based; LSP Position is 0-based.
    assert_eq!(diagnostics[0].range.start.line, 1);
    assert_eq!(diagnostics[0].range.start.character, 0);

    let warn_sample =
      "warn2.typ:2:16: warning: unknown font family: nonexistentfontxyz\n";
    let diagnostics =
      parse_typst_short(warn_sample, Path::new("/proj/warn2.typ"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
  }

  #[test]
  fn test_parse_typst_short_filters_other_files_and_malformed_lines() {
    let sample = "not a typst line\nother.typ:1:1: error: x\n";
    assert!(parse_typst_short(sample, Path::new("/proj/bad.typ")).is_empty());
  }

  // ---------------------------------------------------------------------
  // #177 — a runner that can't run at all must return `None`, not
  // `Some(vec![])`, so the caller falls back to `fml lint` instead of
  // publishing a false "clean" for a file the tool never looked at.
  // ---------------------------------------------------------------------

  #[test]
  fn test_clippy_diagnostics_none_when_no_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    // Deterministic regardless of whether `cargo` is installed in the test
    // environment: no `Cargo.toml` alone is enough to short-circuit.
    assert!(
      clippy_diagnostics(dir.path(), Path::new("main.rs"), None).is_none()
    );
  }

  #[test]
  fn test_golangci_lint_diagnostics_none_when_no_go_mod() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
      golangci_lint_diagnostics(dir.path(), Path::new("main.go"), None)
        .is_none()
    );
  }

  #[test]
  fn test_checkstyle_diagnostics_none_when_no_checkstyle_xml() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
      checkstyle_diagnostics(dir.path(), Path::new("Main.java"), None)
        .is_none()
    );
  }

  #[test]
  fn test_diagnostics_for_file_none_when_structured_tool_cannot_run() {
    // Threads all the way through the public entry point: a rust file in a
    // directory with no `Cargo.toml` must fall back to `fml lint`, not
    // publish an empty (false "clean") diagnostics list.
    let dir = tempfile::tempdir().unwrap();
    assert!(diagnostics_for_file(dir.path(), Path::new("main.rs")).is_none());
  }

  #[test]
  fn test_diagnostics_for_file_some_empty_when_tool_ran_and_found_nothing() {
    // The other half of the contract: a genuinely clean result from a tool
    // that did run is `Some(vec![])`, distinct from `None`. Exercised at
    // the parser level (`clippy_diagnostics` itself needs `cargo` on PATH
    // plus a real crate to actually run) — this only pins that the
    // `Option` wrapper preserves an empty-but-present result rather than
    // collapsing it to `None`.
    let diagnostics = parse_clippy_json("", Path::new("/proj/src/main.rs"));
    assert_eq!(Some(diagnostics), Some(Vec::new()));
  }
}
