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
//! Only `rust` (clippy) and `python` (ruff) are wired up to structured
//! diagnostics today, matching the issue's own guidance to start with the
//! most common surfaces rather than force-fitting all 12. Every other
//! surface — cpp, go, java, javascript, json, kotlin, markdown, toml, typst,
//! yaml — falls back to the caller's generic single warning
//! (`lsp.rs::did_save`) until a follow-up issue adds JSON/structured output
//! parsing for the tools those surfaces shell out to (most of which do
//! support a machine-readable mode, e.g. `eslint --format=json`, `yamllint
//! -f parsable`, but each has its own schema to model).
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
      diagnostics_for_file(Path::new("."), Path::new("notes.md")).is_none()
    );
    assert!(
      diagnostics_for_file(Path::new("."), Path::new("config.yaml")).is_none()
    );
  }
}
