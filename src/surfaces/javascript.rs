use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  serialize_json_pretty, sync_file_helper, tool_missing_result,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeFormatterConfig {
  pub enabled: bool,
  pub indent_style: String,
  pub indent_width: usize,
  pub line_width: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeJsFormatterConfig {
  pub quote_style: String,
  pub trailing_commas: String,
  pub semicolons: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeJsConfig {
  pub formatter: BiomeJsFormatterConfig,
}

/// `assist.actions.source.organizeImports` — the modern (Biome >= 2.0) home
/// for import sorting, replacing the removed top-level `organizeImports`
/// config block from Biome 1.x. Value is `"on"` or `"off"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeAssistSourceActions {
  pub organize_imports: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeAssistActions {
  pub source: BiomeAssistSourceActions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeAssistConfig {
  pub enabled: bool,
  pub actions: BiomeAssistActions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BiomeLinterRules {
  pub preset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeLinterConfig {
  pub enabled: bool,
  pub rules: BiomeLinterRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BiomeConfig {
  #[serde(rename = "$schema")]
  pub schema: String,
  pub formatter: BiomeFormatterConfig,
  pub javascript: BiomeJsConfig,
  pub assist: BiomeAssistConfig,
  pub linter: BiomeLinterConfig,
}

impl NativeConfig for BiomeConfig {
  const FILE_NAME: &'static str = "biome.json";
}

impl BiomeConfig {
  #[must_use]
  pub fn from_context(ctx: &ExecutionContext) -> Self {
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

  pub fn render(&self) -> Result<String, serde_json::Error> {
    serialize_json_pretty(self)
  }
}

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

pub const JS_TS_EXTENSIONS: &[&str] =
  &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

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
    "--linter-enabled=false".to_string(),
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

    if !check_binary_exists("biome") {
      return tool_missing_result(
        self.name(),
        start,
        "biome",
        "npm install -g @biomejs/biome",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      JS_TS_EXTENSIONS,
      &ctx.paths,
      &ctx.lang_config.files,
      &ctx.lang_config.exclude,
    );
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("biome");
          cmd.args(build_biome_format_args(
            &[scratch.to_path_buf()],
            &ctx.lang_config.extra_args,
          ));
          cmd.current_dir(&ctx.root);
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let files_to_pass = if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      files
    } else {
      Vec::new()
    };

    let mut cmd = create_tool_command("biome");
    cmd.args(build_biome_format_args(
      &files_to_pass,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(&ctx.root);

    match cmd.output() {
      Ok(output) => {
        if output.status.success() {
          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::Passed,
            duration: start.elapsed(),
          }
        } else {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Formatting issues found in JavaScript/TypeScript files".to_string()
          };

          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ViolationsFound {
              message: msg,
              diff: None,
            },
            duration: start.elapsed(),
          }
        }
      }
      Err(e) => SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to execute biome check: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("biome") {
      return tool_missing_result(
        self.name(),
        start,
        "biome",
        "npm install -g @biomejs/biome",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      JS_TS_EXTENSIONS,
      &ctx.paths,
      &ctx.lang_config.files,
      &ctx.lang_config.exclude,
    );
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let files_to_pass = if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      files
    } else {
      Vec::new()
    };

    let mut cmd = create_tool_command("biome");
    cmd.args(build_biome_lint_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(&ctx.root);

    match cmd.output() {
      Ok(output) => {
        if output.status.success() {
          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::Passed,
            duration: start.elapsed(),
          }
        } else {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = if stderr.trim().is_empty() {
            stdout
          } else {
            stderr
          };

          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ViolationsFound {
              message: msg,
              diff: None,
            },
            duration: start.elapsed(),
          }
        }
      }
      Err(e) => SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to execute biome lint: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join(BiomeConfig::FILE_NAME);
    let cfg = BiomeConfig::from_context(ctx);
    let content = match cfg.render() {
      Ok(c) => c,
      Err(e) => {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to serialize {}: {}",
              BiomeConfig::FILE_NAME,
              e
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    sync_file_helper(
      &target,
      BiomeConfig::FILE_NAME,
      &content,
      check,
      start,
      self.name(),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::{
    JavaScriptOptions, ResolvedGlobalConfig, ResolvedLangConfig,
  };
  use std::sync::Arc;
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

    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: lang_cfg,
      check_only: false,
    };

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
}
