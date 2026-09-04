//! JavaScript/TypeScript language surface: formats and lints via `biome`,
//! syncing the managed `biome.json` from `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo,
  classify_all_nonzero_as_error, create_tool_command,
  diff_check_via_tempcopy_classified, extra_args_set_flag, find_files_with_ext,
  render_native_config, run_tool_command, run_tool_command_classified,
  sync_native_config, tool_missing_guard,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Formatter configuration block for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeFormatterConfig {
  /// Whether the formatter is enabled.
  pub enabled: bool,
  /// Indent style (`"space"` or `"tab"`).
  pub indent_style: String,
  /// Indentation spaces count per level.
  pub indent_width: usize,
  /// Maximum line width.
  pub line_width: usize,
}

/// JS-specific formatter options for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeJsFormatterConfig {
  /// Preferred string quote style.
  pub quote_style: String,
  /// Trailing comma policy.
  pub trailing_commas: String,
  /// Semicolon requirement policy.
  pub semicolons: String,
}

/// JS configuration wrapper for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeJsConfig {
  /// JavaScript formatter configuration.
  pub formatter: BiomeJsFormatterConfig,
}

/// `assist.actions.source.organizeImports` — the modern (Biome >= 2.0) home
/// for import sorting, replacing the removed top-level `organizeImports`
/// config block from Biome 1.x. Value is `"on"` or `"off"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeAssistSourceActions {
  /// Organize imports action state (`"on"` or `"off"`).
  pub organize_imports: String,
}

/// Assist actions sub-block for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeAssistActions {
  /// Source assist actions.
  pub source: BiomeAssistSourceActions,
}

/// Assist configuration block for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeAssistConfig {
  /// Whether assist feature is enabled.
  pub enabled: bool,
  /// Assist actions settings.
  pub actions: BiomeAssistActions,
}

/// Linter rules sub-block for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeLinterRules {
  /// Linter rule preset name (e.g. `"recommended"`).
  pub preset: String,
}

/// Linter configuration block for `biome.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeLinterConfig {
  /// Whether the linter is enabled.
  pub enabled: bool,
  /// Linter rules configuration.
  pub rules: BiomeLinterRules,
}

/// Native `biome.json` configuration representation for JavaScript/TypeScript formatting and linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeConfig {
  /// JSON Schema reference URI.
  #[serde(rename = "$schema")]
  pub schema: String,
  /// Formatter configuration block.
  pub formatter: BiomeFormatterConfig,
  /// JavaScript language options.
  pub javascript: BiomeJsConfig,
  /// Code assist / import sorting configuration.
  pub assist: BiomeAssistConfig,
  /// Linter configuration block.
  pub linter: BiomeLinterConfig,
}

impl NativeConfig for BiomeConfig {
  const FILE_NAME: &'static str = "biome.json";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let indent_style = if ctx.lang_config.use_tabs {
      "tab"
    } else {
      "space"
    };

    let quote_style = ctx
      .lang_config
      .javascript
      .as_ref()
      .and_then(|j| j.quote_style.as_deref())
      .unwrap_or("double");

    let trailing_comma = ctx
      .lang_config
      .javascript
      .as_ref()
      .and_then(|j| j.trailing_comma.as_deref())
      .unwrap_or("all");

    let semicolons = ctx
      .lang_config
      .javascript
      .as_ref()
      .and_then(|j| j.semicolons.as_deref())
      .unwrap_or("always");

    let organize_imports_enabled = ctx
      .lang_config
      .javascript
      .as_ref()
      .and_then(|j| j.organize_imports)
      .unwrap_or(true);

    Self {
      schema: "https://biomejs.dev/schemas/2.0.0/schema.json".to_string(),
      formatter: BiomeFormatterConfig {
        enabled: true,
        indent_style: indent_style.to_string(),
        indent_width: ctx.lang_config.indent_size,
        line_width: ctx.lang_config.line_length,
      },
      javascript: BiomeJsConfig {
        formatter: BiomeJsFormatterConfig {
          quote_style: quote_style.to_string(),
          trailing_commas: trailing_comma.to_string(),
          semicolons: semicolons.to_string(),
        },
      },
      assist: BiomeAssistConfig {
        enabled: true,
        actions: BiomeAssistActions {
          source: BiomeAssistSourceActions {
            organize_imports: if organize_imports_enabled {
              "on".to_string()
            } else {
              "off".to_string()
            },
          },
        },
      },
      linter: BiomeLinterConfig {
        enabled: true,
        rules: BiomeLinterRules {
          preset: "recommended".to_string(),
        },
      },
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// JavaScript/TypeScript language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct JavaScriptSurface;

impl DeclaresFacets for JavaScriptSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs
      | Facet::IndentWidth
      | Facet::LineLength
      | Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ImportSort => FacetSupport::Configurable,
      Facet::ProseWrap | Facet::Edition | Facet::Standard => {
        FacetSupport::Unsupported
      }
    }
  }
}

/// Standard file extensions recognized for JavaScript and TypeScript source files.
pub const JS_TS_EXTENSIONS: &[&str] =
  &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

/// Renders the resolved [`BiomeConfig`]'s formatting-layout settings as the
/// inline `--indent-style`/`--line-width`/etc. flags `biome check`/`biome
/// format` accept, so `fml fmt` can apply formality.toml's settings without
/// writing `biome.json` to disk (Fixes #151). Only `fml sync` writes that
/// file now (see [`JavaScriptSurface::sync_config`]). This covers the
/// formatting-layout options only — the linter preset and the
/// `assist.actions.source.organizeImports` toggle have no equivalent
/// single-action inline flag (only the coarser `--assist-enabled` /
/// `--javascript-assist-enabled`), so those two stay config-file-only;
/// biome's own defaults for both already match what `BiomeConfig::default`
/// would render, so this is a low-risk gap, not a functional regression.
#[must_use]
pub fn build_biome_inline_format_args(cfg: &BiomeConfig) -> Vec<String> {
  vec![
    format!("--indent-style={}", cfg.formatter.indent_style),
    format!("--indent-width={}", cfg.formatter.indent_width),
    format!("--line-width={}", cfg.formatter.line_width),
    format!(
      "--javascript-formatter-quote-style={}",
      cfg.javascript.formatter.quote_style
    ),
    format!(
      "--trailing-commas={}",
      cfg.javascript.formatter.trailing_commas
    ),
    format!("--semicolons={}", cfg.javascript.formatter.semicolons),
  ]
}

/// The biome flag `fml fmt` passes to keep the linter out of the Smart Format
/// pass, and the value it passes it with. Named because the format path both
/// passes it and refuses an `extra_args` override of it (see
/// [`linter_enabled_override_message`]).
const BIOME_LINTER_ENABLED_FLAG: &str = "--linter-enabled";
/// The value [`BIOME_LINTER_ENABLED_FLAG`] is passed with on the format path.
const BIOME_LINTER_ENABLED_VALUE: &str = "false";

/// Builds the message for an `extra_args` entry that sets biome's
/// `--linter-enabled` on the format path, quoting the offending argument back
/// at the user.
///
/// Fixes #173: the format path always passes `--linter-enabled=false` itself,
/// and biome rejects the flag given twice (`argument --linter-enabled cannot
/// be used multiple times in this context`, exit 1) — reproduced against the
/// pinned `@biomejs/biome@2.5.10`. So *no* spelling in `extra_args` ever
/// worked: `=true` never re-enabled the linter and `=false` never restated a
/// default, because biome refuses the duplicate before it parses either value.
/// Both already produced an `[ERR] Execution error` that was accurate — biome
/// genuinely could not run — but named neither the flag nor `extra_args`. This
/// refusal replaces an opaque tool error with an actionable explanation; it
/// does not correct a misclassified exit code, because there is no lint
/// finding on this path to misclassify. See [`extra_args_set_flag`].
fn linter_enabled_override_message(offending: &str) -> String {
  format!(
    "`[lang.javascript] extra_args` contains `{offending}`, but `fml fmt` \
     already passes `{BIOME_LINTER_ENABLED_FLAG}={BIOME_LINTER_ENABLED_VALUE}` \
     to `biome check --write` — and biome rejects that flag given twice.\n\n\
     No value works here. Because `fml` supplies the flag itself, biome \
     rejects the duplicate before reading either value: it exits with an \
     error, formats nothing, and names neither `extra_args` nor the \
     duplicated flag. `--linter-enabled=true` therefore never re-enables the \
     linter, and `--linter-enabled=false` never restates a default — both \
     simply break the format pass.\n\nRemove \
     `{BIOME_LINTER_ENABLED_FLAG}` from `extra_args` and run linting through \
     `fml lint` (which is where biome's linter belongs) instead."
  )
}

/// Builds the argument list for the "Smart Format" pass: `biome check --write`
/// with the linter disabled so this step only applies formatting and (per
/// `biome.json`'s `organizeImports.enabled`) import sorting — never lint fixes.
/// Linting itself is handled separately by `lint()`.
#[must_use]
pub fn build_biome_format_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec![
    "check".to_string(),
    "--write".to_string(),
    format!("{BIOME_LINTER_ENABLED_FLAG}={BIOME_LINTER_ENABLED_VALUE}"),
  ];
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for biome lint invocation.
#[must_use]
pub fn build_biome_lint_args(
  files: &[PathBuf],
  fix: bool,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["lint".to_string()];
  if fix {
    args.push("--write".to_string());
  }
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds the argument vector for `biome lint --reporter=json <file>`, used
/// by `fml lsp`'s structured-diagnostics path (Fixes #165) to get
/// machine-readable per-violation output instead of parsing biome's
/// human-readable terminal report. `--reporter=json` is marked experimental
/// by biome as of 2.x but its diagnostic shape (`diagnostics[].location.path`
/// / `.start`/`.end` `{line, column}`, both 1-based) has been stable across
/// the versions this was verified against.
#[must_use]
pub fn build_biome_lint_json_args(file: &Path) -> Vec<String> {
  vec![
    "lint".to_string(),
    "--reporter=json".to_string(),
    file.to_string_lossy().to_string(),
  ]
}

impl LanguageSurface for JavaScriptSurface {
  fn name(&self) -> &'static str {
    "javascript"
  }

  fn aliases(&self) -> &[&'static str] {
    &["js", "ts", "typescript", "jsx", "tsx"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    JS_TS_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("biome.json").is_file()
      || root.join("biome.jsonc").is_file()
      || root.join("tsconfig.json").is_file()
      || root.join("package.json").is_file()
      || !find_files_with_ext(root, JS_TS_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "biome",
      description: "Fast formatter and linter for JavaScript, TypeScript, JSX and TSX",
      install_hint: "Install via: npm install -g @biomejs/biome (or pnpm add -g / yarn global add / bun add -g / brew install biome)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    let files = ctx.matched_files(JS_TS_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Fixes #173: refuse, with an explanation, rather than hand biome an
    // argv it rejects with an error that explains nothing. Only checked here,
    // not in `lint()`: `--linter-enabled` is a flag the format path passes
    // itself, and biome's linter is exactly what `fml lint` is supposed to
    // run. Deliberately *above* `tool_missing_guard`: a malformed
    // `formality.toml` is wrong regardless of whether biome happens to be
    // installed, and reporting the config error first is the more useful
    // ordering (it also makes this guard's tests hermetic).
    if let Some(offending) = extra_args_set_flag(
      BIOME_LINTER_ENABLED_FLAG,
      &ctx.lang_config.extra_args,
    ) {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: linter_enabled_override_message(&offending),
        },
        duration: start.elapsed(),
      };
    }

    if let Some(res) = tool_missing_guard(
      self.name(),
      "biome",
      start,
      Some("npm install -g @biomejs/biome"),
    ) {
      return res;
    }

    // Inline `--indent-style`/`--line-width`/etc. instead of writing
    // `biome.json` to disk — see `build_biome_inline_format_args` (Fixes
    // #151). `fml sync` remains the only path that materializes the file.
    let inline_config =
      build_biome_inline_format_args(&BiomeConfig::from_context(ctx));

    if ctx.check_only {
      return diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("biome");
          cmd.args(build_biome_format_args(
            &[scratch.to_path_buf()],
            &ctx.lang_config.extra_args,
          ));
          cmd.args(&inline_config);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        // `biome check --write` (linter disabled) rewrites the scratch copy
        // in place and exits 0 whether or not it reformatted anything; it
        // only exits non-zero on an operational failure it cannot fix — a
        // parse error, an unreadable file, a bad `--config`. There is no
        // "found drift" exit code on this path (formatting drift is detected
        // by diffing the file), so every non-zero exit is an
        // `ExecutionError` (Fixes #151). Same reasoning applies verbatim to
        // the non-`--check` write branch below (Fixes #155): `biome check
        // --write` has no in-place-write variant of a "found drift" exit
        // code either.
        classify_all_nonzero_as_error,
      );
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = create_tool_command("biome");
    cmd.args(build_biome_format_args(
      &files_to_pass,
      &ctx.lang_config.extra_args,
    ));
    cmd.args(&inline_config);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command_classified(
      self.name(),
      &mut cmd,
      classify_all_nonzero_as_error,
    )
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "biome",
      start,
      Some("npm install -g @biomejs/biome"),
    ) {
      return res;
    }

    let files = ctx.matched_files(JS_TS_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = create_tool_command("biome");
    cmd.args(build_biome_lint_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt` no longer goes through this path for the formatting-layout
  // options (Fixes #151): it passes them to biome inline (see
  // `build_biome_inline_format_args`, used in `format()` above). The
  // linter preset and `organizeImports` toggle have no equivalent
  // single-action CLI flag (see that function's doc comment for why), so
  // `fml lint` still relies on biome's own defaults there rather than on
  // this file. This method is now reached only by `fml sync`, for users who
  // explicitly want `biome.json` materialized on disk.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<BiomeConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{JavaScriptOptions, ResolvedLangConfig};
  use crate::surfaces::{SurfaceStatus, check_binary_exists, test_ctx};
  use tempfile::TempDir;

  #[test]
  fn test_build_biome_format_args_default_and_with_files() {
    let no_files = build_biome_format_args(&[], &[]);
    assert_eq!(
      no_files,
      vec![
        "check".to_string(),
        "--write".to_string(),
        "--linter-enabled=false".to_string(),
        ".".to_string(),
      ]
    );

    let files = vec![PathBuf::from("a.ts"), PathBuf::from("b.tsx")];
    let extra = vec!["--no-errors-on-unmatched".to_string()];
    let with_files = build_biome_format_args(&files, &extra);
    assert_eq!(
      with_files,
      vec![
        "check".to_string(),
        "--write".to_string(),
        "--linter-enabled=false".to_string(),
        "a.ts".to_string(),
        "b.tsx".to_string(),
        "--no-errors-on-unmatched".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_biome_lint_json_args() {
    let args = build_biome_lint_json_args(Path::new("src/a.ts"));
    assert_eq!(
      args,
      vec![
        "lint".to_string(),
        "--reporter=json".to_string(),
        "src/a.ts".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_biome_lint_args_with_and_without_fix() {
    let no_fix = build_biome_lint_args(&[], false, &[]);
    assert_eq!(no_fix, vec!["lint".to_string(), ".".to_string()]);

    let files = vec![PathBuf::from("a.js")];
    let extra = vec!["--max-diagnostics=50".to_string()];
    let with_fix = build_biome_lint_args(&files, true, &extra);
    assert_eq!(
      with_fix,
      vec![
        "lint".to_string(),
        "--write".to_string(),
        "a.js".to_string(),
        "--max-diagnostics=50".to_string(),
      ]
    );
  }

  #[test]
  fn test_javascript_surface_file_extensions_and_aliases() {
    let surface = JavaScriptSurface;
    assert_eq!(
      surface.file_extensions(),
      &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"]
    );
    assert!(surface.aliases().contains(&"ts"));
    assert!(surface.aliases().contains(&"js"));
    assert!(surface.supports_lint_fix());
  }

  #[test]
  fn test_javascript_surface_detect() {
    let temp = TempDir::new().unwrap();
    let surface = JavaScriptSurface;
    assert!(!surface.detect(temp.path()));

    let file = temp.path().join("index.ts");
    std::fs::write(&file, "export const x: number = 1;\n").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_biome_config_typed_serialization() {
    let cfg = BiomeConfig {
      schema: "https://biomejs.dev/schemas/1.5.0/schema.json".to_string(),
      formatter: BiomeFormatterConfig {
        enabled: true,
        indent_style: "space".to_string(),
        indent_width: 2,
        line_width: 80,
      },
      javascript: BiomeJsConfig {
        formatter: BiomeJsFormatterConfig {
          quote_style: "single".to_string(),
          trailing_commas: "all".to_string(),
          semicolons: "always".to_string(),
        },
      },
      assist: BiomeAssistConfig {
        enabled: true,
        actions: BiomeAssistActions {
          source: BiomeAssistSourceActions {
            organize_imports: "on".to_string(),
          },
        },
      },
      linter: BiomeLinterConfig {
        enabled: true,
        rules: BiomeLinterRules {
          preset: "recommended".to_string(),
        },
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$schema\""));
    assert!(rendered.contains("\"indentWidth\": 2"));
    assert!(rendered.contains("\"lineWidth\": 80"));
    assert!(rendered.contains("\"quoteStyle\": \"single\""));
    assert!(rendered.contains("\"trailingCommas\": \"all\""));
    assert!(rendered.contains("\"assist\""));
    assert!(rendered.contains("\"organizeImports\": \"on\""));
    assert!(rendered.contains("\"linter\""));
    assert!(rendered.contains("\"preset\": \"recommended\""));
  }

  #[test]
  fn test_javascript_sync_config_from_context() {
    let temp = TempDir::new().unwrap();
    let surface = JavaScriptSurface;
    let mut lang_cfg = ResolvedLangConfig::new("javascript");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.use_tabs = true;
    lang_cfg.javascript = Some(JavaScriptOptions {
      quote_style: Some("single".to_string()),
      trailing_comma: Some("es5".to_string()),
      semicolons: Some("asNeeded".to_string()),
      organize_imports: Some(true),
    });

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join("biome.json");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("\"indentStyle\": \"tab\""));
    assert!(content.contains("\"indentWidth\": 4"));
    assert!(content.contains("\"lineWidth\": 100"));
    assert!(content.contains("\"quoteStyle\": \"single\""));
    assert!(content.contains("\"trailingCommas\": \"es5\""));
    assert!(content.contains("\"assist\""));
    assert!(content.contains("\"organizeImports\": \"on\""));
    assert!(content.contains("\"enabled\": true"));
  }

  #[test]
  fn test_javascript_facet_declarations() {
    let surface = JavaScriptSurface;
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::TrailingComma),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::Standard),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_build_biome_inline_format_args_shape() {
    let temp = TempDir::new().unwrap();
    let mut lang_cfg = ResolvedLangConfig::new("javascript");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.javascript = Some(JavaScriptOptions {
      quote_style: Some("single".to_string()),
      trailing_comma: Some("es5".to_string()),
      semicolons: Some("as-needed".to_string()),
      organize_imports: Some(true),
    });
    let ctx = test_ctx(temp.path(), lang_cfg);
    let cfg = BiomeConfig::from_context(&ctx);
    let args = build_biome_inline_format_args(&cfg);
    assert!(args.contains(&"--indent-width=4".to_string()));
    assert!(args.contains(&"--line-width=100".to_string()));
    assert!(
      args.contains(&"--javascript-formatter-quote-style=single".to_string())
    );
    assert!(args.contains(&"--trailing-commas=es5".to_string()));
    assert!(args.contains(&"--semicolons=as-needed".to_string()));
  }

  #[test]
  fn test_javascript_format_does_not_write_biome_json() {
    // Fixes #151: `fml fmt` must not write `biome.json` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !check_binary_exists("biome") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.js"), "const x=1;\n").unwrap();

    let surface = JavaScriptSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("javascript"));

    let _ = surface.format(&ctx);

    assert!(!temp.path().join("biome.json").exists());
    assert!(!temp.path().join("biome.jsonc").exists());
  }

  #[test]
  fn test_javascript_check_reports_execution_error_on_formatter_failure() {
    // Fixes #151: when biome cannot format a file on the `fml fmt --check`
    // path (here: a syntax error it has no way to parse or rewrite), the
    // surface must classify that as `ExecutionError` (`[ERR]`), never as a
    // lint-style `ViolationsFound` (`[FAIL]`). `biome check --write` has no
    // "found drift" exit code, so a non-zero exit is always operational.
    if !check_binary_exists("biome") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("broken.ts"),
      "const x: = = ;\nfunction (( {\n",
    )
    .unwrap();

    let surface = JavaScriptSurface;
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("javascript"));
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on --check must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn test_javascript_write_reports_execution_error_on_formatter_failure() {
    // Fixes #155: the non-`--check` write path must classify the same
    // operational biome failure as `ExecutionError`, not `ViolationsFound`
    // — mirroring `test_javascript_check_reports_execution_error_on_formatter_failure`
    // above.
    if !check_binary_exists("biome") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("broken.ts"),
      "const x: = = ;\nfunction (( {\n",
    )
    .unwrap();

    let surface = JavaScriptSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("javascript"));

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  /// Builds a context over a lone clean `.ts` file whose `extra_args` carry
  /// `extra`, for the `--linter-enabled` guard tests below.
  fn ctx_with_extra_args(temp: &TempDir, extra: &[&str]) -> ExecutionContext {
    std::fs::write(temp.path().join("a.ts"), "const x = 1;\n").unwrap();
    let mut lang = ResolvedLangConfig::new("javascript");
    lang.extra_args = extra.iter().map(|s| (*s).to_string()).collect();
    test_ctx(temp.path(), lang)
  }

  /// Asserts `res` is the `--linter-enabled` override refusal, not some other
  /// `ExecutionError` (a real biome failure would also be `ExecutionError`,
  /// so matching on the variant alone would be a vacuous assertion).
  fn assert_linter_override_refusal(res: &SurfaceResult) {
    match &res.status {
      SurfaceStatus::ExecutionError { message } => {
        assert!(
          message.contains("--linter-enabled")
            && message.contains("extra_args")
            && message.contains("fml lint"),
          "diagnostic must name the flag, where it came from, and the way \
           out; got: {message}"
        );
      }
      other => panic!("expected ExecutionError, got {other:?}"),
    }
    assert!(!res.is_success());
  }

  #[test]
  fn test_javascript_format_refuses_extra_args_linter_enabled_override() {
    // Fixes #173: `--linter-enabled=true` in `extra_args` does *not* re-enable
    // biome's linter — the format path passes the flag itself, and biome
    // rejects the duplicate outright (verified against the pinned
    // `@biomejs/biome@2.5.10`). What the user got before was an accurate but
    // opaque `[ERR]` naming neither `extra_args` nor the flag; the surface now
    // refuses up front with a diagnostic that explains the cause. Hermetic:
    // the guard runs before `tool_missing_guard`, so no biome install is
    // needed. Asserted on both the `--check` and write branches, which pass
    // the flag alike.
    let temp = TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--linter-enabled=true"]);
    assert_linter_override_refusal(&JavaScriptSurface.format(&ctx));

    let mut check_ctx = ctx_with_extra_args(&temp, &["--linter-enabled=true"]);
    check_ctx.check_only = true;
    assert_linter_override_refusal(&JavaScriptSurface.format(&check_ctx));
  }

  #[test]
  fn test_javascript_format_refuses_redundant_linter_enabled_restatement() {
    // Fixes #173: `--linter-enabled=false` looks like a harmless restatement
    // of what `fml fmt` already passes, but biome rejects the duplicate flag
    // outright ("argument `--linter-enabled` cannot be used multiple times in
    // this context") before it parses the value — so this spelling was just
    // as broken as `=true`, and gets the same explanation rather than being
    // let through. Hermetic, per the guard's placement above the tool guard.
    let temp = TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--linter-enabled=false"]);
    assert_linter_override_refusal(&JavaScriptSurface.format(&ctx));
  }

  #[test]
  fn test_javascript_format_allows_unrelated_extra_args() {
    // The #173 guard is narrow: only the one flag `fml` passes itself is
    // refused. An unrelated `extra_args` entry must still format normally, or
    // the guard would be a regression for every other user.
    if !check_binary_exists("biome") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--no-errors-on-unmatched"]);
    let res = JavaScriptSurface.format(&ctx);
    assert!(
      res.is_success(),
      "an unrelated extra_args entry must still format, got: {:?}",
      res.status
    );
  }

  #[test]
  fn test_linter_enabled_override_message_quotes_the_offending_argument() {
    // Hermetic (no biome needed): the diagnostic echoes the argument as the
    // user wrote it, so it is greppable in their own `formality.toml`.
    let message = linter_enabled_override_message("--linter-enabled true");
    assert!(
      message.contains("`--linter-enabled true`"),
      "got: {message}"
    );
    assert!(message.contains("--linter-enabled=false"), "got: {message}");
  }
}
