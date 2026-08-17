use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_FILE_NAME: &str = "formality.toml";
pub const CONFIG_FILE_CANDIDATES: &[&str] =
  &["formality.toml", ".formality.toml"];

/// Common layout facets configuring formatting layout across tools.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct LayoutFacet {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
}

impl LayoutFacet {
  pub fn merge(&mut self, other: LayoutFacet) {
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
  }

  pub fn is_empty(&self) -> bool {
    self.indent_size.is_none()
      && self.line_length.is_none()
      && self.use_tabs.is_none()
      && self.prose_wrap.is_none()
  }
}

/// Typed formatting and linting options for Rust.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct RustOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub edition: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub version: Option<String>,
}

impl RustOptions {
  pub fn merge(&mut self, other: RustOptions) {
    if other.edition.is_some() {
      self.edition = other.edition;
    }
    if other.version.is_some() {
      self.version = other.version;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.edition.is_none() && self.version.is_none()
  }
}

/// Typed formatting and linting options for Python.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct PythonOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub quote_style: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target_version: Option<String>,
}

impl PythonOptions {
  pub fn merge(&mut self, other: PythonOptions) {
    if other.quote_style.is_some() {
      self.quote_style = other.quote_style;
    }
    if other.target_version.is_some() {
      self.target_version = other.target_version;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.quote_style.is_none() && self.target_version.is_none()
  }
}

/// Typed formatting and linting options for C/C++.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct CppOptions {
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "Standard",
    alias = "std"
  )]
  pub standard: Option<String>,
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "ColumnLimit",
    alias = "column-limit"
  )]
  pub column_limit: Option<usize>,
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "BasedOnStyle",
    alias = "based-on-style"
  )]
  pub based_on_style: Option<String>,
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "PointerAlignment",
    alias = "pointer-alignment"
  )]
  pub pointer_alignment: Option<String>,
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "BreakBeforeBraces",
    alias = "break-before-braces"
  )]
  pub break_before_braces: Option<String>,
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "SortIncludes",
    alias = "sort-includes"
  )]
  pub sort_includes: Option<bool>,
}

impl CppOptions {
  pub fn merge(&mut self, other: CppOptions) {
    if other.standard.is_some() {
      self.standard = other.standard;
    }
    if other.column_limit.is_some() {
      self.column_limit = other.column_limit;
    }
    if other.based_on_style.is_some() {
      self.based_on_style = other.based_on_style;
    }
    if other.pointer_alignment.is_some() {
      self.pointer_alignment = other.pointer_alignment;
    }
    if other.break_before_braces.is_some() {
      self.break_before_braces = other.break_before_braces;
    }
    if other.sort_includes.is_some() {
      self.sort_includes = other.sort_includes;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.standard.is_none()
      && self.column_limit.is_none()
      && self.based_on_style.is_none()
      && self.pointer_alignment.is_none()
      && self.break_before_braces.is_none()
      && self.sort_includes.is_none()
  }
}

/// Typed formatting and linting options for Markdown.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct MarkdownOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
}

impl MarkdownOptions {
  pub fn merge(&mut self, other: MarkdownOptions) {
    if other.prose_wrap.is_some() {
      self.prose_wrap = other.prose_wrap;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.prose_wrap.is_none()
  }
}

/// Typed formatting and linting options for YAML.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct YamlOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_sequence: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub document_start: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub truthy: Option<bool>,
}

impl YamlOptions {
  pub fn merge(&mut self, other: YamlOptions) {
    if other.indent_sequence.is_some() {
      self.indent_sequence = other.indent_sequence;
    }
    if other.document_start.is_some() {
      self.document_start = other.document_start;
    }
    if other.truthy.is_some() {
      self.truthy = other.truthy;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.indent_sequence.is_none()
      && self.document_start.is_none()
      && self.truthy.is_none()
  }
}

/// Typed formatting and linting options for JSON.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct JsonOptions {}

impl JsonOptions {
  pub fn merge(&mut self, _other: JsonOptions) {}

  pub fn is_empty(&self) -> bool {
    true
  }
}

/// Typed formatting and linting options for TOML.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct TomlOptions {}

impl TomlOptions {
  pub fn merge(&mut self, _other: TomlOptions) {}

  pub fn is_empty(&self) -> bool {
    true
  }
}

/// Typed formatting and linting options for Typst.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct TypstOptions {}

impl TypstOptions {
  pub fn merge(&mut self, _other: TypstOptions) {}

  pub fn is_empty(&self) -> bool {
    true
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlobalConfig {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub languages: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ignore_languages: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub end_of_line: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub charset: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub insert_final_newline: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trim_trailing_whitespace: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub layout: Option<LayoutFacet>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub exclude: Vec<PathBuf>,
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
      layout: None,
      exclude: Vec::new(),
    }
  }
}

impl GlobalConfig {
  pub fn merge(&mut self, other: GlobalConfig) {
    if other.languages.is_some() {
      self.languages = other.languages;
    }
    if other.ignore_languages.is_some() {
      self.ignore_languages = other.ignore_languages;
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
    if let Some(other_layout) = other.layout {
      if let Some(ref mut our_layout) = self.layout {
        our_layout.merge(other_layout);
      } else {
        self.layout = Some(other_layout);
      }
    }
    if !other.exclude.is_empty() {
      self.exclude = other.exclude;
    }
  }
}

fn extract_options<T>(
  initial: Option<T>,
  options: &Option<toml::Value>,
  extra: &BTreeMap<String, toml::Value>,
  merge_fn: impl Fn(&mut T, T),
  is_empty_fn: impl Fn(&T) -> bool,
) -> Option<T>
where
  T: for<'de> Deserialize<'de> + Clone,
{
  let mut opts = initial;
  if let Some(o) = options
    && let Ok(deserialized) = o.clone().try_into::<T>()
  {
    if let Some(ref mut cur) = opts {
      merge_fn(cur, deserialized);
    } else if !is_empty_fn(&deserialized) {
      opts = Some(deserialized);
    }
  }
  if !extra.is_empty()
    && let Ok(val) = toml::Value::try_from(extra.clone())
    && let Ok(deserialized) = val.try_into::<T>()
  {
    if let Some(ref mut cur) = opts {
      merge_fn(cur, deserialized);
    } else if !is_empty_fn(&deserialized) {
      opts = Some(deserialized);
    }
  }
  opts
}
#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub struct LangConfig {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub format_tool: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub lint_tool: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub enabled: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub extra_args: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub files: Option<Vec<PathBuf>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub exclude: Option<Vec<PathBuf>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub layout: Option<LayoutFacet>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rust: Option<RustOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub python: Option<PythonOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cpp: Option<CppOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub markdown: Option<MarkdownOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub yaml: Option<YamlOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub json: Option<JsonOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub toml: Option<TomlOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub typst: Option<TypstOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(skip)]
  pub options: Option<toml::Value>,
  #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
  #[schemars(skip)]
  pub extra: BTreeMap<String, toml::Value>,
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
    if let Some(other_layout) = other.layout {
      if let Some(ref mut our_layout) = self.layout {
        our_layout.merge(other_layout);
      } else {
        self.layout = Some(other_layout);
      }
    }
    if let Some(other_rust) = other.rust {
      if let Some(ref mut our_rust) = self.rust {
        our_rust.merge(other_rust);
      } else {
        self.rust = Some(other_rust);
      }
    }
    if let Some(other_py) = other.python {
      if let Some(ref mut our_py) = self.python {
        our_py.merge(other_py);
      } else {
        self.python = Some(other_py);
      }
    }
    if let Some(other_cpp) = other.cpp {
      if let Some(ref mut our_cpp) = self.cpp {
        our_cpp.merge(other_cpp);
      } else {
        self.cpp = Some(other_cpp);
      }
    }
    if let Some(other_md) = other.markdown {
      if let Some(ref mut our_md) = self.markdown {
        our_md.merge(other_md);
      } else {
        self.markdown = Some(other_md);
      }
    }
    if let Some(other_yaml) = other.yaml {
      if let Some(ref mut our_yaml) = self.yaml {
        our_yaml.merge(other_yaml);
      } else {
        self.yaml = Some(other_yaml);
      }
    }
    if let Some(other_json) = other.json {
      if let Some(ref mut our_json) = self.json {
        our_json.merge(other_json);
      } else {
        self.json = Some(other_json);
      }
    }
    if let Some(other_toml) = other.toml {
      if let Some(ref mut our_toml) = self.toml {
        our_toml.merge(other_toml);
      } else {
        self.toml = Some(other_toml);
      }
    }
    if let Some(other_typst) = other.typst {
      if let Some(ref mut our_typst) = self.typst {
        our_typst.merge(other_typst);
      } else {
        self.typst = Some(other_typst);
      }
    }
    if other.options.is_some() {
      self.options = other.options;
    }
    for (k, v) in other.extra {
      self.extra.insert(k, v);
    }
  }

  pub fn rust_options(&self) -> Option<RustOptions> {
    extract_options(
      self.rust.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |r| r.is_empty(),
    )
  }

  pub fn python_options(&self) -> Option<PythonOptions> {
    extract_options(
      self.python.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |p| p.is_empty(),
    )
  }

  pub fn cpp_options(&self) -> Option<CppOptions> {
    extract_options(
      self.cpp.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |c| c.is_empty(),
    )
  }

  pub fn markdown_options(&self) -> Option<MarkdownOptions> {
    let mut opts = extract_options(
      self.markdown.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |m| m.is_empty(),
    );
    if opts.is_none() {
      if let Some(ref pw) = self.prose_wrap {
        opts = Some(MarkdownOptions {
          prose_wrap: Some(pw.clone()),
        });
      } else if let Some(ref l) = self.layout
        && let Some(ref pw) = l.prose_wrap
      {
        opts = Some(MarkdownOptions {
          prose_wrap: Some(pw.clone()),
        });
      }
    }
    opts
  }

  pub fn yaml_options(&self) -> Option<YamlOptions> {
    extract_options(
      self.yaml.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |y| y.is_empty(),
    )
  }

  pub fn json_options(&self) -> Option<JsonOptions> {
    extract_options(
      self.json.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |_| false,
    )
  }

  pub fn toml_options(&self) -> Option<TomlOptions> {
    extract_options(
      self.toml.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |_| false,
    )
  }

  pub fn typst_options(&self) -> Option<TypstOptions> {
    extract_options(
      self.typst.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |_| false,
    )
  }
}

#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub struct FormalityConfig {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub global: Option<GlobalConfig>,
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
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
  pub layout: LayoutFacet,
  pub exclude: Vec<PathBuf>,
}

impl Default for ResolvedGlobalConfig {
  fn default() -> Self {
    FormalityConfig::with_defaults().resolve_global()
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLangConfig {
  pub name: String,
  pub format_tool: Option<String>,
  pub lint_tool: Option<String>,
  pub indent_size: usize,
  pub line_length: usize,
  pub use_tabs: bool,
  pub prose_wrap: Option<String>,
  pub layout: LayoutFacet,
  pub enabled: bool,
  pub extra_args: Vec<String>,
  pub files: Vec<PathBuf>,
  pub exclude: Vec<PathBuf>,
  pub rust: Option<RustOptions>,
  pub python: Option<PythonOptions>,
  pub cpp: Option<CppOptions>,
  pub markdown: Option<MarkdownOptions>,
  pub yaml: Option<YamlOptions>,
  pub json: Option<JsonOptions>,
  pub toml: Option<TomlOptions>,
  pub typst: Option<TypstOptions>,
  pub extra: BTreeMap<String, toml::Value>,
}

impl ResolvedLangConfig {
  pub fn new(name: &str) -> Self {
    FormalityConfig::with_defaults().resolve_for_lang(name)
  }
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
    assert_eq!(global.layout.indent_size, Some(2));
    assert_eq!(global.layout.line_length, Some(80));
    assert_eq!(global.layout.use_tabs, Some(false));

    let rust = cfg.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 2);
    assert_eq!(rust.line_length, 80);
    assert_eq!(rust.format_tool.as_deref(), Some("cargo-fmt"));
    assert_eq!(rust.lint_tool.as_deref(), Some("clippy"));
    assert!(rust.enabled);
    assert_eq!(rust.layout.indent_size, Some(2));
    assert_eq!(rust.layout.line_length, Some(80));
    assert_eq!(rust.rust, Some(RustOptions::default()));

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
  fn test_languages_list_parsing() {
    let toml = r#"
      [global]
      languages = ["rust", "toml"]
      ignore_languages = ["cpp"]
      indent_size = 4
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(
      global.languages,
      Some(vec!["rust".to_string(), "toml".to_string()])
    );
    assert_eq!(global.ignore_languages, Some(vec!["cpp".to_string()]));
    assert_eq!(global.indent_size, 4);
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

  #[test]
  fn test_lang_config_extra_args_files_and_exclude() {
    let toml = r#"
      [global]
      indent_size = 2

      [lang.rust]
      extra_args = ["--verbose", "--", "-D", "clippy::all"]
      files = ["src/lib.rs", "src/main.rs"]
      exclude = ["tests/fixtures", "src/generated/**"]
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.extra_args,
      vec!["--verbose", "--", "-D", "clippy::all"]
    );
    assert_eq!(
      rust.files,
      vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")]
    );
    assert_eq!(
      rust.exclude,
      vec![
        PathBuf::from("tests/fixtures"),
        PathBuf::from("src/generated/**")
      ]
    );
  }

  #[test]
  fn test_layout_facet_direct_and_inheritance() {
    let toml = r#"
      [global]
      indent_size = 2
      line_length = 80

      [global.layout]
      use_tabs = true
      prose_wrap = "preserve"

      [lang.rust.layout]
      indent_size = 4
      line_length = 100

      [lang.markdown]
      prose_wrap = "always"
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(global.indent_size, 2);
    assert_eq!(global.line_length, 80);
    assert!(global.use_tabs);
    assert_eq!(global.layout.prose_wrap.as_deref(), Some("preserve"));

    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 4);
    assert_eq!(rust.line_length, 100);
    assert!(rust.use_tabs);
    assert_eq!(rust.prose_wrap.as_deref(), Some("preserve"));
    assert_eq!(rust.layout.indent_size, Some(4));
    assert_eq!(rust.layout.line_length, Some(100));

    let md = parsed.resolve_for_lang("markdown");
    assert_eq!(md.indent_size, 2);
    assert_eq!(md.line_length, 80);
    assert!(md.use_tabs);
    assert_eq!(md.prose_wrap.as_deref(), Some("always"));
    assert_eq!(
      md.markdown,
      Some(MarkdownOptions {
        prose_wrap: Some("always".to_string())
      })
    );
  }

  #[test]
  fn test_typed_options_deserialization_from_toml() {
    let toml = r#"
      [lang.rust]
      edition = "2021"
      version = "1.75"

      [lang.python]
      quote_style = "single"
      target_version = "py311"

      [lang.cpp]
      standard = "c++20"
      column_limit = 100
      based_on_style = "Google"
      pointer_alignment = "Left"
      break_before_braces = "Attach"
      sort_includes = true

      [lang.markdown]
      prose_wrap = "never"

      [lang.yaml]
      indent_sequence = true

      [lang.json]

      [lang.toml]

      [lang.typst]
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.rust,
      Some(RustOptions {
        edition: Some("2021".to_string()),
        version: Some("1.75".to_string()),
      })
    );

    let python = parsed.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(PythonOptions {
        quote_style: Some("single".to_string()),
        target_version: Some("py311".to_string()),
      })
    );

    let cpp = parsed.resolve_for_lang("cpp");
    assert_eq!(
      cpp.cpp,
      Some(CppOptions {
        standard: Some("c++20".to_string()),
        column_limit: Some(100),
        based_on_style: Some("Google".to_string()),
        pointer_alignment: Some("Left".to_string()),
        break_before_braces: Some("Attach".to_string()),
        sort_includes: Some(true),
      })
    );

    let md = parsed.resolve_for_lang("markdown");
    assert_eq!(
      md.markdown,
      Some(MarkdownOptions {
        prose_wrap: Some("never".to_string()),
      })
    );

    let yaml = parsed.resolve_for_lang("yaml");
    assert_eq!(
      yaml.yaml,
      Some(YamlOptions {
        indent_sequence: Some(true),
        document_start: None,
        truthy: None,
      })
    );

    let json = parsed.resolve_for_lang("json");
    assert_eq!(json.json, Some(JsonOptions {}));

    let toml_lang = parsed.resolve_for_lang("toml");
    assert_eq!(toml_lang.toml, Some(TomlOptions {}));

    let typst = parsed.resolve_for_lang("typst");
    assert_eq!(typst.typst, Some(TypstOptions {}));
  }

  #[test]
  fn test_typed_options_subtable_deserialization() {
    let toml = r#"
      [lang.rust.rust]
      edition = "2024"
      version = "1.85"

      [lang.python.python]
      quote_style = "double"
      target_version = "py312"

      [lang.cpp.cpp]
      standard = "c++23"
      column_limit = 120
      based_on_style = "Chromium"
      pointer_alignment = "Right"
      break_before_braces = "Allman"
      sort_includes = false

      [lang.yaml.yaml]
      indent_sequence = false
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.rust,
      Some(RustOptions {
        edition: Some("2024".to_string()),
        version: Some("1.85".to_string()),
      })
    );

    let python = parsed.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(PythonOptions {
        quote_style: Some("double".to_string()),
        target_version: Some("py312".to_string()),
      })
    );

    let cpp = parsed.resolve_for_lang("cpp");
    assert_eq!(
      cpp.cpp,
      Some(CppOptions {
        standard: Some("c++23".to_string()),
        column_limit: Some(120),
        based_on_style: Some("Chromium".to_string()),
        pointer_alignment: Some("Right".to_string()),
        break_before_braces: Some("Allman".to_string()),
        sort_includes: Some(false),
      })
    );

    let yaml = parsed.resolve_for_lang("yaml");
    assert_eq!(
      yaml.yaml,
      Some(YamlOptions {
        indent_sequence: Some(false),
        document_start: None,
        truthy: None,
      })
    );
  }

  #[test]
  fn test_typed_options_merging_semantics() {
    let mut base = FormalityConfig::empty();
    let base_toml = r#"
      [global]
      indent_size = 2
      line_length = 80

      [lang.rust]
      edition = "2021"
      indent_size = 4

      [lang.python]
      quote_style = "single"
    "#;
    base.merge(
      FormalityConfig::parse_str(base_toml, Path::new("base.toml")).unwrap(),
    );

    let override_toml = r#"
      [lang.rust]
      version = "1.78"
      line_length = 100

      [lang.python]
      target_version = "py312"
    "#;
    base.merge(
      FormalityConfig::parse_str(override_toml, Path::new("override.toml"))
        .unwrap(),
    );

    let rust = base.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 4);
    assert_eq!(rust.line_length, 100);
    assert_eq!(
      rust.rust,
      Some(RustOptions {
        edition: Some("2021".to_string()),
        version: Some("1.78".to_string()),
      })
    );

    let python = base.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(PythonOptions {
        quote_style: Some("single".to_string()),
        target_version: Some("py312".to_string()),
      })
    );
  }

  #[test]
  fn test_serialization_deserialization_roundtrip() {
    let mut config = FormalityConfig::empty();
    config.global = Some(GlobalConfig {
      languages: Some(vec!["rust".to_string(), "python".to_string()]),
      ignore_languages: None,
      indent_size: Some(2),
      line_length: Some(100),
      end_of_line: Some("lf".to_string()),
      charset: Some("utf-8".to_string()),
      insert_final_newline: Some(true),
      trim_trailing_whitespace: Some(true),
      use_tabs: Some(false),
      layout: Some(LayoutFacet {
        indent_size: Some(2),
        line_length: Some(100),
        use_tabs: Some(false),
        prose_wrap: Some("always".to_string()),
      }),
      exclude: Vec::new(),
    });

    let rust_cfg = LangConfig {
      indent_size: Some(4),
      rust: Some(RustOptions {
        edition: Some("2024".to_string()),
        version: Some("1.85".to_string()),
      }),
      ..Default::default()
    };
    config.lang.insert("rust".to_string(), rust_cfg);

    let py_cfg = LangConfig {
      python: Some(PythonOptions {
        quote_style: Some("double".to_string()),
        target_version: Some("py311".to_string()),
      }),
      ..Default::default()
    };
    config.lang.insert("python".to_string(), py_cfg);

    let serialized = toml::to_string(&config).unwrap();
    let deserialized: FormalityConfig =
      FormalityConfig::parse_str(&serialized, Path::new("test.toml")).unwrap();

    assert_eq!(config, deserialized);
  }

  #[test]
  fn test_language_options_merge_units() {
    let mut rust1 = RustOptions {
      edition: Some("2021".to_string()),
      version: None,
    };
    let rust2 = RustOptions {
      edition: None,
      version: Some("1.75".to_string()),
    };
    rust1.merge(rust2);
    assert_eq!(rust1.edition.as_deref(), Some("2021"));
    assert_eq!(rust1.version.as_deref(), Some("1.75"));

    let mut py1 = PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: None,
    };
    let py2 = PythonOptions {
      quote_style: None,
      target_version: Some("py312".to_string()),
    };
    py1.merge(py2);
    assert_eq!(py1.quote_style.as_deref(), Some("single"));
    assert_eq!(py1.target_version.as_deref(), Some("py312"));

    let mut cpp1 = CppOptions {
      standard: Some("c++17".to_string()),
      column_limit: None,
      based_on_style: Some("LLVM".to_string()),
      pointer_alignment: None,
      break_before_braces: None,
      sort_includes: Some(true),
    };
    let cpp2 = CppOptions {
      standard: None,
      column_limit: Some(100),
      based_on_style: None,
      pointer_alignment: Some("Right".to_string()),
      break_before_braces: Some("Allman".to_string()),
      sort_includes: Some(false),
    };
    cpp1.merge(cpp2);
    assert_eq!(cpp1.standard.as_deref(), Some("c++17"));
    assert_eq!(cpp1.column_limit, Some(100));
    assert_eq!(cpp1.based_on_style.as_deref(), Some("LLVM"));
    assert_eq!(cpp1.pointer_alignment.as_deref(), Some("Right"));
    assert_eq!(cpp1.break_before_braces.as_deref(), Some("Allman"));
    assert_eq!(cpp1.sort_includes, Some(false));

    let mut yaml1 = YamlOptions {
      indent_sequence: Some(true),
      document_start: Some(true),
      truthy: None,
    };
    let yaml2 = YamlOptions {
      indent_sequence: Some(false),
      document_start: None,
      truthy: Some(false),
    };
    yaml1.merge(yaml2);
    assert_eq!(yaml1.indent_sequence, Some(false));
    assert_eq!(yaml1.document_start, Some(true));
    assert_eq!(yaml1.truthy, Some(false));

    let mut layout1 = LayoutFacet {
      indent_size: Some(2),
      line_length: None,
      use_tabs: None,
      prose_wrap: None,
    };
    let layout2 = LayoutFacet {
      indent_size: None,
      line_length: Some(100),
      use_tabs: Some(true),
      prose_wrap: Some("preserve".to_string()),
    };
    layout1.merge(layout2);
    assert_eq!(layout1.indent_size, Some(2));
    assert_eq!(layout1.line_length, Some(100));
    assert_eq!(layout1.use_tabs, Some(true));
    assert_eq!(layout1.prose_wrap.as_deref(), Some("preserve"));
  }

  #[test]
  fn test_yaml_options_document_start_and_truthy_rules() {
    let toml = r#"
      [lang.yaml]
      indent_sequence = true
      document_start = false
      truthy = true
    "#;
    let parsed =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
    let yaml = parsed.resolve_for_lang("yaml");
    assert_eq!(
      yaml.yaml,
      Some(YamlOptions {
        indent_sequence: Some(true),
        document_start: Some(false),
        truthy: Some(true),
      })
    );
  }

  #[test]
  fn test_generate_sample_omits_languages() {
    let sample = FormalityConfig::generate_sample();
    assert!(sample.contains("# formality configuration file"));
    assert!(sample.contains(
      "#:schema https://github.com/arvinduh/formality/releases/download/v"
    ));
    assert!(sample.contains("[global]"));
    assert!(!sample.contains("languages ="));
    assert!(sample.contains("indent_size = 2"));
    assert!(sample.contains("line_length = 80"));
    assert!(sample.contains("end_of_line = \"lf\""));
    assert!(sample.contains("charset = \"utf-8\""));
    assert!(sample.contains("insert_final_newline = true"));
    assert!(sample.contains("trim_trailing_whitespace = true"));

    let parsed =
      FormalityConfig::parse_str(&sample, Path::new("formality.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(global.languages, None);
    assert_eq!(global.indent_size, 2);
    assert_eq!(global.line_length, 80);
    assert_eq!(global.end_of_line, "lf");
    assert_eq!(global.charset, "utf-8");
    assert!(global.insert_final_newline);
    assert!(global.trim_trailing_whitespace);
  }

  #[test]
  fn test_generate_init_template_omits_languages() {
    let template =
      FormalityConfig::generate_init_template(&["rust", "python", "toml"]);
    assert!(!template.contains("languages ="));
    assert!(template.contains("[global]"));

    let parsed =
      FormalityConfig::parse_str(&template, Path::new("formality.toml"))
        .unwrap();
    let global = parsed.resolve_global();
    assert_eq!(global.languages, None);
  }
}
