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
//! `yaml` (yamllint), and `markdown` (markdownlint-cli2/markdownlint) are
//! wired up to structured diagnostics (Fixes #159, #165). Every other
//! surface — cpp, go, java, json, kotlin, toml, typst — falls back to the
//! caller's generic single warning (`lsp.rs::did_save`) until a follow-up
//! issue adds structured-output parsing for the tools those surfaces shell
//! out to. `json` has no linter at all (prettier-only, format-only surface),
//! so it has nothing to add structured diagnostics for.
//!
//! Neither invocation currently threads through `formality.toml`'s
//! `extra_args`/per-language overrides the way `fml lint` proper does — the
//! LSP path calls these with an empty extra-args slice. That's a known
//! simplification, not a correctness bug: the diagnostics still reflect the
//! same rule set, just not any project-specific extra CLI flags.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use tower_lsp::lsp_types::{
  Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range,
};

use crate::surfaces::{all_surfaces, check_binary_exists};

// ---------------------------------------------------------------------------
// Surface detection
// ---------------------------------------------------------------------------

/// Returns the canonical surface name (e.g. `"rust"`, `"python"`) whose
/// `file_extensions()` cover `file`'s extension, if any surface claims it.
#[must_use]
pub fn surface_name_for_file(file: &Path) -> Option<&'static str> {
  let ext = file.extension()?.to_str()?;
  all_surfaces()
    .into_iter()
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
/// `Diagnostic`s for violations touching `file`. Returns an empty vec (not
/// an error) when clippy is missing, there's no `Cargo.toml`, or the
/// invocation otherwise fails to produce output — the caller falls back to
/// the generic warning in that case via the shelled-out `fml lint` check.
fn clippy_diagnostics(root: &Path, file: &Path) -> Vec<Diagnostic> {
  if !check_binary_exists("cargo") || !root.join("Cargo.toml").exists() {
    return Vec::new();
  }

  let mut cmd = Command::new("cargo");
  cmd.args(crate::surfaces::rust::build_clippy_json_args(&[]));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => {
      parse_clippy_json(&String::from_utf8_lossy(&output.stdout), file)
    }
    Err(_) => Vec::new(),
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
/// `Diagnostic`s for `file`'s violations. Returns an empty vec (not an
/// error) when ruff is missing or the invocation otherwise fails to produce
/// output.
fn ruff_diagnostics(root: &Path, file: &Path) -> Vec<Diagnostic> {
  if !check_binary_exists("ruff") {
    return Vec::new();
  }

  let mut cmd = Command::new("ruff");
  cmd.args(crate::surfaces::python::build_ruff_check_json_args(
    &[file.to_path_buf()],
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => {
      parse_ruff_json(&String::from_utf8_lossy(&output.stdout), file)
    }
    Err(_) => Vec::new(),
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
/// `Diagnostic`s for `file`'s violations. Returns an empty vec (not an
/// error) when biome is missing or the invocation otherwise fails to
/// produce output.
fn biome_diagnostics(root: &Path, file: &Path) -> Vec<Diagnostic> {
  if !check_binary_exists("biome") {
    return Vec::new();
  }

  let mut cmd = Command::new("biome");
  cmd.args(crate::surfaces::javascript::build_biome_lint_json_args(
    file,
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => {
      parse_biome_json(&String::from_utf8_lossy(&output.stdout), file)
    }
    Err(_) => Vec::new(),
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
/// for `file`'s violations. Returns an empty vec (not an error) when
/// yamllint is missing or the invocation otherwise fails to produce output.
fn yamllint_diagnostics(root: &Path, file: &Path) -> Vec<Diagnostic> {
  if !check_binary_exists("yamllint") {
    return Vec::new();
  }

  let mut cmd = Command::new("yamllint");
  cmd.args(crate::surfaces::yaml::build_yamllint_parsable_args(file));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => {
      parse_yamllint_parsable(&String::from_utf8_lossy(&output.stdout), file)
    }
    Err(_) => Vec::new(),
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
/// own stderr-first message selection). Returns an empty vec (not an error)
/// when neither binary is present or the invocation otherwise fails to
/// produce output.
fn markdownlint_diagnostics(root: &Path, file: &Path) -> Vec<Diagnostic> {
  let binary = if check_binary_exists("markdownlint-cli2") {
    "markdownlint-cli2"
  } else if check_binary_exists("markdownlint") {
    "markdownlint"
  } else {
    return Vec::new();
  };

  let mut cmd = Command::new(binary);
  cmd.args(crate::surfaces::markdown::build_markdownlint_args(
    &[file.to_path_buf()],
    false,
    &[],
  ));
  cmd.current_dir(root);

  match cmd.output() {
    Ok(output) => {
      parse_markdownlint_text(&String::from_utf8_lossy(&output.stderr), file)
    }
    Err(_) => Vec::new(),
  }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Returns structured per-violation `Diagnostic`s for `file` if its surface
/// has a structured-output parser wired up here, or `None` if it doesn't
/// (see module docs for current coverage) — the caller should fall back to
/// the generic single-warning diagnostic in the `None` case.
#[must_use]
pub fn diagnostics_for_file(
  root: &Path,
  file: &Path,
) -> Option<Vec<Diagnostic>> {
  match surface_name_for_file(file)? {
    "rust" => Some(clippy_diagnostics(root, file)),
    "python" => Some(ruff_diagnostics(root, file)),
    "javascript" => Some(biome_diagnostics(root, file)),
    "yaml" => Some(yamllint_diagnostics(root, file)),
    "markdown" => Some(markdownlint_diagnostics(root, file)),
    _ => None,
  }
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
  fn test_diagnostics_for_file_unsupported_surface_returns_none() {
    assert!(
      diagnostics_for_file(Path::new("."), Path::new("main.cpp")).is_none()
    );
    assert!(
      diagnostics_for_file(Path::new("."), Path::new("config.toml")).is_none()
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
}
