//! Markdown language surface: formats via `prettier` and lints via
//! `markdownlint` (falling back to `prettier --check` if `markdownlint` is
//! unavailable), syncing the managed `.prettierrc.json` /
//! `.markdownlint.json` from `formality.toml`.

use super::{
  AUTO_GENERATED_JSON_COMMENT, DeclaresFacets, ExecutionContext, Facet,
  FacetSupport, LanguageSurface, NativeConfig, PrettierConfig, SurfaceResult,
  SurfaceStatus, ToolInfo, build_prettier_inline_args, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  render_native_config, run_tool_command, sync_native_config,
  sync_prettier_config, tool_missing_guard,
};
use crate::config::ResolvedLangConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Comment field container for markdownlint config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintComment {
  /// Comment description string.
  pub description: String,
}

/// MD013 (line length) rule options for markdownlint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintMd013 {
  /// Maximum line length allowed.
  pub line_length: usize,
  /// Whether to check code blocks.
  pub code_blocks: bool,
  /// Whether to check tables.
  pub tables: bool,
}

/// Native `.markdownlint.json` configuration representation for Markdown linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintConfig {
  /// Warning comment header block.
  #[serde(rename = "$comment")]
  pub comment: MarkdownlintComment,
  /// Default rule enablement setting.
  pub default: bool,
  /// MD013 line length rule settings.
  #[serde(rename = "MD013")]
  pub md013: MarkdownlintMd013,
}

impl NativeConfig for MarkdownlintConfig {
  const FILE_NAME: &'static str = ".markdownlint.json";

  fn from_context(ctx: &ExecutionContext) -> Self {
    markdownlint_config_for_lang(&ctx.lang_config)
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Builds the resolved [`MarkdownlintConfig`] from a [`ResolvedLangConfig`]
/// alone — the shared logic behind both [`NativeConfig::from_context`]
/// (used by `fml sync`/`fml fmt`/`fml lint`, which all have a full
/// [`ExecutionContext`] on hand) and [`write_markdownlint_temp_config`]
/// (also called from `fml lsp`'s `markdownlint_diagnostics`, which only
/// ever resolves a per-language config, not a full `ExecutionContext`).
fn markdownlint_config_for_lang(
  lang_config: &ResolvedLangConfig,
) -> MarkdownlintConfig {
  MarkdownlintConfig {
    comment: MarkdownlintComment {
      description: AUTO_GENERATED_JSON_COMMENT.to_string(),
    },
    default: true,
    md013: MarkdownlintMd013 {
      line_length: lang_config.line_length,
      code_blocks: false,
      tables: false,
    },
  }
}

/// Builds argument vector for markdownlint-cli2 invocation. `config_path`,
/// when given, is passed as `--config <path>` ahead of the file list so it
/// takes effect for both the `--fix` pass and the plain lint pass — the only
/// way to hand markdownlint-cli2 formality.toml's resolved settings, since
/// unlike prettier/rustfmt it has no per-flag inline config mechanism (see
/// [`write_markdownlint_temp_config`]).
#[must_use]
pub fn build_markdownlint_args(
  files: &[PathBuf],
  fix: bool,
  config_path: Option<&Path>,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = Vec::new();
  if fix {
    args.push("--fix".to_string());
  }
  if let Some(path) = config_path {
    args.push("--config".to_string());
    args.push(path.to_string_lossy().to_string());
  }
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Renders the resolved [`MarkdownlintConfig`] to a throwaway temp file and
/// returns the guard holding it, so `fml fmt`/`fml lint`/`fml lsp` can pass
/// `--config <temp-path>` to markdownlint-cli2 without ever writing
/// `.markdownlint.json` into the project tree. markdownlint-cli2 only
/// accepts a config *path* (no per-flag inline settings like prettier or
/// rustfmt get), so a temp file is the only way to hand it formality.toml's
/// resolved settings inline. **A discovered `.markdownlint.json` in the
/// linted file's own directory still takes precedence over this
/// `--config`** — markdownlint-cli2 treats `--config` as a default, not an
/// override, so formality.toml only wins here when no such file exists on
/// disk (true for this repo post-#1, and the common case for any repo that
/// dropped its native config files). The returned
/// [`tempfile::NamedTempFile`] is named with a `.markdownlint-` prefix
/// (never the bare `.markdownlint.json` name) deliberately — that keeps it
/// from being auto-discovered by markdownlint-cli2 itself for the scratch
/// copies `diff_check_via_tempcopy` places in the same tmpdir; don't
/// "simplify" the prefix away. It must be kept alive for the duration of
/// the command it's passed to — it is deleted from disk when dropped,
/// which also guarantees cleanup on an early-return/error path. Only `fml
/// sync` writes the persistent `.markdownlint.json` now (see
/// [`MarkdownSurface::sync_config`]).
pub(crate) fn write_markdownlint_temp_config(
  lang_config: &ResolvedLangConfig,
) -> std::io::Result<tempfile::NamedTempFile> {
  use std::io::Write;

  let cfg = markdownlint_config_for_lang(lang_config);
  let content = cfg
    .render()
    .map_err(|e| std::io::Error::other(e.to_string()))?;

  let mut file = tempfile::Builder::new()
    .prefix(".markdownlint-")
    .suffix(".json")
    .tempfile()?;
  file.write_all(content.as_bytes())?;
  file.flush()?;
  Ok(file)
}

/// Builds argument vector for prettier format invocation.
#[must_use]
pub fn build_prettier_fmt_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["--write".to_string()];
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Markdown language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownSurface;

impl DeclaresFacets for MarkdownSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs
      | Facet::IndentWidth
      | Facet::LineLength
      | Facet::ProseWrap => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ImportSort
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const MD_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkdn"];

impl LanguageSurface for MarkdownSurface {
  fn name(&self) -> &'static str {
    "markdown"
  }

  fn aliases(&self) -> &[&'static str] {
    &["md"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    MD_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &Path) -> bool {
    root.join(".markdownlint.json").is_file()
      || root.join(".markdownlint.yaml").is_file()
      || !find_files_with_ext(root, MD_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "prettier",
        description: "Opinionated code/markdown formatter",
        install_hint: "Install via: npm install -g prettier (or pnpm add -g prettier / brew install prettier / winget install Prettier.Prettier)",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "markdownlint-cli2",
        description: "Fast markdown linter",
        install_hint: "Install via: npm install -g markdownlint-cli2 (or brew install markdownlint-cli2)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  // Orchestrates prettier markdown formatting across check and write modes with fallback tempcopy handling.
  #[allow(clippy::too_many_lines)]
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "prettier",
      start,
      Some("npm install -g prettier"),
    ) {
      return res;
    }

    let files = ctx.matched_files(MD_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let md_binary = if check_binary_exists("markdownlint-cli2") {
      Some("markdownlint-cli2")
    } else if check_binary_exists("markdownlint") {
      Some("markdownlint")
    } else {
      None
    };

    // Inline `--tab-width`/`--print-width`/etc. instead of writing
    // `.prettierrc.json` to disk — see `build_prettier_inline_args` (Fixes
    // #151). `fml sync` remains the only path that materializes the file.
    let inline_config =
      build_prettier_inline_args(&PrettierConfig::from_context(ctx));

    // markdownlint-cli2's own `--fix` pass has no per-flag inline config
    // (it only accepts `--config <path>`), so the resolved settings are
    // rendered to a throwaway temp file and passed via `--config` instead —
    // see `write_markdownlint_temp_config`. Only created when a markdownlint
    // binary was actually found; kept alive across both the check-only and
    // write branches below so the file exists for the duration of every
    // invocation that references its path.
    let md_temp_cfg = if md_binary.is_some() {
      match write_markdownlint_temp_config(&ctx.lang_config) {
        Ok(f) => Some(f),
        Err(e) => {
          return SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ExecutionError {
              message: format!(
                "Failed to write temporary markdownlint config: {e}"
              ),
            },
            duration: start.elapsed(),
          };
        }
      }
    } else {
      None
    };
    let md_temp_cfg_path =
      md_temp_cfg.as_ref().map(tempfile::NamedTempFile::path);

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          if let Some(bin) = md_binary {
            let mut md_cmd = create_tool_command(bin);
            md_cmd.arg("--fix");
            if let Some(cfg_path) = md_temp_cfg_path {
              md_cmd.arg("--config").arg(cfg_path);
            }
            md_cmd.arg(scratch);
            md_cmd.current_dir(ctx.root.as_path());
            let _ = md_cmd.output();
          }

          let mut cmd = create_tool_command("prettier");
          cmd
            .arg("--write")
            .arg("--parser")
            .arg("markdown")
            .args(&inline_config)
            .arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    if let Some(bin) = md_binary {
      let mut md_cmd = create_tool_command(bin);
      md_cmd.arg("--fix");
      if let Some(cfg_path) = md_temp_cfg_path {
        md_cmd.arg("--config").arg(cfg_path);
      }
      for f in &files {
        md_cmd.arg(f);
      }
      md_cmd.current_dir(ctx.root.as_path());
      let _ = md_cmd.output();
    }

    let mut cmd = create_tool_command("prettier");
    cmd.args(build_prettier_fmt_args(&files, &ctx.lang_config.extra_args));
    cmd.args(&inline_config);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    let binary = if check_binary_exists("markdownlint-cli2") {
      "markdownlint-cli2"
    } else if check_binary_exists("markdownlint") {
      "markdownlint"
    } else {
      return tool_missing_guard(
        self.name(),
        "markdownlint-cli2",
        start,
        Some("npm install -g markdownlint-cli2"),
      )
      .unwrap();
    };

    let files = ctx.matched_files(MD_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // See `write_markdownlint_temp_config`: markdownlint-cli2 only accepts
    // config via `--config <path>`, so the resolved formality.toml settings
    // are rendered to a throwaway temp file rather than depending on
    // `.markdownlint.json` being present on disk. `md_temp_cfg` must stay
    // alive until `cmd.output()` below returns.
    let md_temp_cfg = match write_markdownlint_temp_config(&ctx.lang_config) {
      Ok(f) => f,
      Err(e) => {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to write temporary markdownlint config: {e}"
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    let mut cmd = create_tool_command(binary);
    cmd.args(build_markdownlint_args(
      &files,
      fix,
      Some(md_temp_cfg.path()),
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`'s prettier pass no longer goes through the `.prettierrc.json`
  // half of this path (Fixes #151): it passes those settings to prettier
  // inline (see `build_prettier_inline_args`, used in `format()` above).
  // `.markdownlint.json` is still written here because `fml fmt`'s
  // markdownlint-cli2 pass and `fml lint` both still consume it — see the
  // comment in `format()` for why that tool can't take its settings inline.
  // This method (both halves) is otherwise reached only by `fml sync`, for
  // users who explicitly want the native files materialized on disk.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let md_res =
      sync_native_config::<MarkdownlintConfig>(ctx, check, start, self.name());
    if !md_res.is_success() {
      return md_res;
    }

    // Also sync .prettierrc.json
    sync_prettier_config(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::ResolvedLangConfig;
  use crate::surfaces::test_ctx;
  use tempfile::TempDir;

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

  #[test]
  fn test_markdownlint_config_typed_serialization() {
    let cfg = MarkdownlintConfig {
      comment: MarkdownlintComment {
        description: "desc".to_string(),
      },
      default: true,
      md013: MarkdownlintMd013 {
        line_length: 120,
        code_blocks: false,
        tables: false,
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$comment\":"));
    assert!(rendered.contains("\"description\": \"desc\""));
    assert!(rendered.contains("\"default\": true"));
    assert!(rendered.contains("\"MD013\":"));
    assert!(rendered.contains("\"line_length\": 120"));
  }

  #[test]
  fn test_markdown_supports_lint_fix() {
    assert!(MarkdownSurface.supports_lint_fix());
  }

  #[test]
  fn test_build_markdownlint_args_with_and_without_fix() {
    let no_fix = build_markdownlint_args(&[], false, None, &[]);
    assert_eq!(no_fix, Vec::<String>::new());

    let files = vec![PathBuf::from("a.md"), PathBuf::from("b.md")];
    let extra = vec!["--loglevel".to_string(), "warn".to_string()];
    let with_fix = build_markdownlint_args(&files, true, None, &extra);
    assert_eq!(
      with_fix,
      vec![
        "--fix".to_string(),
        "a.md".to_string(),
        "b.md".to_string(),
        "--loglevel".to_string(),
        "warn".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_markdownlint_args_with_config_path() {
    let files = vec![PathBuf::from("a.md")];
    let cfg_path = PathBuf::from("/tmp/some-config.json");
    let args =
      build_markdownlint_args(&files, true, Some(cfg_path.as_path()), &[]);
    assert_eq!(
      args,
      vec![
        "--fix".to_string(),
        "--config".to_string(),
        cfg_path.to_string_lossy().to_string(),
        "a.md".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_markdownlint_args_extra_args_config_wins_last() {
    // markdownlint-cli2 honours the *last* `--config` flag it sees, so a
    // project-supplied `extra_args = ["--config", "mine.json"]` must land
    // after the injected temp-config path to actually override it (see
    // `write_markdownlint_temp_config`'s doc comment).
    let files = vec![PathBuf::from("a.md")];
    let injected = PathBuf::from("/tmp/.markdownlint-abc123.json");
    let extra = vec!["--config".to_string(), "mine.json".to_string()];
    let args =
      build_markdownlint_args(&files, false, Some(injected.as_path()), &extra);
    assert_eq!(
      args,
      vec![
        "--config".to_string(),
        injected.to_string_lossy().to_string(),
        "a.md".to_string(),
        "--config".to_string(),
        "mine.json".to_string(),
      ]
    );
    // The user-supplied override is the last "--config" in the arg list.
    let last_config_idx = args.iter().rposition(|a| a == "--config").unwrap();
    assert_eq!(args[last_config_idx + 1], "mine.json");
  }

  #[test]
  fn test_write_markdownlint_temp_config_reflects_formality_toml() {
    // Regression test for the gap CI caught on issue #1: markdownlint-cli2
    // has no per-flag inline config, so `fml lint`/`fml fmt` used to run it
    // with whatever `.markdownlint.json` happened to be on disk (or its own
    // stricter built-in defaults — MD013 `code_blocks`/`tables: true` — if
    // that file was absent). The temp-file config must carry
    // formality.toml's actual resolved MD013 settings.
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.line_length = 100;

    let temp_cfg = write_markdownlint_temp_config(&lang_cfg).unwrap();
    let content = std::fs::read_to_string(temp_cfg.path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["MD013"]["line_length"], 100);
    assert_eq!(parsed["MD013"]["code_blocks"], false);
    assert_eq!(parsed["MD013"]["tables"], false);
  }

  #[test]
  fn test_lint_respects_formality_toml_md013_with_no_config_on_disk() {
    // End-to-end regression test for the same gap: with no
    // `.markdownlint.json` anywhere on disk (the actual state of this repo's
    // own root after issue #1), `lint()` must still enforce formality.toml's
    // MD013 settings (via the temp-config `--config` pass), not
    // markdownlint-cli2's stricter built-in defaults.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let temp = TempDir::new().unwrap();
    // A code fence line over 80 chars, made of real words with spaces
    // (NOT a single unbroken token like "x".repeat(90) — MD013's default
    // `strict: false` exempts any line with no spaces past the limit, so an
    // unbroken-token line is never flagged by *any* config, config-present
    // or config-absent, fixed or broken; that made an earlier version of
    // this test pass even with the fix reverted). With real spaces,
    // markdownlint-cli2's built-in MD013 default (`code_blocks: true`)
    // flags this; formality.toml's default (`code_blocks: false`, set in
    // `MarkdownlintConfig::from_context`) must not.
    let long_line = "lorem ipsum dolor sit amet ".repeat(5);
    std::fs::write(
      temp.path().join("a.md"),
      format!("# Title\n\n```text\n{long_line}\n```\n"),
    )
    .unwrap();
    assert!(!temp.path().join(".markdownlint.json").exists());

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = surface.lint(&ctx, false);
    assert!(
      res.is_success(),
      "expected clean lint with no .markdownlint.json on disk, got: {:?}",
      res.status
    );
    // Still no native config file was written as a side effect.
    assert!(!temp.path().join(".markdownlint.json").exists());
  }

  #[test]
  fn test_build_prettier_fmt_args() {
    let files = vec![PathBuf::from("readme.md")];
    let extra = vec!["--loglevel".to_string(), "warn".to_string()];
    let args = build_prettier_fmt_args(&files, &extra);
    assert_eq!(
      args,
      vec![
        "--write".to_string(),
        "readme.md".to_string(),
        "--loglevel".to_string(),
        "warn".to_string(),
      ]
    );
  }

  #[test]
  fn test_markdown_sync_config() {
    let temp = TempDir::new().unwrap();
    let surface = MarkdownSurface;
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 2;

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let md_path = temp.path().join(".markdownlint.json");
    let prettier_path = temp.path().join(".prettierrc.json");
    assert!(md_path.is_file());
    assert!(prettier_path.is_file());

    let md_content = std::fs::read_to_string(&md_path).unwrap();
    let prettier_content = std::fs::read_to_string(&prettier_path).unwrap();

    assert!(md_content.contains("\"line_length\": 100"));
    assert!(prettier_content.contains("\"printWidth\": 100"));
    assert!(prettier_content.contains("\"$comment\""));
  }

  #[test]
  fn test_build_prettier_inline_args_shape() {
    let cfg = PrettierConfig {
      comment: "warning".to_string(),
      tab_width: 4,
      print_width: 100,
      use_tabs: true,
      end_of_line: "crlf".to_string(),
      prose_wrap: "preserve".to_string(),
    };
    let args = build_prettier_inline_args(&cfg);
    assert!(args.contains(&"--tab-width=4".to_string()));
    assert!(args.contains(&"--print-width=100".to_string()));
    assert!(args.contains(&"--end-of-line=crlf".to_string()));
    assert!(args.contains(&"--prose-wrap=preserve".to_string()));
    assert!(args.contains(&"--use-tabs".to_string()));
  }

  #[test]
  fn test_markdown_format_does_not_write_prettierrc() {
    // Fixes #151: `fml fmt` must not write `.prettierrc.json` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !check_binary_exists("prettier") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.md"), "# hi\n").unwrap();

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let _ = surface.format(&ctx);

    assert!(!temp.path().join(".prettierrc.json").exists());
  }
}
