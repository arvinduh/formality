use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  serialize_toml_with_header, sync_file_helper,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct RuffFormatConfig {
  pub indent_style: String,
  pub quote_style: String,
  pub line_ending: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuffLintConfig {
  pub select: Vec<String>,
  pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct RuffConfig {
  pub line_length: usize,
  pub indent_width: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target_version: Option<String>,
  pub format: RuffFormatConfig,
  pub lint: RuffLintConfig,
}

impl NativeConfig for RuffConfig {
  const FILE_NAME: &'static str = "ruff.toml";
}

impl RuffConfig {
  pub fn from_context(ctx: &ExecutionContext) -> Self {
    let indent_style = if ctx.lang_config.use_tabs {
      "tab"
    } else {
      "space"
    };
    let line_ending =
      match ctx.global_config.end_of_line.to_lowercase().as_str() {
        "crlf" => "crlf",
        "cr" => "cr",
        _ => "lf",
      };

    let quote_style = ctx
      .lang_config
      .python
      .as_ref()
      .and_then(|p| p.quote_style.as_deref())
      .unwrap_or("double");

    let target_version = ctx
      .lang_config
      .python
      .as_ref()
      .and_then(|p| p.target_version.clone());

    Self {
      line_length: ctx.lang_config.line_length,
      indent_width: ctx.lang_config.indent_size,
      target_version,
      format: RuffFormatConfig {
        indent_style: indent_style.to_string(),
        quote_style: quote_style.to_string(),
        line_ending: line_ending.to_string(),
      },
      lint: RuffLintConfig {
        select: vec![
          "E".to_string(),
          "F".to_string(),
          "I".to_string(),
          "UP".to_string(),
          "B".to_string(),
          "SIM".to_string(),
        ],
        ignore: vec![],
      },
    }
  }

  pub fn render(&self) -> Result<String, toml::ser::Error> {
    serialize_toml_with_header(self)
  }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PythonSurface;

impl DeclaresFacets for PythonSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Configurable,
      Facet::IndentWidth => FacetSupport::Configurable,
      Facet::LineLength => FacetSupport::Configurable,
      Facet::QuoteStyle => FacetSupport::Configurable,
      Facet::TrailingComma => FacetSupport::Unsupported,
      Facet::ImportSort => FacetSupport::Configurable,
      Facet::ProseWrap => FacetSupport::Unsupported,
      Facet::Edition => FacetSupport::Unsupported,
      Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

impl LanguageSurface for PythonSurface {
  fn name(&self) -> &'static str {
    "python"
  }

  fn aliases(&self) -> &[&'static str] {
    &["py"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    &["py"]
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("pyproject.toml").is_file()
      || root.join("requirements.txt").is_file()
      || root.join("setup.py").is_file()
      || root.join("Pipfile").is_file()
      || root.join("ruff.toml").is_file()
      || root.join(".ruff.toml").is_file()
      || !find_files_with_ext(root, &["py"], &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "ruff",
      description: "Fast Python linter and code formatter",
      install_hint: "Install via: uv tool install ruff (or pip install ruff / brew install ruff / cargo binstall ruff)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("ruff") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "ruff".to_string(),
          install_hint: "pip install ruff".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(
      &ctx.root,
      &["py"],
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
          let mut cmd = create_tool_command("ruff");
          cmd.arg("format").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(&ctx.root);
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("ruff");
    cmd.arg("format");

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
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Formatting issues found in Python files".to_string()
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
          message: format!("Failed to execute ruff format: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("ruff") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "ruff".to_string(),
          install_hint: "pip install ruff".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(
      &ctx.root,
      &["py"],
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

    let mut cmd = create_tool_command("ruff");
    cmd.arg("check");
    if fix {
      cmd.arg("--fix");
    }

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
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else {
            stdout
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
          message: format!("Failed to execute ruff check: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join(RuffConfig::FILE_NAME);
    let cfg = RuffConfig::from_context(ctx);
    let content = match cfg.render() {
      Ok(c) => c,
      Err(e) => {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to serialize {}: {}",
              RuffConfig::FILE_NAME,
              e
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    sync_file_helper(
      &target,
      RuffConfig::FILE_NAME,
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
    PythonOptions, ResolvedGlobalConfig, ResolvedLangConfig,
  };
  use tempfile::TempDir;

  #[test]
  fn test_python_sync_config_lint_table_and_quote_style() {
    let temp = TempDir::new().unwrap();
    let surface = PythonSurface;
    let mut lang_cfg = ResolvedLangConfig::new("python");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.python = Some(PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py312".to_string()),
    });

    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Vec::new(),
      global_config: ResolvedGlobalConfig::default(),
      lang_config: lang_cfg,
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join("ruff.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[lint]"));
    assert!(content.contains("select = ["));
    assert!(content.contains("\"E\""));
    assert!(content.contains("\"SIM\""));
    assert!(content.contains("quote-style = \"single\""));
    assert!(content.contains("target-version = \"py312\""));
    assert!(content.contains("line-length = 100"));
    assert!(content.contains("indent-width = 4"));
  }
  #[test]
  fn test_ruff_config_typed_serialization() {
    let cfg = RuffConfig {
      line_length: 100,
      indent_width: 4,
      target_version: Some("py311".to_string()),
      format: RuffFormatConfig {
        indent_style: "space".to_string(),
        quote_style: "single".to_string(),
        line_ending: "lf".to_string(),
      },
      lint: RuffLintConfig {
        select: vec!["E".to_string(), "F".to_string()],
        ignore: vec![],
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("line-length = 100"));
    assert!(rendered.contains("indent-width = 4"));
    assert!(rendered.contains("target-version = \"py311\""));
    assert!(rendered.contains("[format]"));
    assert!(rendered.contains("quote-style = \"single\""));
    assert!(rendered.contains("[lint]"));
  }
}
