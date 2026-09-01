//! YAML language surface: formats via `prettier` and lints via `yamllint`,
//! syncing the managed `.yamllint.yaml` from `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, PrettierConfig, SurfaceResult, ToolInfo,
  build_prettier_inline_args, classify_all_nonzero_as_error,
  create_tool_command, diff_check_via_tempcopy_classified, find_files_with_ext,
  lint_fix_unsupported, render_native_config, run_tool_command,
  run_tool_command_classified, sync_native_config, sync_prettier_config,
  tool_missing_guard,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

/// Toggle state enum for yamllint rules (`"enable"` or `"disable"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum YamllintRuleToggle {
  /// Enable rule.
  Enable,
  /// Disable rule.
  Disable,
}

/// Line length rule parameters for yamllint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintLineLengthRule {
  /// Maximum line length limit.
  pub max: usize,
}

/// Indentation rule parameters for yamllint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintIndentationRule {
  /// Number of spaces per indent level.
  pub spaces: usize,
  /// Whether to indent sequence items.
  #[serde(rename = "indent-sequences")]
  pub indent_sequences: bool,
}

/// Rules configuration subsection for `.yamllint.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintRulesConfig {
  /// Line length rule options.
  #[serde(rename = "line-length")]
  pub line_length: YamllintLineLengthRule,
  /// Indentation rule options.
  pub indentation: YamllintIndentationRule,
  /// Document start marker rule toggle.
  #[serde(rename = "document-start")]
  pub document_start: YamllintRuleToggle,
  /// Boolean truthy check rule toggle.
  pub truthy: YamllintRuleToggle,
}

/// Native `.yamllint.yaml` configuration representation for YAML linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintConfig {
  /// Parent configuration preset name to extend.
  pub extends: String,
  /// Rules configuration subsection.
  pub rules: YamllintRulesConfig,
}

impl NativeConfig for YamllintConfig {
  const FILE_NAME: &'static str = ".yamllint.yaml";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let yaml_opts = ctx.lang_config.yaml.as_ref();
    let indent_sequences =
      yaml_opts.and_then(|y| y.indent_sequence).unwrap_or(true);
    let document_start = match yaml_opts.and_then(|y| y.document_start) {
      Some(true) => YamllintRuleToggle::Enable,
      _ => YamllintRuleToggle::Disable,
    };
    let truthy = match yaml_opts.and_then(|y| y.truthy) {
      Some(true) => YamllintRuleToggle::Enable,
      _ => YamllintRuleToggle::Disable,
    };

    Self {
      extends: "default".to_string(),
      rules: YamllintRulesConfig {
        line_length: YamllintLineLengthRule {
          max: ctx.lang_config.line_length,
        },
        indentation: YamllintIndentationRule {
          spaces: ctx.lang_config.indent_size,
          indent_sequences,
        },
        document_start,
        truthy,
      },
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Renders a [`YamllintConfig`] as the inline YAML source text yamllint's
/// `-d`/`--config-data` flag accepts, so `fml lint` can apply
/// formality.toml's settings without writing `.yamllint.yaml` to disk
/// (Fixes #151). `fml sync` still writes that file for users who want it
/// materialized on disk (see [`YamlSurface::sync_config`], Fixes #158).
#[must_use]
pub fn build_yamllint_inline_config(cfg: &YamllintConfig) -> String {
  // yamllint's `-d` takes a literal YAML document, so this just reuses the
  // same renderer as the on-disk file (minus formality's auto-generated
  // header comment, which isn't meaningful for an inline value).
  serde_yaml::to_string(cfg).unwrap_or_default()
}

/// Builds the argument vector for `yamllint -f parsable <file>`, used by
/// `fml lsp`'s structured-diagnostics path (Fixes #165). `parsable` is
/// yamllint's long-stable gcc-style line format —
/// `path:line:col: [level] message (rule)` — verified against a locally
/// installed yamllint. Like the existing clippy/ruff diagnostics paths, this
/// intentionally runs with yamllint's own default rule set rather than
/// threading through `build_yamllint_inline_config`'s resolved
/// `formality.toml` settings — the same known simplification noted in this
/// module's callers (see `lsp_diagnostics.rs` module docs).
#[must_use]
pub fn build_yamllint_parsable_args(file: &Path) -> Vec<String> {
  vec![
    "-f".to_string(),
    "parsable".to_string(),
    file.to_string_lossy().to_string(),
  ]
}

/// YAML language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct YamlSurface;

impl DeclaresFacets for YamlSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      Facet::IndentWidth
      | Facet::LineLength
      | Facet::QuoteStyle
      | Facet::ProseWrap => FacetSupport::Configurable,
      Facet::TrailingComma
      | Facet::ImportSort
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const YAML_EXTENSIONS: &[&str] = &["yaml", "yml"];

impl LanguageSurface for YamlSurface {
  fn name(&self) -> &'static str {
    "yaml"
  }

  fn aliases(&self) -> &[&'static str] {
    &["yml"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    YAML_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn detect(&self, root: &Path) -> bool {
    root.join(".yamllint").is_file()
      || root.join(".yamllint.yaml").is_file()
      || root.join(".yamllint.yml").is_file()
      || !find_files_with_ext(root, YAML_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "prettier",
        description: "YAML formatter",
        install_hint: "Install via: npm install -g prettier (or pnpm add -g prettier / brew install prettier / winget install Prettier.Prettier)",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "yamllint",
        description: "YAML linter",
        install_hint: "Install via: pip install yamllint (or uv tool install yamllint / brew install yamllint / winget install yamllint)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

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

    let files = ctx.matched_files(YAML_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `--tab-width`/`--print-width`/etc. instead of writing
    // `.prettierrc.json` to disk — see `build_prettier_inline_args` (Fixes
    // #151). `fml sync` remains the only path that materializes the file.
    let inline_config =
      build_prettier_inline_args(&PrettierConfig::from_context(ctx));

    if ctx.check_only {
      return diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("prettier");
          cmd
            .arg("--write")
            .arg("--parser")
            .arg("yaml")
            .args(&inline_config)
            .arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        classify_all_nonzero_as_error,
      );
    }

    let mut cmd = create_tool_command("prettier");
    cmd.arg("--write");
    cmd.args(&inline_config);

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    // `prettier --write` exits `0` whether or not it reformats and only
    // exits non-zero (`2`) on a parse error / bad config / unreadable file
    // — never `1`, which is `--check`-only. So every non-zero exit here is
    // a tool failure (`ExecutionError`), not formatting drift, and the
    // `--check` path above classifies identically (Fixes #107).
    run_tool_command_classified(
      self.name(),
      &mut cmd,
      classify_all_nonzero_as_error,
    )
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if fix {
      return lint_fix_unsupported(self.name(), start);
    }

    if let Some(res) = tool_missing_guard(
      self.name(),
      "yamllint",
      start,
      Some("pip install yamllint"),
    ) {
      return res;
    }

    let files = ctx.matched_files(YAML_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `-d <yaml source>` instead of writing `.yamllint.yaml` to disk
    // — see `build_yamllint_inline_config` (Fixes #151). `fml sync` remains
    // the only path that materializes the file.
    let inline_config =
      build_yamllint_inline_config(&YamllintConfig::from_context(ctx));

    let mut cmd = create_tool_command("yamllint");
    cmd.arg("-d").arg(&inline_config);
    if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      for f in &files {
        cmd.arg(f);
      }
    } else {
      cmd.arg(".");
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151): they
  // pass the resolved config to prettier/yamllint inline (see
  // `build_prettier_inline_args` and `build_yamllint_inline_config`, used in
  // `format()`/`lint()` above). This method is now reached only by `fml
  // sync`, for users who explicitly want `.yamllint.yaml` and
  // `.prettierrc.json` materialized on disk (Fixes #158: previously this
  // never called `sync_native_config::<YamllintConfig>`, so `.yamllint.yaml`
  // was never actually written by `fml sync`).
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let yamllint_res =
      sync_native_config::<YamllintConfig>(ctx, check, start, self.name());
    if !yamllint_res.is_success() {
      return yamllint_res;
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
  use crate::surfaces::{SurfaceStatus, check_binary_exists, test_ctx};
  use tempfile::TempDir;

  #[test]
  fn test_yamllint_config_typed_serialization() {
    let cfg = YamllintConfig {
      extends: "default".to_string(),
      rules: YamllintRulesConfig {
        line_length: YamllintLineLengthRule { max: 120 },
        indentation: YamllintIndentationRule {
          spaces: 4,
          indent_sequences: true,
        },
        document_start: YamllintRuleToggle::Disable,
        truthy: YamllintRuleToggle::Disable,
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("extends: default"));
    assert!(rendered.contains("line-length:"));
    assert!(rendered.contains("max: 120"));
    assert!(rendered.contains("spaces: 4"));
    assert!(rendered.contains("indent-sequences: true"));
    assert!(rendered.contains("document-start: disable"));
    assert!(rendered.contains("truthy: disable"));
  }

  #[test]
  fn test_build_yamllint_parsable_args() {
    let args = build_yamllint_parsable_args(Path::new("config/app.yaml"));
    assert_eq!(
      args,
      vec![
        "-f".to_string(),
        "parsable".to_string(),
        "config/app.yaml".to_string(),
      ]
    );
  }

  #[test]
  fn test_yamllint_config_from_context_rules_disabled_by_default() {
    let temp = TempDir::new().unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("yaml"));
    let cfg = YamllintConfig::from_context(&ctx);
    assert_eq!(cfg.rules.document_start, YamllintRuleToggle::Disable);
    assert_eq!(cfg.rules.truthy, YamllintRuleToggle::Disable);
    assert!(cfg.rules.indentation.indent_sequences);

    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("document-start: disable"));
    assert!(rendered.contains("truthy: disable"));
    assert!(rendered.contains("indent-sequences: true"));
  }

  #[test]
  fn test_yamllint_config_from_context_rules_enabled() {
    let temp = TempDir::new().unwrap();
    let mut lang_cfg = ResolvedLangConfig::new("yaml");
    lang_cfg.yaml = Some(crate::config::YamlOptions {
      indent_sequence: Some(false),
      document_start: Some(true),
      truthy: Some(true),
    });

    let ctx = test_ctx(temp.path(), lang_cfg);
    let cfg = YamllintConfig::from_context(&ctx);
    assert_eq!(cfg.rules.document_start, YamllintRuleToggle::Enable);
    assert_eq!(cfg.rules.truthy, YamllintRuleToggle::Enable);
    assert!(!cfg.rules.indentation.indent_sequences);

    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("document-start: enable"));
    assert!(rendered.contains("truthy: enable"));
    assert!(rendered.contains("indent-sequences: false"));
  }

  #[test]
  fn test_yaml_sync_config_writes_prettier_and_yamllint() {
    let temp = TempDir::new().unwrap();
    let surface = YamlSurface;
    let mut lang_cfg = ResolvedLangConfig::new("yaml");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.yaml = Some(crate::config::YamlOptions {
      indent_sequence: Some(false),
      document_start: Some(true),
      truthy: Some(true),
    });

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));
    assert!(temp.path().join(".prettierrc.json").is_file());

    // Fixes #158: `fml sync` must also materialize `.yamllint.yaml`.
    let yamllint_path = temp.path().join(".yamllint.yaml");
    assert!(yamllint_path.is_file());
    let content = std::fs::read_to_string(&yamllint_path).unwrap();
    assert!(content.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(content.contains("extends: default"));
    assert!(content.contains("max: 100"));
    assert!(content.contains("spaces: 4"));
    assert!(content.contains("indent-sequences: false"));
    assert!(content.contains("document-start: enable"));
    assert!(content.contains("truthy: enable"));
  }

  #[test]
  fn test_build_yamllint_inline_config_shape() {
    let cfg = YamllintConfig {
      extends: "default".to_string(),
      rules: YamllintRulesConfig {
        line_length: YamllintLineLengthRule { max: 120 },
        indentation: YamllintIndentationRule {
          spaces: 4,
          indent_sequences: true,
        },
        document_start: YamllintRuleToggle::Disable,
        truthy: YamllintRuleToggle::Disable,
      },
    };
    let inline = build_yamllint_inline_config(&cfg);
    assert!(inline.contains("max: 120"));
    assert!(inline.contains("spaces: 4"));
    assert!(!inline.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
  }

  #[test]
  fn test_yaml_format_and_lint_do_not_write_native_files() {
    // Fixes #151: `fml fmt`/`fml lint` must not write `.prettierrc.json` or
    // `.yamllint.yaml` as a side effect; only `fml sync` writes those files
    // (see `sync_config`, Fixes #158).
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.yaml"), "a: 1\n").unwrap();

    let surface = YamlSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("yaml"));

    if check_binary_exists("prettier") {
      let _ = surface.format(&ctx);
    }
    if check_binary_exists("yamllint") {
      let _ = surface.lint(&ctx, false);
    }

    assert!(!temp.path().join(".prettierrc.json").exists());
    assert!(!temp.path().join(".yamllint.yaml").exists());
  }
}
