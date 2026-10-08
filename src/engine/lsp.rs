//! The language server behind `fml lsp`, built from granular operations.
//!
//! Owns what the server computes: which files are formality configs, the
//! edits formatting one file produces, and the diagnostics linting one file
//! produces. `server` wires those into the LSP handlers; `diagnostics` parses
//! each linter's machine-readable output. The process, its runtime, and the
//! stdio transport belong to the CLI.

/// Structured per-violation lint diagnostics parsed from each linter.
pub mod diagnostics;
/// The `tower_lsp` handlers, each delegating to a function here.
pub mod server;

use std::fs;
use std::path;

use log;
use tower_lsp::lsp_types;

use crate::config;
use crate::engine::runner;
use crate::engine::target;

/// Returns whether `path` names a formality config file (`formality.toml`
/// or `.formality.toml`).
#[must_use]
pub fn is_config_file(path: &path::Path) -> bool {
  path
    .file_name()
    .and_then(|n| n.to_str())
    .is_some_and(|name| config::CONFIG_FILE_CANDIDATES.contains(&name))
}

/// Runs `plan` over the single file `path`, with the surfaces that claim it,
/// logging each surface's result.
fn run_file(
  root: &path::Path,
  path: &path::Path,
  config: &config::FormalityConfig,
  plan: &runner::Plan,
) -> runner::ExitStatus {
  let paths = [path.to_path_buf()];
  let scope =
    runner::Scope::resolve(root, &paths, &config.resolve_global().exclude);
  match target::resolve_target_surfaces(root, &[], &scope, config) {
    Ok(surfaces) => {
      let results = runner::Runner::run(&surfaces, root, &scope, plan, config);
      for result in &results {
        log::info!("{}: {:?}", result.surface_name, result.status);
      }
      runner::compute_exit_status(&results, plan.allow_missing)
    }
    Err(err) => {
      log::error!("{}: {err}", path.display());
      runner::ExitStatus::Error
    }
  }
}

/// Formats `path` on disk and returns the edits that bring an editor's copy
/// in line, or `None` when the file cannot be read or formatting fails (the
/// reason is logged).
#[must_use]
pub fn format_file(
  root: &path::Path,
  path: &path::Path,
  config: &config::FormalityConfig,
) -> Option<Vec<lsp_types::TextEdit>> {
  let before = fs::read_to_string(path)
    .inspect_err(|err| log::error!("cannot read {}: {err}", path.display()))
    .ok()?;
  if !run_file(root, path, config, &runner::Plan::fmt(false, false)).is_clean()
  {
    log::error!("formatting failed for {}", path.display());
    return None;
  }
  let after = fs::read_to_string(path).unwrap_or_default();
  Some(formatting_edits(&before, &after))
}

/// Lints `path` and returns its diagnostics.
///
/// Surfaces with a structured parser get one diagnostic per violation. The
/// rest, and any whose structured tool could not run, fall back to a full
/// lint pass that yields one file-level warning on failure, so a file is
/// never published clean when no linter actually ran (#177 [pre-recreation]).
#[must_use]
pub fn diagnose_file(
  root: &path::Path,
  path: &path::Path,
  config: &config::FormalityConfig,
) -> Vec<lsp_types::Diagnostic> {
  if let Some(diagnostics) =
    diagnostics::diagnostics_for_file_with_config(root, path, Some(config))
  {
    return diagnostics;
  }
  if run_file(root, path, config, &runner::Plan::lint(false)).is_clean() {
    return Vec::new();
  }
  vec![lsp_types::Diagnostic {
    range: lsp_types::Range::default(),
    severity: Some(lsp_types::DiagnosticSeverity::WARNING),
    source: Some("formality".to_string()),
    message: "fml lint found issues; see the Formality output channel."
      .to_string(),
    ..Default::default()
  }]
}

/// Returns the range covering all of `text`, in LSP coordinates: 0-indexed
/// lines and UTF-16 code-unit columns.
fn full_document_range(text: &str) -> lsp_types::Range {
  let line_count = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
  let last_col = text.lines().last().map_or(0, |l| {
    u32::try_from(l.encode_utf16().count()).unwrap_or(u32::MAX)
  });
  lsp_types::Range {
    start: lsp_types::Position {
      line: 0,
      character: 0,
    },
    end: lsp_types::Position {
      line: line_count.saturating_sub(1),
      character: last_col,
    },
  }
}

/// Returns the edits that turn `before` into `after`: none when equal,
/// otherwise one whole-document replacement.
fn formatting_edits(before: &str, after: &str) -> Vec<lsp_types::TextEdit> {
  if before == after {
    return Vec::new();
  }
  vec![lsp_types::TextEdit {
    range: full_document_range(before),
    new_text: after.to_string(),
  }]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn full_document_range_empty_document() {
    let range = full_document_range("");
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
  }

  #[test]
  fn full_document_range_single_line() {
    let range = full_document_range("hello world");
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 0,
        character: 11
      }
    );
  }

  #[test]
  fn full_document_range_single_line_with_trailing_newline() {
    let range = full_document_range("hello world\n");
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 0,
        character: 11
      }
    );
  }

  #[test]
  fn full_document_range_multiline() {
    let text = "fn main() {\n    println!(\"hello\");\n}";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 2,
        character: 1
      }
    );
  }

  #[test]
  fn full_document_range_multiline_with_trailing_newline() {
    let text = "line 1\nline 2\nline 3\n";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 2,
        character: 6
      }
    );
  }

  #[test]
  fn full_document_range_multibyte_unicode_utf16_counts() {
    // 🦀 is 4 UTF-8 bytes, but 2 UTF-16 code units (surrogate pair)
    // 🚀 is 4 UTF-8 bytes, but 2 UTF-16 code units
    let text = "let crab = \"🦀 🚀\";";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    // "let crab = \"" = 12
    // "🦀" = 2
    // " " = 1
    // "🚀" = 2
    // "\";" = 2
    // Total = 19 UTF-16 code units (vs 23 UTF-8 bytes)
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 0,
        character: 19
      }
    );

    // Chinese characters: 3 UTF-8 bytes each, 1 UTF-16 code unit each
    let chinese = "你好世界";
    let range_chinese = full_document_range(chinese);
    assert_eq!(
      range_chinese.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range_chinese.end,
      lsp_types::Position {
        line: 0,
        character: 4
      }
    );
  }

  #[test]
  fn full_document_range_multiline_with_multibyte_unicode() {
    let text = "fn main() {\n    // 🦀 🚀\n}";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      lsp_types::Position {
        line: 2,
        character: 1
      }
    );

    let text_unicode_last_line = "fn main() {\n    let s = \"你好 🌍\";";
    let range_unicode_last = full_document_range(text_unicode_last_line);
    assert_eq!(
      range_unicode_last.start,
      lsp_types::Position {
        line: 0,
        character: 0
      }
    );
    // Line 1: "    let s = \"你好 🌍\";" -> 13 + 2 + 1 + 2 + 2 = 20 UTF-16 code units
    assert_eq!(
      range_unicode_last.end,
      lsp_types::Position {
        line: 1,
        character: 20
      }
    );
  }

  #[test]
  fn formatting_edits_no_change() {
    let content = "fn main() {}\n";
    let edits = formatting_edits(content, content);
    assert!(edits.is_empty());
  }

  #[test]
  fn formatting_edits_with_changes() {
    let before = "fn main(){\nprintln!(\"hello\");\n}";
    let after = "fn main() {\n    println!(\"hello\");\n}\n";
    let edits = formatting_edits(before, after);
    assert_eq!(edits.len(), 1);
    assert_eq!(
      edits[0].range,
      lsp_types::Range {
        start: lsp_types::Position {
          line: 0,
          character: 0
        },
        end: lsp_types::Position {
          line: 2,
          character: 1
        },
      }
    );
    assert_eq!(edits[0].new_text, after);
  }

  #[test]
  fn formatting_edits_multibyte_unicode() {
    let before = "fn main() {\nlet msg = \"🦀 世界\";\n}";
    let after = "fn main() {\n    let msg = \"🦀 世界\";\n}\n";
    let edits = formatting_edits(before, after);
    assert_eq!(edits.len(), 1);
    assert_eq!(
      edits[0].range,
      lsp_types::Range {
        start: lsp_types::Position {
          line: 0,
          character: 0
        },
        end: lsp_types::Position {
          line: 2,
          character: 1
        },
      }
    );
    assert_eq!(edits[0].new_text, after);
  }

  #[test]
  fn test_is_config_file() {
    assert!(is_config_file(path::Path::new("formality.toml")));
    assert!(is_config_file(path::Path::new(".formality.toml")));
    assert!(is_config_file(path::Path::new(
      "/path/to/project/formality.toml"
    )));
    assert!(is_config_file(path::Path::new(
      "/path/to/project/.formality.toml"
    )));
    #[cfg(windows)]
    assert!(is_config_file(path::Path::new(
      "C:\\path\\to\\project\\.formality.toml"
    )));

    assert!(!is_config_file(path::Path::new("other.toml")));
    assert!(!is_config_file(path::Path::new("Cargo.toml")));
    assert!(!is_config_file(path::Path::new("notes.txt")));
  }
}
