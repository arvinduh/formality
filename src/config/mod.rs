//! `formality.toml` parsing, cascade resolution (user config → project
//! config → per-language overrides), and the typed config surface every
//! other module reads through ([`FormalityConfig`], [`ResolvedGlobalConfig`],
//! [`ResolvedLangConfig`]).

/// Formatting and linting layout facet definitions.
pub mod facets;
/// X-macro table generating the repetitive per-language options wiring
/// shared by `LangConfig`/`resolve_for_lang`/`default_tools_for_lang` —
/// see its module docs for the design.
mod lang_table;
/// Per-language strongly typed formatting options.
pub mod options;
/// Configuration parsing, cascade merging, and path resolution.
pub mod resolve;
/// JSON Schema generator for formality.toml configuration validation.
pub mod schema;

pub use facets::LayoutFacet;
pub use options::{
  CppOptions, GoOptions, JavaOptions, JavaScriptOptions, JsonOptions,
  KotlinOptions, MarkdownOptions, PythonOptions, RustOptions, TomlOptions,
  TypstOptions, YamlOptions,
};
pub use resolve::{find_project_config, find_user_config};
pub use schema::{
  SCHEMA_VERSION, SchemaStatus, check_schema_version_content,
  check_schema_version_file, generate_schema, parse_schema_version,
  print_schema_notice, spawn_schema_check,
};

use lang_table::{impl_lang_accessors, impl_lang_merge, lang_options_table};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Default configuration filename (`formality.toml`).
pub const DEFAULT_CONFIG_FILE_NAME: &str = "formality.toml";
/// Supported configuration file candidates in lookup order.
pub const CONFIG_FILE_CANDIDATES: &[&str] =
  &["formality.toml", ".formality.toml"];

/// Global default settings applicable across all language surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GlobalConfig {
  /// Explicit list of active language surface names to manage.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub languages: Option<Vec<String>>,
  /// List of language surface names to ignore/skip.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ignore_languages: Option<Vec<String>>,
  /// Default indentation width (spaces).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  /// Default maximum line length.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  /// Default line ending format (`"lf"` or `"crlf"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub end_of_line: Option<String>,
  /// Default file encoding charset (e.g. `"utf-8"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub charset: Option<String>,
  /// Whether files should end with a trailing newline.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub insert_final_newline: Option<bool>,
  /// Whether trailing whitespace on lines should be trimmed.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trim_trailing_whitespace: Option<bool>,
  /// Whether to use tabs instead of spaces for indentation.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  /// Optional layout facet settings override.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub layout: Option<LayoutFacet>,
  /// Global file path exclude patterns.
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
  /// Merges values from `other` into `self`, overwriting set fields.
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
  options: Option<&toml::Value>,
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
/// Per-language configuration section (`[lang.<surface>]`).
#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub struct LangConfig {
  /// Custom formatter tool binary or command name override.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub format_tool: Option<String>,
  /// Custom linter tool binary or command name override.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub lint_tool: Option<String>,
  /// Per-language indentation size override.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  /// Per-language line length override.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  /// Whether to use tabs for indentation in this surface.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  /// Prose wrapping strategy (for Markdown, etc.).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
  /// Whether this language surface is enabled.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub enabled: Option<bool>,
  /// Additional command-line arguments to pass to the underlying tool. A
  /// surface that drives more than one binary (e.g. markdown's
  /// markdownlint + prettier, Python's isort + ruff format) forwards this
  /// same list to every tool invocation it makes — there is no per-tool
  /// split, so set only flags valid across all of them (Fixes #150).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub extra_args: Option<Vec<String>>,
  /// Explicit file pattern inclusions for this surface.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub files: Option<Vec<PathBuf>>,
  /// Explicit file pattern exclusions for this surface.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub exclude: Option<Vec<PathBuf>>,
  /// Surface layout facet configuration.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub layout: Option<LayoutFacet>,
  /// Rust surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rust: Option<RustOptions>,
  /// Python surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub python: Option<PythonOptions>,
  /// C/C++ surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cpp: Option<CppOptions>,
  // NOTE: `java` is intentionally kept as its own clearly-scoped block,
  // alphabetically between `cpp` and `markdown`, to keep merges with
  // sibling language-surface additions (JS/TS, Go, Kotlin) low-conflict.
  /// Java surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub java: Option<JavaOptions>,
  /// Go surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub go: Option<GoOptions>,
  /// Markdown surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub markdown: Option<MarkdownOptions>,
  /// YAML surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub yaml: Option<YamlOptions>,
  /// JSON surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub json: Option<JsonOptions>,
  /// TOML surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub toml: Option<TomlOptions>,
  /// Typst surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub typst: Option<TypstOptions>,
  /// JavaScript/TypeScript surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub javascript: Option<JavaScriptOptions>,
  /// Kotlin surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kotlin: Option<KotlinOptions>,
  /// Untyped options table for custom options.
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(skip)]
  pub options: Option<toml::Value>,
  /// Extra unrecognized fields parsed from TOML.
  #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
  #[schemars(skip)]
  pub extra: BTreeMap<String, toml::Value>,
}

impl LangConfig {
  /// Merges `other` configuration settings into `self`.
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

    macro_rules! merge_option {
      ($field:ident) => {
        if let Some(other_val) = other.$field {
          if let Some(ref mut our_val) = self.$field {
            our_val.merge(other_val);
          } else {
            self.$field = Some(other_val);
          }
        }
      };
    }

    merge_option!(layout);
    // markdown is deliberately excluded from `lang_options_table!` (see
    // src/config/lang_table.rs), so it stays merged by hand here even
    // though this arm itself is otherwise identical to the generated ones.
    merge_option!(markdown);
    lang_options_table!(impl_lang_merge, self, other);

    if other.options.is_some() {
      self.options = other.options;
    }
    for (k, v) in other.extra {
      self.extra.insert(k, v);
    }
  }

  lang_options_table!(impl_lang_accessors);

  /// Extracts resolved [`MarkdownOptions`].
  #[must_use]
  pub fn markdown_options(&self) -> Option<MarkdownOptions> {
    let mut opts = extract_options(
      self.markdown.clone(),
      self.options.as_ref(),
      &self.extra,
      options::MarkdownOptions::merge,
      options::MarkdownOptions::is_empty,
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
}

/// Root formality configuration structure matching `formality.toml`.
#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub struct FormalityConfig {
  /// Global defaults block (`[global]`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub global: Option<GlobalConfig>,
  /// Per-language surface configuration map (`[lang.<name>]`).
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub lang: BTreeMap<String, LangConfig>,
}

/// Fully resolved global configuration with all default fallbacks applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGlobalConfig {
  /// Explicit active languages, if specified.
  pub languages: Option<Vec<String>>,
  /// Ignored languages list, if specified.
  pub ignore_languages: Option<Vec<String>>,
  /// Effective indentation size.
  pub indent_size: usize,
  /// Effective line length limit.
  pub line_length: usize,
  /// Effective line ending style.
  pub end_of_line: String,
  /// Effective character encoding charset.
  pub charset: String,
  /// Effective trailing newline requirement.
  pub insert_final_newline: bool,
  /// Effective trailing whitespace trimming requirement.
  pub trim_trailing_whitespace: bool,
  /// Whether tab indentation is enabled.
  pub use_tabs: bool,
  /// Synthesized layout facet.
  pub layout: LayoutFacet,
  /// Resolved global exclude file paths.
  pub exclude: Vec<PathBuf>,
}

impl Default for ResolvedGlobalConfig {
  fn default() -> Self {
    FormalityConfig::with_defaults().resolve_global()
  }
}

/// Fully resolved per-language surface configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLangConfig {
  /// Surface identifier name.
  pub name: String,
  /// Selected formatting tool binary.
  pub format_tool: Option<String>,
  /// Selected linting tool binary.
  pub lint_tool: Option<String>,
  /// Resolved indentation size.
  pub line_length: usize,
  /// Resolved line length.
  pub indent_size: usize,
  /// Whether tab indentation is enabled.
  pub use_tabs: bool,
  /// Resolved prose wrap strategy.
  pub prose_wrap: Option<String>,
  /// Resolved surface layout facet.
  pub layout: LayoutFacet,
  /// Whether this language surface is active/enabled.
  pub enabled: bool,
  /// Extra CLI arguments for tools. Forwarded verbatim to every tool
  /// invocation a surface's `format()`/`lint()` makes, including each pass
  /// of a multi-tool surface — see the doc comment on
  /// `LangConfig::extra_args` (Fixes #150).
  pub extra_args: Vec<String>,
  /// Targeted file path inclusions.
  pub files: Vec<PathBuf>,
  /// Excluded file paths.
  pub exclude: Vec<PathBuf>,
  /// Resolved Rust surface options.
  pub rust: Option<RustOptions>,
  /// Resolved Python surface options.
  pub python: Option<PythonOptions>,
  /// Resolved C/C++ surface options.
  pub cpp: Option<CppOptions>,
  /// Resolved Java surface options.
  pub java: Option<JavaOptions>,
  /// Resolved Go surface options.
  pub go: Option<GoOptions>,
  /// Resolved Markdown surface options.
  pub markdown: Option<MarkdownOptions>,
  /// Resolved YAML surface options.
  pub yaml: Option<YamlOptions>,
  /// Resolved JSON surface options.
  pub json: Option<JsonOptions>,
  /// Resolved TOML surface options.
  pub toml: Option<TomlOptions>,
  /// Resolved Typst surface options.
  pub typst: Option<TypstOptions>,
  /// Resolved JavaScript/TypeScript surface options.
  pub javascript: Option<JavaScriptOptions>,
  /// Resolved Kotlin surface options.
  pub kotlin: Option<KotlinOptions>,
  /// Extra key-value options.
  pub extra: BTreeMap<String, toml::Value>,
}

impl ResolvedLangConfig {
  /// Creates a [`ResolvedLangConfig`] with default settings for the named surface.
  #[must_use]
  pub fn new(name: &str) -> Self {
    FormalityConfig::with_defaults().resolve_for_lang(name)
  }
}

/// Errors occurring during configuration loading, parsing, or validation.
#[derive(Debug)]
pub enum ConfigError {
  /// File system IO error while reading configuration file.
  Io {
    /// File path where IO error occurred.
    path: PathBuf,
    /// Underlying IO error.
    source: std::io::Error,
  },
  /// TOML deserialization or syntax error.
  Parse {
    /// File path where parse error occurred.
    path: PathBuf,
    /// Underlying TOML error.
    source: toml::de::Error,
  },
  /// Logical or validation configuration error.
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
      ConfigError::Invalid(msg) => write!(f, "Invalid config: {msg}"),
    }
  }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
