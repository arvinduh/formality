//! Shared Prettier configuration model, CLI argument builders, and configuration syncing.

use super::{
  AUTO_GENERATED_JSON_COMMENT, ExecutionContext, NativeConfig, SurfaceResult,
  render_native_config, sync_native_config,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Native `.prettierrc.json` configuration representation for Markdown, YAML, and JSON formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrettierConfig {
  /// Warning comment field.
  #[serde(rename = "$comment")]
  pub comment: String,
  /// Indentation tab width in spaces.
  pub tab_width: usize,
  /// Maximum print width limit.
  pub print_width: usize,
  /// Whether tab indentation is enabled.
  pub use_tabs: bool,
  /// End of line newline style.
  pub end_of_line: String,
  /// Prose wrapping strategy string.
  pub prose_wrap: String,
}

impl NativeConfig for PrettierConfig {
  const FILE_NAME: &'static str = ".prettierrc.json";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let eol = match ctx.global_config.end_of_line.to_lowercase().as_str() {
      "crlf" => "crlf",
      "cr" => "cr",
      _ => "lf",
    };
    let prose_wrap = ctx.lang_config.prose_wrap.as_deref().unwrap_or("always");

    Self {
      comment: AUTO_GENERATED_JSON_COMMENT.to_string(),
      tab_width: ctx.lang_config.indent_size,
      print_width: ctx.lang_config.line_length,
      use_tabs: ctx.lang_config.use_tabs,
      end_of_line: eol.to_string(),
      prose_wrap: prose_wrap.to_string(),
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Renders the resolved [`PrettierConfig`] as the inline `--tab-width`/
/// `--print-width`/etc. flags `prettier` accepts on the CLI, so `fml fmt`
/// can apply formality.toml's settings without writing `.prettierrc.json`
/// to disk (Fixes #151). Shared by the Markdown, YAML, and JSON surfaces,
/// which all format via prettier. Only `fml sync` writes that file now.
#[must_use]
pub fn build_prettier_inline_args(cfg: &PrettierConfig) -> Vec<String> {
  let mut args = vec![
    format!("--tab-width={}", cfg.tab_width),
    format!("--print-width={}", cfg.print_width),
    format!("--end-of-line={}", cfg.end_of_line),
    format!("--prose-wrap={}", cfg.prose_wrap),
  ];
  if cfg.use_tabs {
    args.push("--use-tabs".to_string());
  }
  args
}

/// Synchronizes `.prettierrc.json` native configuration for surfaces formatted with Prettier.
#[must_use]
pub fn sync_prettier_config(
  ctx: &ExecutionContext,
  check: bool,
  start: Instant,
  surface_name: &'static str,
) -> SurfaceResult {
  sync_native_config::<PrettierConfig>(ctx, check, start, surface_name)
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_prettier_config_typed_serialization() {
    let cfg = PrettierConfig {
      comment: "warning".to_string(),
      tab_width: 4,
      print_width: 100,
      use_tabs: true,
      end_of_line: "crlf".to_string(),
      prose_wrap: "preserve".to_string(),
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$comment\": \"warning\""));
    assert!(rendered.contains("\"tabWidth\": 4"));
    assert!(rendered.contains("\"printWidth\": 100"));
    assert!(rendered.contains("\"useTabs\": true"));
    assert!(rendered.contains("\"endOfLine\": \"crlf\""));
    assert!(rendered.contains("\"proseWrap\": \"preserve\""));
  }
}
