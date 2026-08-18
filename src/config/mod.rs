pub mod facets;
pub mod options;
pub mod resolve;
pub mod schema;

pub use facets::LayoutFacet;
pub use options::{
  CppOptions, GoOptions, JavaScriptOptions, JsonOptions, KotlinOptions,
  MarkdownOptions, PythonOptions, RustOptions, TomlOptions, TypstOptions,
  YamlOptions,
};
pub use resolve::{find_project_config, find_user_config};
pub use schema::generate_schema;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_CONFIG_FILE_NAME: &str = "formality.toml";
pub const CONFIG_FILE_CANDIDATES: &[&str] =
  &["formality.toml", ".formality.toml"];

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
  pub go: Option<GoOptions>,
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
  pub javascript: Option<JavaScriptOptions>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kotlin: Option<KotlinOptions>,
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
    if let Some(other_go) = other.go {
      if let Some(ref mut our_go) = self.go {
        our_go.merge(other_go);
      } else {
        self.go = Some(other_go);
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
    if let Some(other_js) = other.javascript {
      if let Some(ref mut our_js) = self.javascript {
        our_js.merge(other_js);
      } else {
        self.javascript = Some(other_js);
      }
    }
    if let Some(other_kotlin) = other.kotlin {
      if let Some(ref mut our_kotlin) = self.kotlin {
        our_kotlin.merge(other_kotlin);
      } else {
        self.kotlin = Some(other_kotlin);
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

  pub fn go_options(&self) -> Option<GoOptions> {
    extract_options(
      self.go.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |g| g.is_empty(),
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

  pub fn javascript_options(&self) -> Option<JavaScriptOptions> {
    extract_options(
      self.javascript.clone(),
      &self.options,
      &self.extra,
      |cur, other| cur.merge(other),
      |j| j.is_empty(),
    )
  }

  pub fn kotlin_options(&self) -> Option<KotlinOptions> {
    extract_options(
      self.kotlin.clone(),
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
  pub go: Option<GoOptions>,
  pub markdown: Option<MarkdownOptions>,
  pub yaml: Option<YamlOptions>,
  pub json: Option<JsonOptions>,
  pub toml: Option<TomlOptions>,
  pub typst: Option<TypstOptions>,
  pub javascript: Option<JavaScriptOptions>,
  pub kotlin: Option<KotlinOptions>,
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

#[cfg(test)]
mod tests;
