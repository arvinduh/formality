use super::facets::LayoutFacet;
use super::lang_table::{
  build_resolved_lang_config, default_tool_opt, impl_default_tools_fn,
  lang_options_table,
};
use super::options::MarkdownOptions;
use super::{
  CONFIG_FILE_CANDIDATES, ConfigError, FormalityConfig, GlobalConfig,
  ResolvedGlobalConfig, ResolvedLangConfig,
};
use crate::surfaces::SurfaceRegistry;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

impl FormalityConfig {
  /// Constructs an empty [`FormalityConfig`] with no global or language overrides.
  #[must_use]
  pub fn empty() -> Self {
    Self {
      global: None,
      lang: BTreeMap::new(),
    }
  }

  /// Constructs a [`FormalityConfig`] initialized with standard global default settings.
  #[must_use]
  pub fn with_defaults() -> Self {
    Self {
      global: Some(GlobalConfig::default()),
      lang: BTreeMap::new(),
    }
  }

  /// Parses configuration from a TOML string.
  ///
  /// # Errors
  ///
  /// Returns a [`ConfigError::Parse`] if the TOML is invalid or does not match the schema.
  pub fn parse_str(content: &str, path: &Path) -> Result<Self, ConfigError> {
    toml::from_str(content).map_err(|source| ConfigError::Parse {
      path: path.to_path_buf(),
      source,
    })
  }

  /// Loads and parses configuration from a file path.
  ///
  /// # Errors
  ///
  /// Returns a [`ConfigError::Io`] if the file cannot be read, or [`ConfigError::Parse`] if the TOML is invalid.
  pub fn load_file(path: &Path) -> Result<Self, ConfigError> {
    let content =
      fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
      })?;
    Self::parse_str(&content, path)
  }

  /// Merges `other` configuration settings into `self`.
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

  /// Resolves the global configuration settings.
  #[must_use]
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

  /// Resolves effective configuration settings for a specific named language surface.
  #[must_use]
  pub fn resolve_for_lang(&self, lang_name: &str) -> ResolvedLangConfig {
    let global = self.resolve_global();
    let lang_cfg = self.lang.get(lang_name);

    let (default_fmt, default_lint) = default_tools_for_lang(lang_name);
    let (layout, indent_size, line_length, use_tabs, prose_wrap) =
      resolve_layout_for_lang(lang_name, lang_cfg, &global);

    let markdown = lang_cfg
      .and_then(super::LangConfig::markdown_options)
      .or_else(|| {
        if lang_name == "markdown" {
          Some(MarkdownOptions {
            prose_wrap: prose_wrap.clone(),
          })
        } else {
          None
        }
      });

    let extra = lang_cfg.map(|l| l.extra.clone()).unwrap_or_default();

    lang_options_table!(
      build_resolved_lang_config,
      lang_cfg,
      lang_name,
      lang_name.to_string(),
      lang_cfg
        .and_then(|l| l.format_tool.clone())
        .or_else(|| default_fmt.map(std::string::ToString::to_string)),
      lang_cfg
        .and_then(|l| l.lint_tool.clone())
        .or_else(|| default_lint.map(std::string::ToString::to_string)),
      indent_size,
      line_length,
      use_tabs,
      prose_wrap,
      layout,
      lang_cfg.and_then(|l| l.enabled).unwrap_or(true),
      lang_cfg
        .and_then(|l| l.extra_args.clone())
        .unwrap_or_default(),
      lang_cfg.and_then(|l| l.files.clone()).unwrap_or_default(),
      {
        let mut ex = global.exclude.clone();
        if let Some(lang_ex) = lang_cfg.and_then(|l| l.exclude.clone()) {
          ex.extend(lang_ex);
        }
        ex
      },
      markdown,
      extra
    )
  }

  /// Returns the raw `[lang.X]` section names from this config whose `X`
  /// does not match any known surface's canonical name or alias, as
  /// registered in `registry`.
  ///
  /// This intentionally does *not* flag section names that are valid
  /// surface names/aliases but simply aren't detected/active in the
  /// current workspace (e.g. `[lang.rust]` in a Python-only repo) — that
  /// is a legitimate pre-configuration for a language the user expects to
  /// add later, not a mistake. It only flags names that don't resolve to
  /// *any* registered surface at all, which is almost always a typo (e.g.
  /// `[lang.pythonn]`).
  #[must_use]
  pub fn unrecognized_lang_sections(
    &self,
    registry: &SurfaceRegistry,
  ) -> Vec<&str> {
    self
      .lang
      .keys()
      .filter(|name| registry.resolve_canonical_name(name).is_none())
      .map(std::string::String::as_str)
      .collect()
  }

  /// Loads configuration with layered resolution:
  /// Embedded defaults -> User config (`~/.config/formality/config.toml`) -> Project config (`formality.toml` / `.formality.toml`)
  ///
  /// # Errors
  ///
  /// Returns a [`ConfigError`] if reading or parsing any discovered configuration file fails.
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
  #[must_use]
  pub fn generate_sample() -> String {
    let mut out = String::new();
    out.push_str("# formality configuration file\n");
    out.push_str("# https://github.com/arvinduh/formality\n");
    // Reference the schema from the versioned GitHub Release asset under the schema tag
    // (s{major}.{minor}, e.g. s1.0) — never from a raw git branch URL — so users are
    // always pinned to a specific schema release rather than an ever-changing main branch.
    out.push_str(&format!(
      "#:schema https://github.com/arvinduh/formality/releases/download/s{}/formality.schema.json\n\n",
      crate::config::schema::SCHEMA_VERSION
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

  /// Generates standard template for `fml init` versioned to current package
  /// release, customized with commented-out `[lang.<name>]` stub sections for
  /// each surface detected in the workspace.
  ///
  /// The stubs stay commented out deliberately: `fml` auto-detects and
  /// formats every surface with sane defaults out of the box (see #68), so
  /// `generate_sample()`'s `[global]` block alone is a fully working config.
  /// The per-language stubs exist purely as discoverable, ready-to-uncomment
  /// starting points that show users *which* languages were found and *what*
  /// they can override, without silently constraining or hardcoding
  /// anything the way an active `languages = [...]` list would.
  #[must_use]
  pub fn generate_init_template(detected_langs: &[&str]) -> String {
    let mut out = Self::generate_sample();

    if detected_langs.is_empty() {
      return out;
    }

    // Deduplicate and sort for deterministic, readable output regardless of
    // detection order.
    let mut langs: Vec<&str> = detected_langs.to_vec();
    langs.sort_unstable();
    langs.dedup();

    out.push_str(
      "\n# Detected language surfaces below. Uncomment a section to override\n\
       # its defaults (indent_size, line_length, format_tool, lint_tool, ...).\n\
       # Leave commented out to keep using formality's built-in defaults.\n",
    );
    for lang in langs {
      let _ = write!(out, "\n# [lang.{lang}]\n");
      out.push_str("# indent_size = 2\n");
      out.push_str("# line_length = 80\n");
    }

    out
  }
}
/// Searches parent directories starting from `start_dir` for project config files (`formality.toml` / `.formality.toml`).
#[must_use]
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
#[must_use]
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

lang_options_table!(impl_default_tools_fn);

fn resolve_layout_for_lang(
  lang_name: &str,
  lang_cfg: Option<&super::LangConfig>,
  global: &ResolvedGlobalConfig,
) -> (LayoutFacet, usize, usize, bool, Option<String>) {
  let lang_layout = lang_cfg.and_then(|l| l.layout.as_ref());

  // Java's indent width is dictated by the configured google-java-format
  // style (Google = 2, AOSP = 4) when the user hasn't explicitly pinned
  // `indent_size` themselves. Resolving it here — the single source of
  // truth `ResolvedLangConfig::indent_size` — is what keeps the generated
  // `checkstyle.xml` and `.editorconfig` in agreement; see
  // `JavaSurface::facet_support` (Configurable, not Fixed) and
  // `CheckstyleConfig::from_context`, both of which just read this value.
  let java_style_is_aosp = lang_name == "java"
    && lang_cfg
      .and_then(super::LangConfig::java_options)
      .and_then(|j| j.style)
      .as_deref()
      == Some("aosp");

  let indent_size = lang_cfg
    .and_then(|l| l.indent_size)
    .or_else(|| lang_layout.and_then(|l| l.indent_size))
    .unwrap_or(if java_style_is_aosp {
      4
    } else {
      global.indent_size
    });

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

  (layout, indent_size, line_length, use_tabs, prose_wrap)
}
