use super::facets::LayoutFacet;
use super::options::{
  CppOptions, JsonOptions, MarkdownOptions, PythonOptions, RustOptions,
  TomlOptions, TypstOptions, YamlOptions,
};
use super::{
  CONFIG_FILE_CANDIDATES, ConfigError, FormalityConfig, GlobalConfig,
  ResolvedGlobalConfig, ResolvedLangConfig,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

impl FormalityConfig {
  pub fn empty() -> Self {
    Self {
      global: None,
      lang: BTreeMap::new(),
    }
  }

  pub fn with_defaults() -> Self {
    Self {
      global: Some(GlobalConfig::default()),
      lang: BTreeMap::new(),
    }
  }

  pub fn parse_str(content: &str, path: &Path) -> Result<Self, ConfigError> {
    toml::from_str(content).map_err(|source| ConfigError::Parse {
      path: path.to_path_buf(),
      source,
    })
  }

  pub fn load_file(path: &Path) -> Result<Self, ConfigError> {
    let content =
      fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
      })?;
    Self::parse_str(&content, path)
  }

  pub fn merge(&mut self, other: FormalityConfig) {
    if let Some(other_global) = other.global {
      if let Some(ref mut our_global) = self.global {
        our_global.merge(other_global);
      } else {
        self.global = Some(other_global);
      }
    }

    for (lang_name, lang_cfg) in other.lang {
      self
        .lang
        .entry(lang_name)
        .and_modify(|existing| existing.merge(lang_cfg.clone()))
        .or_insert(lang_cfg);
    }
  }

  pub fn resolve_global(&self) -> ResolvedGlobalConfig {
    let base = GlobalConfig::default();
    let current = self.global.as_ref();
    let current_layout = current.and_then(|g| g.layout.as_ref());

    let indent_size = current
      .and_then(|g| g.indent_size)
      .or_else(|| current_layout.and_then(|l| l.indent_size))
      .unwrap_or(base.indent_size.unwrap_or(2));
    let line_length = current
      .and_then(|g| g.line_length)
      .or_else(|| current_layout.and_then(|l| l.line_length))
      .unwrap_or(base.line_length.unwrap_or(80));
    let end_of_line = current
      .and_then(|g| g.end_of_line.clone())
      .unwrap_or_else(|| base.end_of_line.unwrap_or_else(|| "lf".to_string()));
    let charset = current
      .and_then(|g| g.charset.clone())
      .unwrap_or_else(|| base.charset.unwrap_or_else(|| "utf-8".to_string()));
    let insert_final_newline = current
      .and_then(|g| g.insert_final_newline)
      .unwrap_or(base.insert_final_newline.unwrap_or(true));
    let trim_trailing_whitespace = current
      .and_then(|g| g.trim_trailing_whitespace)
      .unwrap_or(base.trim_trailing_whitespace.unwrap_or(true));
    let use_tabs = current
      .and_then(|g| g.use_tabs)
      .or_else(|| current_layout.and_then(|l| l.use_tabs))
      .unwrap_or(base.use_tabs.unwrap_or(false));
    let prose_wrap = current_layout.and_then(|l| l.prose_wrap.clone());

    let layout = LayoutFacet {
      indent_size: Some(indent_size),
      line_length: Some(line_length),
      use_tabs: Some(use_tabs),
      prose_wrap: prose_wrap.clone(),
    };

    ResolvedGlobalConfig {
      languages: current.and_then(|g| g.languages.clone()),
      ignore_languages: current.and_then(|g| g.ignore_languages.clone()),
      indent_size,
      line_length,
      end_of_line,
      charset,
      insert_final_newline,
      trim_trailing_whitespace,
      use_tabs,
      layout,
      exclude: current.map(|g| g.exclude.clone()).unwrap_or_default(),
    }
  }

  pub fn resolve_for_lang(&self, lang_name: &str) -> ResolvedLangConfig {
    let global = self.resolve_global();
    let lang_cfg = self.lang.get(lang_name);
    let lang_layout = lang_cfg.and_then(|l| l.layout.as_ref());

    let (default_fmt, default_lint) = match lang_name {
      "rust" => (Some("cargo-fmt"), Some("clippy")),
      "python" => (Some("ruff-format"), Some("ruff-check")),
      "cpp" => (Some("clang-format"), Some("clang-tidy")),
      "markdown" => (Some("prettier"), Some("markdownlint")),
      "yaml" => (Some("prettier"), Some("yamllint")),
      "json" => (Some("prettier"), None),
      "toml" => (Some("taplo"), Some("taplo")),
      "typst" => (Some("typstyle"), Some("typstyle")),
      _ => (None, None),
    };

    let indent_size = lang_cfg
      .and_then(|l| l.indent_size)
      .or_else(|| lang_layout.and_then(|l| l.indent_size))
      .unwrap_or(global.indent_size);

    let line_length = lang_cfg
      .and_then(|l| l.line_length)
      .or_else(|| lang_layout.and_then(|l| l.line_length))
      .unwrap_or(global.line_length);

    let use_tabs = lang_cfg
      .and_then(|l| l.use_tabs)
      .or_else(|| lang_layout.and_then(|l| l.use_tabs))
      .unwrap_or(global.use_tabs);

    let prose_wrap = lang_cfg
      .and_then(|l| l.prose_wrap.clone())
      .or_else(|| lang_layout.and_then(|l| l.prose_wrap.clone()))
      .or_else(|| global.layout.prose_wrap.clone());

    let layout = LayoutFacet {
      indent_size: Some(indent_size),
      line_length: Some(line_length),
      use_tabs: Some(use_tabs),
      prose_wrap: prose_wrap.clone(),
    };

    let rust = lang_cfg.and_then(|l| l.rust_options()).or_else(|| {
      if lang_name == "rust" {
        Some(RustOptions::default())
      } else {
        None
      }
    });

    let python = lang_cfg.and_then(|l| l.python_options()).or_else(|| {
      if lang_name == "python" {
        Some(PythonOptions::default())
      } else {
        None
      }
    });

    let cpp = lang_cfg.and_then(|l| l.cpp_options()).or_else(|| {
      if lang_name == "cpp" {
        Some(CppOptions::default())
      } else {
        None
      }
    });

    let markdown = lang_cfg.and_then(|l| l.markdown_options()).or_else(|| {
      if lang_name == "markdown" {
        Some(MarkdownOptions {
          prose_wrap: prose_wrap.clone(),
        })
      } else {
        None
      }
    });

    let yaml = lang_cfg.and_then(|l| l.yaml_options()).or_else(|| {
      if lang_name == "yaml" {
        Some(YamlOptions::default())
      } else {
        None
      }
    });

    let json = lang_cfg.and_then(|l| l.json_options()).or_else(|| {
      if lang_name == "json" {
        Some(JsonOptions::default())
      } else {
        None
      }
    });

    let toml = lang_cfg.and_then(|l| l.toml_options()).or_else(|| {
      if lang_name == "toml" {
        Some(TomlOptions::default())
      } else {
        None
      }
    });

    let typst = lang_cfg.and_then(|l| l.typst_options()).or_else(|| {
      if lang_name == "typst" {
        Some(TypstOptions::default())
      } else {
        None
      }
    });

    let extra = lang_cfg.map(|l| l.extra.clone()).unwrap_or_default();

    ResolvedLangConfig {
      name: lang_name.to_string(),
      format_tool: lang_cfg
        .and_then(|l| l.format_tool.clone())
        .or_else(|| default_fmt.map(|s| s.to_string())),
      lint_tool: lang_cfg
        .and_then(|l| l.lint_tool.clone())
        .or_else(|| default_lint.map(|s| s.to_string())),
      indent_size,
      line_length,
      use_tabs,
      prose_wrap,
      layout,
      enabled: lang_cfg.and_then(|l| l.enabled).unwrap_or(true),
      extra_args: lang_cfg
        .and_then(|l| l.extra_args.clone())
        .unwrap_or_default(),
      files: lang_cfg.and_then(|l| l.files.clone()).unwrap_or_default(),
      exclude: {
        let mut ex = global.exclude.clone();
        if let Some(lang_ex) = lang_cfg.and_then(|l| l.exclude.clone()) {
          ex.extend(lang_ex);
        }
        ex
      },
      rust,
      python,
      cpp,
      markdown,
      yaml,
      json,
      toml,
      typst,
      extra,
    }
  }

  /// Loads configuration with layered resolution:
  /// Embedded defaults -> User config (~/.config/formality/config.toml) -> Project config (formality.toml / .formality.toml)
  pub fn load_layered(
    repo_root: Option<&Path>,
  ) -> Result<(Self, Option<PathBuf>), ConfigError> {
    let mut config = Self::with_defaults();

    // 1. User config (cross-platform: Linux, macOS, Windows)
    if let Some(user_path) = find_user_config()
      && user_path.is_file()
    {
      let user_cfg = Self::load_file(&user_path)?;
      config.merge(user_cfg);
    }

    // 2. Project config
    let project_config_path = if let Some(root) = repo_root {
      find_project_config(root)
    } else if let Ok(cwd) = std::env::current_dir() {
      find_project_config(&cwd)
    } else {
      None
    };

    if let Some(ref proj_path) = project_config_path
      && proj_path.is_file()
    {
      let proj_cfg = Self::load_file(proj_path)?;
      config.merge(proj_cfg);
    }

    Ok((config, project_config_path))
  }

  /// Generates sample configuration template versioned to current package release.
  /// Omits hardcoded `languages = [...]` so `fml` uses built-in auto-detection across
  /// all workspace surfaces by default.
  pub fn generate_sample() -> String {
    let mut out = String::new();
    out.push_str("# formality configuration file\n");
    out.push_str("# https://github.com/arvinduh/formality\n");
    // Reference the schema from the versioned GitHub Release asset — never
    // from a raw git branch URL — so users are always pinned to a specific
    // release rather than an ever-changing main branch.
    out.push_str(&format!(
      "#:schema https://github.com/arvinduh/formality/releases/download/v{}/formality.schema.json\n\n",
      env!("CARGO_PKG_VERSION")
    ));
    out.push_str("[global]\n");
    out.push_str("indent_size = 2\n");
    out.push_str("line_length = 80\n");
    out.push_str("end_of_line = \"lf\"\n");
    out.push_str("charset = \"utf-8\"\n");
    out.push_str("insert_final_newline = true\n");
    out.push_str("trim_trailing_whitespace = true\n");

    out
  }

  /// Generates standard template for `fml init` versioned to current package release.
  pub fn generate_init_template(_detected_langs: &[&str]) -> String {
    Self::generate_sample()
  }
}
pub fn find_project_config(start_dir: &Path) -> Option<PathBuf> {
  let mut current = if start_dir.is_file() {
    start_dir.parent()?.to_path_buf()
  } else {
    start_dir.to_path_buf()
  };

  loop {
    for &candidate_name in CONFIG_FILE_CANDIDATES {
      let candidate = current.join(candidate_name);
      if candidate.is_file() {
        return Some(candidate);
      }
    }

    // Stop if .git is reached
    if current.join(".git").exists() {
      break;
    }

    if !current.pop() {
      break;
    }
  }

  None
}

/// Finds the global user configuration across Linux, macOS, and Windows.
pub fn find_user_config() -> Option<PathBuf> {
  // 1. XDG_CONFIG_HOME (Linux / Custom Unix)
  if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
    let path = PathBuf::from(&xdg_config)
      .join("formality")
      .join("config.toml");
    if path.is_file() {
      return Some(path);
    }
  }

  // 2. APPDATA (Windows)
  if let Ok(app_data) = std::env::var("APPDATA") {
    let path = PathBuf::from(&app_data)
      .join("formality")
      .join("config.toml");
    if path.is_file() {
      return Some(path);
    }
  }

  // 3. HOME directory (Linux, macOS, Unix)
  if let Ok(home) = std::env::var("HOME") {
    let home_path = PathBuf::from(&home);

    // Standard Linux ~/.config/formality/config.toml
    let xdg_fallback = home_path
      .join(".config")
      .join("formality")
      .join("config.toml");
    if xdg_fallback.is_file() {
      return Some(xdg_fallback);
    }

    // macOS ~/Library/Application Support/formality/config.toml
    let mac_path = home_path
      .join("Library")
      .join("Application Support")
      .join("formality")
      .join("config.toml");
    if mac_path.is_file() {
      return Some(mac_path);
    }
  }

  // 4. USERPROFILE (Windows fallback)
  if let Ok(user_profile) = std::env::var("USERPROFILE") {
    let win_fallback = PathBuf::from(user_profile)
      .join(".config")
      .join("formality")
      .join("config.toml");
    if win_fallback.is_file() {
      return Some(win_fallback);
    }
  }

  None
}
