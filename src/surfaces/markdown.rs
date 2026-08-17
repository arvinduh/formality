use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  sync_file_helper,
};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownSurface;

impl DeclaresFacets for MarkdownSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Configurable,
      Facet::IndentWidth => FacetSupport::Configurable,
      Facet::LineLength => FacetSupport::Configurable,
      Facet::QuoteStyle => FacetSupport::Unsupported,
      Facet::TrailingComma => FacetSupport::Unsupported,
      Facet::ImportSort => FacetSupport::Unsupported,
      Facet::ProseWrap => FacetSupport::Configurable,
      Facet::Edition => FacetSupport::Unsupported,
      Facet::Standard => FacetSupport::Unsupported,
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

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("prettier") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "prettier".to_string(),
          install_hint: "npm install -g prettier".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(
      &ctx.root,
      MD_EXTENSIONS,
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
          cmd
            .arg("--write")
            .arg("--parser")
            .arg("markdown")
            .arg(scratch);
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
            "Markdown formatting violations found".to_string()
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
          message: format!("Failed to execute prettier: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    let (binary, has_fix_flag) = if check_binary_exists("markdownlint-cli2") {
      ("markdownlint-cli2", true)
    } else if check_binary_exists("markdownlint") {
      ("markdownlint", true)
    } else {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "markdownlint-cli2".to_string(),
          install_hint: "npm install -g markdownlint-cli2".to_string(),
        },
        duration: start.elapsed(),
      };
    };

    let files = find_files_with_ext(
      &ctx.root,
      MD_EXTENSIONS,
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

    let mut cmd = create_tool_command(binary);
    if fix && has_fix_flag {
      cmd.arg("--fix");
    }

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
          message: format!("Failed to execute {}: {}", binary, e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join(".markdownlint.json");

    let content = format!(
      "{{\n  \"$comment\": {{\n    \"description\": \"WARNING: DO NOT EDIT THIS FILE DIRECTLY! Automatically generated and managed by formality (fml). Canonical source of truth: formality.toml. Run 'fml sync' to update.\"\n  }},\n  \"default\": true,\n  \"MD013\": {{\n    \"line_length\": {},\n    \"code_blocks\": false,\n    \"tables\": false\n  }}\n}}\n",
      ctx.lang_config.line_length
    );

    let md_res = sync_file_helper(
      &target,
      ".markdownlint.json",
      &content,
      check,
      start,
      self.name(),
    );
    if !md_res.is_success() {
      return md_res;
    }

    // Also sync .prettierrc.json
    sync_prettier_config(ctx, check, start, self.name())
  }
}

pub fn sync_prettier_config(
  ctx: &ExecutionContext,
  check: bool,
  start: Instant,
  surface_name: &'static str,
) -> SurfaceResult {
  let target = ctx.root.join(".prettierrc.json");
  let eol = match ctx.global_config.end_of_line.to_lowercase().as_str() {
    "crlf" => "crlf",
    "cr" => "cr",
    _ => "lf",
  };
  let prose_wrap = ctx.lang_config.prose_wrap.as_deref().unwrap_or("always");

  let content = format!(
    "{{\n  \"$comment\": \"WARNING: DO NOT EDIT THIS FILE DIRECTLY! Automatically generated and managed by formality (fml). Canonical source of truth: formality.toml. Run 'fml sync' to update.\",\n  \"tabWidth\": {},\n  \"printWidth\": {},\n  \"useTabs\": {},\n  \"endOfLine\": \"{}\",\n  \"proseWrap\": \"{}\"\n}}\n",
    ctx.lang_config.indent_size,
    ctx.lang_config.line_length,
    ctx.lang_config.use_tabs,
    eol,
    prose_wrap
  );

  sync_file_helper(
    &target,
    ".prettierrc.json",
    &content,
    check,
    start,
    surface_name,
  )
}
