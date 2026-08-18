use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  markdown::sync_prettier_config, serialize_yaml_with_header,
  tool_missing_result,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum YamllintRuleToggle {
  Enable,
  Disable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintLineLengthRule {
  pub max: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintIndentationRule {
  pub spaces: usize,
  #[serde(rename = "indent-sequences")]
  pub indent_sequences: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintRulesConfig {
  #[serde(rename = "line-length")]
  pub line_length: YamllintLineLengthRule,
  pub indentation: YamllintIndentationRule,
  #[serde(rename = "document-start")]
  pub document_start: YamllintRuleToggle,
  pub truthy: YamllintRuleToggle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YamllintConfig {
  pub extends: String,
  pub rules: YamllintRulesConfig,
}

impl NativeConfig for YamllintConfig {
  const FILE_NAME: &'static str = ".yamllint.yaml";
}

impl YamllintConfig {
  #[must_use]
  pub fn from_context(ctx: &ExecutionContext) -> Self {
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

  pub fn render(&self) -> Result<String, serde_yaml::Error> {
    serialize_yaml_with_header(self)
  }
}

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

    if !check_binary_exists("prettier") {
      return tool_missing_result(
        self.name(),
        start,
        "prettier",
        "npm install -g prettier",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      YAML_EXTENSIONS,
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
          let mut cmd = create_tool_command("prettier");
          cmd.arg("--write").arg("--parser").arg("yaml").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(&ctx.root);
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("prettier");
    cmd.arg("--write");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
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
          let msg = if !stdout.trim().is_empty() {
            stdout
          } else if !stderr.trim().is_empty() {
            stderr
          } else {
            "YAML formatting violations found".to_string()
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
          message: format!("Failed to execute prettier: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if fix {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Skipped {
          reason: "Tool does not support autofix; run fml fmt instead"
            .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    if !check_binary_exists("yamllint") {
      return tool_missing_result(
        self.name(),
        start,
        "yamllint",
        "pip install yamllint",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      YAML_EXTENSIONS,
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

    let mut cmd = create_tool_command("yamllint");
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
          message: format!("Failed to execute yamllint: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_prettier_config(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
  use std::sync::Arc;
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
  fn test_yamllint_config_from_context_rules_disabled_by_default() {
    let temp = TempDir::new().unwrap();
    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("yaml"),
      check_only: false,
    };
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

    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: lang_cfg,
      check_only: false,
    };
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
  fn test_yaml_sync_config_delegates_to_prettier() {
    let temp = TempDir::new().unwrap();
    let surface = YamlSurface;
    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("yaml"),
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));
    assert!(temp.path().join(".prettierrc.json").is_file());
  }
}
