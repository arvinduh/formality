use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_FILE_NAME: &str = "formality.toml";
pub const CONFIG_FILE_CANDIDATES: &[&str] =
  &["formality.toml", ".formality.toml"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalConfig {
  pub languages: Option<Vec<String>>,
  pub ignore_languages: Option<Vec<String>>,
  pub indent_size: Option<usize>,
  pub line_length: Option<usize>,
  pub end_of_line: Option<String>,
  pub charset: Option<String>,
  pub insert_final_newline: Option<bool>,
  pub trim_trailing_whitespace: Option<bool>,
  pub use_tabs: Option<bool>,
}

impl Default for GlobalConfig {
  fn default() -> Self {
    Self {
      languages: None,
      ignore_languages: None,
      indent_size: Some(2),
      line_length: Some(80),
      end_of_line: Some("lf".to_string()),
      charset: Some("utf-8".to_string()),
      insert_final_newline: Some(true),
      trim_trailing_whitespace: Some(true),
      use_tabs: Some(false),
    }
  }
}

impl GlobalConfig {
  pub fn merge(&mut self, other: GlobalConfig) {
    if other.languages.is_some() {
      self.languages = other.languages;
      self.ignore_languages = None;
    }
    if other.ignore_languages.is_some() {
      self.ignore_languages = other.ignore_languages;
      self.languages = None;
    }
    if other.indent_size.is_some() {
      self.indent_size = other.indent_size;
    }
    if other.line_length.is_some() {
      self.line_length = other.line_length;
    }
    if other.end_of_line.is_some() {
      self.end_of_line = other.end_of_line;
    }
    if other.charset.is_some() {
      self.charset = other.charset;
    }
    if other.insert_final_newline.is_some() {
      self.insert_final_newline = other.insert_final_newline;
    }
    if other.trim_trailing_whitespace.is_some() {
      self.trim_trailing_whitespace = other.trim_trailing_whitespace;
    }
    if other.use_tabs.is_some() {
      self.use_tabs = other.use_tabs;
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangConfig {
  pub format_tool: Option<String>,
  pub lint_tool: Option<String>,
  pub indent_size: Option<usize>,
  pub line_length: Option<usize>,
  pub use_tabs: Option<bool>,
  pub prose_wrap: Option<String>,
  pub enabled: Option<bool>,
  pub extra_args: Option<Vec<String>>,
  pub files: Option<Vec<String>>,
  pub exclude: Option<Vec<String>>,
}

impl LangConfig {
  pub fn merge(&mut self, other: LangConfig) {
    if other.format_tool.is_some() {
      self.format_tool = other.format_tool;
    }
    if other.lint_tool.is_some() {
      self.lint_tool = other.lint_tool;
    }
    if other.indent_size.is_some() {
      self.indent_size = other.indent_size;
    }
    if other.line_length.is_some() {
      self.line_length = other.line_length;
    }
    if other.use_tabs.is_some() {
      self.use_tabs = other.use_tabs;
    }
    if other.prose_wrap.is_some() {
      self.prose_wrap = other.prose_wrap;
    }
    if other.enabled.is_some() {
      self.enabled = other.enabled;
    }
    if other.extra_args.is_some() {
      self.extra_args = other.extra_args;
    }
    if other.files.is_some() {
      self.files = other.files;
    }
    if other.exclude.is_some() {
      self.exclude = other.exclude;
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormalityConfig {
  pub global: Option<GlobalConfig>,
  #[serde(default)]
  pub lang: BTreeMap<String, LangConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGlobalConfig {
  pub languages: Option<Vec<String>>,
  pub ignore_languages: Option<Vec<String>>,
  pub indent_size: usize,
  pub line_length: usize,
  pub end_of_line: String,
  pub charset: String,
  pub insert_final_newline: bool,
  pub trim_trailing_whitespace: bool,
  pub use_tabs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLangConfig {
  pub name: String,
  pub format_tool: Option<String>,
  pub lint_tool: Option<String>,
  pub indent_size: usize,
  pub line_length: usize,
  pub use_tabs: bool,
  pub prose_wrap: Option<String>,
  pub enabled: bool,
  pub extra_args: Vec<String>,
  pub files: Vec<String>,
  pub exclude: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
  Io {
    path: PathBuf,
    source: std::io::Error,
  },
  Parse {
    path: PathBuf,
    source: toml::de::Error,
  },
  Invalid(String),
}

impl std::fmt::Display for ConfigError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ConfigError::Io { path, source } => {
        write!(
          f,
          "Failed to read config file at {}: {}",
          path.display(),
          source
        )
      }
      ConfigError::Parse { path, source } => {
        write!(
          f,
          "Failed to parse config file at {}: {}",
          path.display(),
          source
        )
      }
      ConfigError::Invalid(msg) => write!(f, "Invalid config: {}", msg),
    }
  }
}

impl std::error::Error for ConfigError {}

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
    let config: Self =
      toml::from_str(content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
      })?;
    config.validate(path)?;
    Ok(config)
  }

  pub fn validate(&self, path: &Path) -> Result<(), ConfigError> {
    if let Some(ref global) = self.global {
      if global.languages.is_some() && global.ignore_languages.is_some() {
        return Err(ConfigError::Invalid(format!(
          "Cannot specify both 'languages' (allowlist) and 'ignore_languages' (blocklist) in [global] at {}.\n\
           • Use 'languages = [...]' to exclusively allow specific surfaces.\n\
           • Use 'ignore_languages = [...]' to exclude specific surfaces from auto-detection.",
          path.display()
        )));
      }
    }
    Ok(())
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

    ResolvedGlobalConfig {
      languages: current.and_then(|g| g.languages.clone()),
      ignore_languages: current.and_then(|g| g.ignore_languages.clone()),
      indent_size: current
        .and_then(|g| g.indent_size)
        .unwrap_or(base.indent_size.unwrap_or(2)),
      line_length: current
        .and_then(|g| g.line_length)
        .unwrap_or(base.line_length.unwrap_or(80)),
      end_of_line: current.and_then(|g| g.end_of_line.clone()).unwrap_or_else(
        || base.end_of_line.unwrap_or_else(|| "lf".to_string()),
      ),
      charset: current
        .and_then(|g| g.charset.clone())
        .unwrap_or_else(|| base.charset.unwrap_or_else(|| "utf-8".to_string())),
      insert_final_newline: current
        .and_then(|g| g.insert_final_newline)
        .unwrap_or(base.insert_final_newline.unwrap_or(true)),
      trim_trailing_whitespace: current
        .and_then(|g| g.trim_trailing_whitespace)
        .unwrap_or(base.trim_trailing_whitespace.unwrap_or(true)),
      use_tabs: current
        .and_then(|g| g.use_tabs)
        .unwrap_or(base.use_tabs.unwrap_or(false)),
    }
  }

  pub fn resolve_for_lang(&self, lang_name: &str) -> ResolvedLangConfig {
    let global = self.resolve_global();
    let lang_cfg = self.lang.get(lang_name);

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

    ResolvedLangConfig {
      name: lang_name.to_string(),
      format_tool: lang_cfg
        .and_then(|l| l.format_tool.clone())
        .or_else(|| default_fmt.map(|s| s.to_string())),
      lint_tool: lang_cfg
        .and_then(|l| l.lint_tool.clone())
        .or_else(|| default_lint.map(|s| s.to_string())),
      indent_size: lang_cfg
        .and_then(|l| l.indent_size)
        .unwrap_or(global.indent_size),
      line_length: lang_cfg
        .and_then(|l| l.line_length)
        .unwrap_or(global.line_length),
      use_tabs: lang_cfg.and_then(|l| l.use_tabs).unwrap_or(global.use_tabs),
      prose_wrap: lang_cfg.and_then(|l| l.prose_wrap.clone()),
      enabled: lang_cfg.and_then(|l| l.enabled).unwrap_or(true),
      extra_args: lang_cfg
        .and_then(|l| l.extra_args.clone())
        .unwrap_or_default(),
      files: lang_cfg.and_then(|l| l.files.clone()).unwrap_or_default(),
      exclude: lang_cfg.and_then(|l| l.exclude.clone()).unwrap_or_default(),
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

  /// Generates standard template for `fml init` versioned to current package release
  pub fn generate_init_template(detected_langs: &[&str]) -> String {
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

    if !detected_langs.is_empty() {
      let formatted_langs: Vec<String> = detected_langs
        .iter()
        .map(|l| format!("\"{}\"", l))
        .collect();
      out.push_str(&format!("languages = [{}]\n", formatted_langs.join(", ")));
    }

    out.push_str("indent_size = 2\n");
    out.push_str("line_length = 80\n");
    out.push_str("end_of_line = \"lf\"\n");
    out.push_str("charset = \"utf-8\"\n");
    out.push_str("insert_final_newline = true\n");
    out.push_str("trim_trailing_whitespace = true\n");

    out
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_default_resolution() {
    let cfg = FormalityConfig::with_defaults();
    let global = cfg.resolve_global();
    assert_eq!(global.indent_size, 2);
    assert_eq!(global.line_length, 80);
    assert_eq!(global.end_of_line, "lf");

    let rust = cfg.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 2);
    assert_eq!(rust.line_length, 80);
    assert_eq!(rust.format_tool.as_deref(), Some("cargo-fmt"));
    assert_eq!(rust.lint_tool.as_deref(), Some("clippy"));
    assert!(rust.enabled);

    let json = cfg.resolve_for_lang("json");
    assert_eq!(json.format_tool.as_deref(), Some("prettier"));
  }

  #[test]
  fn test_find_project_config_candidates() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    // No config initially
    assert_eq!(find_project_config(root), None);

    // Test .formality.toml
    let hidden = root.join(".formality.toml");
    fs::write(&hidden, "[global]\nindent_size = 4\n").unwrap();
    assert_eq!(find_project_config(root), Some(hidden.clone()));

    // Test formality.toml (higher precedence than .formality.toml)
    let standard = root.join("formality.toml");
    fs::write(&standard, "[global]\nindent_size = 2\n").unwrap();
    assert_eq!(find_project_config(root), Some(standard));
  }

  #[test]
  fn test_languages_allowlist_parsing() {
    let toml = r#"
      [global]
      languages = ["rust", "toml"]
      indent_size = 4
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(
      global.languages,
      Some(vec!["rust".to_string(), "toml".to_string()])
    );
    assert_eq!(global.ignore_languages, None);
    assert_eq!(global.indent_size, 4);
  }

  #[test]
  fn test_ignore_languages_blocklist_parsing() {
    let toml = r#"
      [global]
      ignore_languages = ["cpp", "python"]
      indent_size = 4
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(global.languages, None);
    assert_eq!(
      global.ignore_languages,
      Some(vec!["cpp".to_string(), "python".to_string()])
    );
    assert_eq!(global.indent_size, 4);
  }

  #[test]
  fn test_languages_and_ignore_languages_are_mutually_exclusive() {
    let toml = r#"
      [global]
      languages = ["rust", "toml"]
      ignore_languages = ["cpp"]
    "#;
    let result = FormalityConfig::parse_str(toml, Path::new("test.toml"));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Cannot specify both 'languages' (allowlist) and 'ignore_languages' (blocklist)"));
  }

  #[test]
  fn test_merge_and_override() {
    let mut base = FormalityConfig::with_defaults();

    let override_toml = r#"
            [global]
            indent_size = 4
            line_length = 100

            [lang.markdown]
            indent_size = 2
            prose_wrap = "always"
        "#;

    let parsed =
      FormalityConfig::parse_str(override_toml, Path::new("test.toml"))
        .unwrap();
    base.merge(parsed);

    let global = base.resolve_global();
    assert_eq!(global.indent_size, 4);
    assert_eq!(global.line_length, 100);

    let rust = base.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 4);
    assert_eq!(rust.line_length, 100);

    let md = base.resolve_for_lang("markdown");
    assert_eq!(md.indent_size, 2);
    assert_eq!(md.line_length, 100);
    assert_eq!(md.prose_wrap.as_deref(), Some("always"));
  }
}
