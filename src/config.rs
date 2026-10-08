//! `formality.toml` parsing, cascade resolution, and typed models.
//!
//! Owns the configuration schema and layered resolution logic. Strict document
//! syntax validation lives in `strict`, option definitions live in `options`,
//! and schema generation lives in `schema`.

/// Formatting and linting layout facet definitions.
pub mod facets;
/// X-macro table generating the repetitive per-language options wiring
/// shared by `LangConfig`/`resolve_for_lang` — see its module docs for the
/// design.
mod lang_table;
/// Per-language strongly typed formatting options.
pub mod options;
/// Configuration parsing, cascade merging, and path resolution.
pub mod resolve;
/// JSON Schema generator for formality.toml configuration validation.
pub mod schema;
/// Strict document parsing that locates a rejected key by path and line.
mod strict;

use std::collections;
use std::path;

use schemars;
use serde;
use toml;

// Macros, imported by name: the X-macro's recursion and its `$callback`
// ident resolve at the call site, so a module path cannot reach them.
use crate::config::lang_table::impl_lang_accessors;
use crate::config::lang_table::impl_lang_merge;
use crate::config::lang_table::lang_options_table;
use crate::surfaces::tooling;

/// Default configuration filename (`formality.toml`).
pub const DEFAULT_CONFIG_FILE_NAME: &str = "formality.toml";
/// Supported configuration file candidates in lookup order.
pub const CONFIG_FILE_CANDIDATES: &[&str] =
  &["formality.toml", ".formality.toml"];

/// Global default settings applicable across all language surfaces.
#[derive(
  Debug,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
  PartialEq,
  Clone,
)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
  /// Explicit list of active language surface names to manage.
  #[serde(skip_serializing_if = "Option::is_none")]
  languages: Option<Vec<String>>,
  /// List of language surface names to ignore/skip.
  #[serde(skip_serializing_if = "Option::is_none")]
  ignore_languages: Option<Vec<String>>,
  /// Default indentation width (spaces).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  /// Default maximum line length.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  /// Default line ending format (`"lf"` or `"crlf"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  end_of_line: Option<String>,
  /// Default file encoding charset (e.g. `"utf-8"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  charset: Option<String>,
  /// Whether files should end with a trailing newline.
  #[serde(skip_serializing_if = "Option::is_none")]
  insert_final_newline: Option<bool>,
  /// Whether trailing whitespace on lines should be trimmed.
  #[serde(skip_serializing_if = "Option::is_none")]
  trim_trailing_whitespace: Option<bool>,
  /// Whether to use tabs instead of spaces for indentation.
  #[serde(skip_serializing_if = "Option::is_none")]
  use_tabs: Option<bool>,
  /// Optional layout facet settings override.
  #[serde(skip_serializing_if = "Option::is_none")]
  layout: Option<facets::LayoutFacet>,
  /// Global file path exclude patterns.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  exclude: Vec<path::PathBuf>,
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
  fn merge(&mut self, other: GlobalConfig) {
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
  extra: &collections::BTreeMap<String, toml::Value>,
  merge_fn: impl Fn(&mut T, T),
  is_empty_fn: impl Fn(&T) -> bool,
) -> Option<T>
where
  T: for<'de> serde::Deserialize<'de> + Clone,
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
  Debug,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
  Clone,
  Default,
  PartialEq,
)]
pub struct LangConfig {
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
  /// Additional command-line arguments per tool, keyed by the tool that
  /// receives them, e.g. `prettier = ["--prose-wrap", "always"]` under
  /// `[lang.markdown.extra_args]`. Each list is appended after fml's own
  /// flags for that tool only. Each surface accepts its own tool keys; see
  /// docs/language-surfaces.md.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub extra_args: Option<collections::BTreeMap<String, Vec<String>>>,
  /// Explicit file pattern inclusions for this surface.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub files: Option<Vec<path::PathBuf>>,
  /// Explicit file pattern exclusions for this surface.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub exclude: Option<Vec<path::PathBuf>>,
  /// Surface layout facet configuration.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub layout: Option<facets::LayoutFacet>,
  /// Rust surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rust: Option<options::RustOptions>,
  /// Python surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub python: Option<options::PythonOptions>,
  /// C/C++ surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cpp: Option<options::CppOptions>,
  // NOTE: `java` is intentionally kept as its own clearly-scoped block,
  // alphabetically between `cpp` and `markdown`, to keep merges with
  // sibling language-surface additions (JS/TS, Go, Kotlin) low-conflict.
  /// Java surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub java: Option<options::JavaOptions>,
  /// Go surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub go: Option<options::GoOptions>,
  /// Markdown surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub markdown: Option<options::MarkdownOptions>,
  /// YAML surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub yaml: Option<options::YamlOptions>,
  /// JSON surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub json: Option<options::JsonOptions>,
  /// TOML surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub toml: Option<options::TomlOptions>,
  /// Typst surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub typst: Option<options::TypstOptions>,
  /// JavaScript/TypeScript surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub javascript: Option<options::JavaScriptOptions>,
  /// Kotlin surface specific options.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kotlin: Option<options::KotlinOptions>,
  /// Untyped options table for custom options.
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(skip)]
  pub options: Option<toml::Value>,
  /// Extra unrecognized fields parsed from TOML.
  #[serde(
    default,
    flatten,
    skip_serializing_if = "collections::BTreeMap::is_empty"
  )]
  #[schemars(skip)]
  pub extra: collections::BTreeMap<String, toml::Value>,
}

impl LangConfig {
  /// Merges `other` configuration settings into `self`.
  fn merge(&mut self, other: LangConfig) {
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
    // Per tool: a later layer replaces one tool's list and keeps the rest.
    if let Some(other_args) = other.extra_args {
      self.extra_args.get_or_insert_default().extend(other_args);
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

  /// Extracts resolved [`options::MarkdownOptions`].
  #[must_use]
  fn markdown_options(&self) -> Option<options::MarkdownOptions> {
    let mut opts = extract_options(
      self.markdown.clone(),
      self.options.as_ref(),
      &self.extra,
      options::MarkdownOptions::merge,
      options::MarkdownOptions::is_empty,
    );
    if opts.is_none() {
      if let Some(ref pw) = self.prose_wrap {
        opts = Some(options::MarkdownOptions {
          prose_wrap: Some(pw.clone()),
          no_inline_html: None,
        });
      } else if let Some(ref l) = self.layout
        && let Some(ref pw) = l.prose_wrap
      {
        opts = Some(options::MarkdownOptions {
          prose_wrap: Some(pw.clone()),
          no_inline_html: None,
        });
      }
    }
    opts
  }
}

/// Root formality configuration structure matching `formality.toml`.
#[derive(
  Debug,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
  PartialEq,
  Default,
  Clone,
)]
#[serde(deny_unknown_fields)]
pub struct FormalityConfig {
  /// Global defaults block (`[global]`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub global: Option<GlobalConfig>,
  /// Per-language surface configuration map (`[lang.<name>]`).
  #[serde(default, skip_serializing_if = "collections::BTreeMap::is_empty")]
  pub lang: collections::BTreeMap<String, LangConfig>,
}

/// Fully resolved global configuration with all default fallbacks applied.
#[derive(Debug, Clone)]
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
  pub layout: facets::LayoutFacet,
  /// Resolved global exclude file paths.
  pub exclude: Vec<path::PathBuf>,
}

impl Default for ResolvedGlobalConfig {
  fn default() -> Self {
    FormalityConfig::with_defaults().resolve_global()
  }
}

/// Fully resolved per-language surface configuration.
#[derive(Debug, PartialEq, Clone)]
pub struct ResolvedLangConfig {
  /// Surface identifier name.
  name: String,
  /// Resolved indentation size.
  pub line_length: usize,
  /// Resolved line length.
  pub indent_size: usize,
  /// Whether tab indentation is enabled.
  pub use_tabs: bool,
  /// Resolved prose wrap strategy.
  pub prose_wrap: Option<String>,
  /// Resolved surface layout facet.
  layout: facets::LayoutFacet,
  /// Whether this language surface is active/enabled.
  pub enabled: bool,
  /// Extra CLI arguments keyed by the tool that receives them; read through
  /// [`ResolvedLangConfig::tool_args`].
  pub extra_args: collections::BTreeMap<String, Vec<String>>,
  /// Targeted file path inclusions.
  pub files: Vec<path::PathBuf>,
  /// Excluded file paths.
  pub exclude: Vec<path::PathBuf>,
  /// Resolved Rust surface options.
  pub rust: Option<options::RustOptions>,
  /// Resolved Python surface options.
  pub python: Option<options::PythonOptions>,
  /// Resolved C/C++ surface options.
  pub cpp: Option<options::CppOptions>,
  /// Resolved Java surface options.
  pub java: Option<options::JavaOptions>,
  /// Resolved Go surface options.
  pub go: Option<options::GoOptions>,
  /// Resolved Markdown surface options.
  pub markdown: Option<options::MarkdownOptions>,
  /// Resolved YAML surface options.
  pub yaml: Option<options::YamlOptions>,
  /// Resolved JSON surface options.
  json: Option<options::JsonOptions>,
  /// Resolved TOML surface options.
  pub toml: Option<options::TomlOptions>,
  /// Resolved Typst surface options.
  typst: Option<options::TypstOptions>,
  /// Resolved JavaScript/TypeScript surface options.
  pub javascript: Option<options::JavaScriptOptions>,
  /// Resolved Kotlin surface options.
  kotlin: Option<options::KotlinOptions>,
  /// Extra key-value options.
  extra: collections::BTreeMap<String, toml::Value>,
}

#[cfg(test)]
impl ResolvedLangConfig {
  /// Creates a [`ResolvedLangConfig`] with default settings for the named surface.
  #[must_use]
  pub fn new(name: &str) -> Self {
    FormalityConfig::with_defaults().resolve_for_lang(name)
  }
}

impl ResolvedLangConfig {
  /// Returns the `extra_args` configured for `tool`, one of the keys the
  /// surface declares in `LanguageSurface::extra_args_tools`; empty when
  /// none are set.
  #[must_use]
  pub fn tool_args(&self, tool: &str) -> &[String] {
    self.extra_args.get(tool).map_or(&[], Vec::as_slice)
  }
}

/// Errors occurring during configuration loading, parsing, or validation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// File system IO error while reading configuration file.
  #[error("Failed to read config file at {}: {source}", path.display())]
  Io {
    /// File path where IO error occurred.
    path: path::PathBuf,
    /// Underlying IO error.
    source: std::io::Error,
  },
  /// TOML deserialization or syntax error.
  #[error("Failed to parse config file at {}: {source}", path.display())]
  Parse {
    /// File path where parse error occurred.
    path: path::PathBuf,
    /// Underlying TOML error.
    source: toml::de::Error,
  },
  /// A key this `fml` does not accept: misspelled, removed, or added by a
  /// newer `fml`.
  #[error(
    "unknown key `{key}` in {}:{line}. It may need a newer fml (`fml \
     --version`), or it is misspelled or was removed; `fml schema` lists \
     the keys this fml accepts.",
    path.display()
  )]
  UnknownKey {
    /// File path of the config holding the key.
    path: path::PathBuf,
    /// Dotted key path, e.g. `lang.python.format_tool`.
    key: String,
    /// One-based line of the key.
    line: usize,
  },
  /// A known key whose value has the wrong type or shape.
  #[error(
    "invalid value for `{key}` in {}:{line}: {reason}. Check `fml \
     schema` for the type this fml expects.",
    path.display()
  )]
  InvalidValue {
    /// File path of the config holding the value.
    path: path::PathBuf,
    /// Dotted key path, e.g. `global.line_length`.
    key: String,
    /// One-based line of the value.
    line: usize,
    /// Why the value was rejected, e.g.
    /// `invalid type: string "80", expected usize`.
    reason: String,
  },
  /// A `[lang.<name>]` section spelled as a surface alias or with other
  /// casing, which no reader would look up.
  #[error(
    "section `[lang.{name}]` in {}:{line} is not a canonical surface \
     name; rename it to `[lang.{canonical}]`.",
    path.display()
  )]
  NonCanonicalLang {
    /// File path of the config holding the section.
    path: path::PathBuf,
    /// The section name as written, e.g. `py`.
    name: String,
    /// The surface's canonical name, e.g. `python`.
    canonical: &'static str,
    /// One-based line of the section name.
    line: usize,
  },
  /// `[lang.<name>] extra_args` written as one flat list instead of a table
  /// keyed by tool.
  #[error(
    "`lang.{lang}.extra_args` in {}:{line} is a list, but extra_args \
     takes one list per tool: write it as a table, e.g. \
     `[lang.{lang}.extra_args]` then `{} = [\"--flag\"]`. Tools for \
     `{lang}`: `{}`.",
    path.display(),
    tools[0],
    tools.join("`, `")
  )]
  FlatExtraArgs {
    /// File path of the config holding the list.
    path: path::PathBuf,
    /// The section name, e.g. `markdown`.
    lang: String,
    /// One-based line of the `extra_args` key.
    line: usize,
    /// The tool keys the surface accepts; never empty, as every surface
    /// declares at least one (`every_surface_declares_extra_args_tools`).
    tools: &'static [&'static str],
  },
  /// An `extra_args` key naming no tool its surface runs.
  #[error("{}", format_unknown_tool(path, lang, tool, *line, tools))]
  UnknownTool {
    /// File path of the config holding the key.
    path: path::PathBuf,
    /// The section name, e.g. `markdown`.
    lang: String,
    /// The key as written, e.g. `prettierr`.
    tool: String,
    /// One-based line of the key.
    line: usize,
    /// The tool keys the surface accepts. `Display` suggests the one `tool`
    /// is a legacy alias of, e.g. `clippy-driver` for `clippy`, rather than
    /// storing it: a stored field pushes the error past clippy's
    /// `result_large_err` limit on Windows, where `PathBuf` is larger.
    tools: &'static [&'static str],
  },
}

fn format_unknown_tool(
  path: &path::Path,
  lang: &str,
  tool: &str,
  line: usize,
  tools: &[&str],
) -> String {
  let canonical = tooling::canonical_binary(tool);
  let suggestion = match tools.iter().find(|key| **key == canonical) {
    Some(key) => format!(" Did you mean `{key}`?"),
    None => String::new(),
  };
  format!(
    "unknown key `lang.{lang}.extra_args.{tool}` in {}:{line}: \
     `{lang}` runs no tool named `{tool}`; its extra_args keys are \
     `{}`.{suggestion}",
    path.display(),
    tools.join("`, `")
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::surfaces;
  use std::fs;
  use std::path;

  #[test]
  fn default_resolution() {
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
    assert!(rust.enabled);
    assert_eq!(rust.layout.indent_size, Some(2));
    assert_eq!(rust.layout.line_length, Some(80));
    assert_eq!(rust.rust, Some(options::RustOptions::default()));
  }

  #[test]
  fn resolve_for_lang_with_global_equivalence() {
    let toml = r"
      [global]
      indent_size = 4
      line_length = 100
      use_tabs = true

      [lang.rust]
      indent_size = 2

      [lang.python]
      line_length = 88
    ";
    let cfg =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let global = cfg.resolve_global();

    for lang in &[
      "rust",
      "python",
      "cpp",
      "java",
      "go",
      "markdown",
      "yaml",
      "json",
      "toml",
      "typst",
      "javascript",
      "kotlin",
    ] {
      let resolved_standard = cfg.resolve_for_lang(lang);
      let resolved_with_global =
        cfg.resolve_for_lang_with_global(lang, &global);
      assert_eq!(resolved_standard, resolved_with_global);
    }
  }

  /// Confirms every table-driven surface (all twelve except the
  /// hand-written `markdown`) gets its typed options struct populated with
  /// its `Default` value when resolved with no explicit `[lang.X]`
  /// section — i.e. `build_resolved_lang_config!`'s per-row
  /// `.or_else(|| if lang_name == "..." { Some(<Ty>::default()) } ...)`
  /// fallback actually fires for each row, and `LangConfig::merge`'s
  /// generated per-field merge picks up an explicit override afterward.
  #[test]
  fn macro_generated_accessors_default_and_merge_for_all_table_rows() {
    let cfg = FormalityConfig::with_defaults();

    let rust = cfg.resolve_for_lang("rust");
    assert_eq!(rust.rust, Some(options::RustOptions::default()));
    let python = cfg.resolve_for_lang("python");
    assert_eq!(python.python, Some(options::PythonOptions::default()));
    let cpp = cfg.resolve_for_lang("cpp");
    assert_eq!(cpp.cpp, Some(options::CppOptions::default()));
    let java = cfg.resolve_for_lang("java");
    assert_eq!(java.java, Some(options::JavaOptions::default()));
    let go = cfg.resolve_for_lang("go");
    assert_eq!(go.go, Some(options::GoOptions::default()));
    let yaml = cfg.resolve_for_lang("yaml");
    assert_eq!(yaml.yaml, Some(options::YamlOptions::default()));
    let json = cfg.resolve_for_lang("json");
    assert_eq!(json.json, Some(options::JsonOptions::default()));
    let toml_lang = cfg.resolve_for_lang("toml");
    assert_eq!(toml_lang.toml, Some(options::TomlOptions::default()));
    let typst = cfg.resolve_for_lang("typst");
    assert_eq!(typst.typst, Some(options::TypstOptions::default()));
    let javascript = cfg.resolve_for_lang("javascript");
    assert_eq!(
      javascript.javascript,
      Some(options::JavaScriptOptions::default())
    );
    let kotlin = cfg.resolve_for_lang("kotlin");
    assert_eq!(kotlin.kotlin, Some(options::KotlinOptions::default()));

    // A surface not being resolved gets `None` for a language it isn't,
    // rather than every row's default leaking into every other language.
    assert_eq!(rust.python, None);
    assert_eq!(rust.kotlin, None);

    // Merge (LangConfig::merge, generated by `impl_lang_merge!`) still
    // overwrites-or-merges per row when an explicit override is present.
    let mut base = FormalityConfig::default();
    base.lang.insert(
      "rust".to_string(),
      LangConfig {
        rust: Some(options::RustOptions {
          edition: Some("2018".to_string()),
        }),
        ..Default::default()
      },
    );
    let mut override_cfg = FormalityConfig::default();
    override_cfg.lang.insert(
      "rust".to_string(),
      LangConfig {
        rust: Some(options::RustOptions {
          edition: Some("2021".to_string()),
        }),
        ..Default::default()
      },
    );
    base.merge(override_cfg);
    let merged_rust = base.resolve_for_lang("rust");
    assert_eq!(
      merged_rust.rust,
      Some(options::RustOptions {
        edition: Some("2021".to_string()),
      })
    );
  }

  #[test]
  fn find_project_config_candidates() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    // No config initially
    assert_eq!(resolve::find_project_config(root), None);

    // Test .formality.toml
    let hidden = root.join(".formality.toml");
    fs::write(&hidden, "[global]\nindent_size = 4\n").unwrap();
    assert_eq!(resolve::find_project_config(root), Some(hidden));

    // Test formality.toml (higher precedence than .formality.toml)
    let standard = root.join("formality.toml");
    fs::write(&standard, "[global]\nindent_size = 2\n").unwrap();
    assert_eq!(resolve::find_project_config(root), Some(standard));
  }

  #[test]
  fn languages_list_parsing() {
    let toml = r#"
        [global]
        languages = ["rust", "toml"]
        ignore_languages = ["cpp"]
        indent_size = 4
      "#;
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let global = parsed.resolve_global();
    assert_eq!(
      global.languages,
      Some(vec!["rust".to_string(), "toml".to_string()])
    );
    assert_eq!(global.ignore_languages, Some(vec!["cpp".to_string()]));
    assert_eq!(global.indent_size, 4);
  }

  #[test]
  fn merge_and_override() {
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
      FormalityConfig::parse_str(override_toml, path::Path::new("test.toml"))
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
  fn lang_config_extra_args_files_and_exclude() {
    let toml = r#"
        [global]
        indent_size = 2

        [lang.rust]
        extra_args = { clippy-driver = ["--verbose", "--", "-D", "clippy::all"] }
        files = ["src/lib.rs", "src/main.rs"]
        exclude = ["tests/fixtures", "src/generated/**"]
      "#;
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.tool_args("clippy-driver"),
      ["--verbose", "--", "-D", "clippy::all"]
    );
    assert!(rust.tool_args("rustfmt").is_empty());
    assert_eq!(
      rust.files,
      vec![
        path::PathBuf::from("src/lib.rs"),
        path::PathBuf::from("src/main.rs")
      ]
    );
    assert_eq!(
      rust.exclude,
      vec![
        path::PathBuf::from("tests/fixtures"),
        path::PathBuf::from("src/generated/**")
      ]
    );
  }

  #[test]
  fn layout_facet_direct_and_inheritance() {
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
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
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
      Some(options::MarkdownOptions {
        prose_wrap: Some("always".to_string()),
        no_inline_html: None,
      })
    );
  }

  #[test]
  fn typed_options_deserialization_from_toml() {
    let toml = r#"
        [lang.rust]
        edition = "2021"

        [lang.python]
        quote_style = "single"
        target_version = "py311"
        ignore_rules = ["E501", "F401"]

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
        align_entries = true
        indent_entries = false
        indent_tables = true

        [lang.typst]
      "#;
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();

    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.rust,
      Some(options::RustOptions {
        edition: Some("2021".to_string()),
      })
    );

    let python = parsed.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(options::PythonOptions {
        quote_style: Some("single".to_string()),
        target_version: Some("py311".to_string()),
        ignore_rules: Some(vec!["E501".to_string(), "F401".to_string()]),
      })
    );

    let cpp = parsed.resolve_for_lang("cpp");
    assert_eq!(
      cpp.cpp,
      Some(options::CppOptions {
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
      Some(options::MarkdownOptions {
        prose_wrap: Some("never".to_string()),
        no_inline_html: None,
      })
    );

    let yaml = parsed.resolve_for_lang("yaml");
    assert_eq!(
      yaml.yaml,
      Some(options::YamlOptions {
        indent_sequence: Some(true),
        document_start: None,
        truthy: None,
      })
    );

    let json = parsed.resolve_for_lang("json");
    assert_eq!(json.json, Some(options::JsonOptions {}));

    let toml_lang = parsed.resolve_for_lang("toml");
    assert_eq!(
      toml_lang.toml,
      Some(options::TomlOptions {
        align_entries: Some(true),
        indent_entries: Some(false),
        indent_tables: Some(true),
      })
    );

    let typst = parsed.resolve_for_lang("typst");
    assert_eq!(typst.typst, Some(options::TypstOptions {}));
  }

  #[test]
  fn typed_options_subtable_deserialization() {
    let toml = r#"
        [lang.rust.rust]
        edition = "2024"

        [lang.python.python]
        quote_style = "double"
        target_version = "py312"
        ignore_rules = ["E501"]

        [lang.cpp.cpp]
        standard = "c++23"
        column_limit = 120
        based_on_style = "Chromium"
        pointer_alignment = "Right"
        break_before_braces = "Allman"
        sort_includes = false

        [lang.yaml.yaml]
        indent_sequence = false

        [lang.toml.toml]
        align_entries = true
        indent_tables = false
      "#;
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();

    let rust = parsed.resolve_for_lang("rust");
    assert_eq!(
      rust.rust,
      Some(options::RustOptions {
        edition: Some("2024".to_string()),
      })
    );

    let python = parsed.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(options::PythonOptions {
        quote_style: Some("double".to_string()),
        target_version: Some("py312".to_string()),
        ignore_rules: Some(vec!["E501".to_string()]),
      })
    );

    let cpp = parsed.resolve_for_lang("cpp");
    assert_eq!(
      cpp.cpp,
      Some(options::CppOptions {
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
      Some(options::YamlOptions {
        indent_sequence: Some(false),
        document_start: None,
        truthy: None,
      })
    );

    let toml_lang = parsed.resolve_for_lang("toml");
    assert_eq!(
      toml_lang.toml,
      Some(options::TomlOptions {
        align_entries: Some(true),
        indent_entries: None,
        indent_tables: Some(false),
      })
    );
  }

  #[test]
  fn typed_options_merging_semantics() {
    let mut base = FormalityConfig::default();
    let base_toml = r#"
        [global]
        indent_size = 2
        line_length = 80

        [lang.rust]
        edition = "2021"
        indent_size = 4

        [lang.python]
        quote_style = "single"
        ignore_rules = ["E501"]

        [lang.toml]
        align_entries = false
        indent_entries = true
      "#;
    base.merge(
      FormalityConfig::parse_str(base_toml, path::Path::new("base.toml"))
        .unwrap(),
    );

    let override_toml = r#"
        [lang.rust]
        edition = "2024"
        line_length = 100

        [lang.python]
        target_version = "py312"
        ignore_rules = ["F401", "SIM101"]

        [lang.toml]
        align_entries = true
        indent_tables = true
      "#;
    base.merge(
      FormalityConfig::parse_str(
        override_toml,
        path::Path::new("override.toml"),
      )
      .unwrap(),
    );

    let rust = base.resolve_for_lang("rust");
    assert_eq!(rust.indent_size, 4);
    assert_eq!(rust.line_length, 100);
    assert_eq!(
      rust.rust,
      Some(options::RustOptions {
        edition: Some("2024".to_string()),
      })
    );

    let python = base.resolve_for_lang("python");
    assert_eq!(
      python.python,
      Some(options::PythonOptions {
        quote_style: Some("single".to_string()),
        target_version: Some("py312".to_string()),
        ignore_rules: Some(vec!["F401".to_string(), "SIM101".to_string()]),
      })
    );

    let toml_lang = base.resolve_for_lang("toml");
    assert_eq!(
      toml_lang.toml,
      Some(options::TomlOptions {
        align_entries: Some(true),
        indent_entries: Some(true),
        indent_tables: Some(true),
      })
    );
  }

  #[test]
  fn serialization_deserialization_roundtrip() {
    let mut config = FormalityConfig {
      global: Some(GlobalConfig {
        languages: Some(vec!["rust".to_string(), "python".to_string()]),
        ignore_languages: None,
        indent_size: Some(2),
        line_length: Some(100),
        end_of_line: Some("lf".to_string()),
        charset: Some("utf-8".to_string()),
        insert_final_newline: Some(true),
        trim_trailing_whitespace: Some(true),
        use_tabs: Some(false),
        layout: Some(facets::LayoutFacet {
          indent_size: Some(2),
          line_length: Some(100),
          use_tabs: Some(false),
          prose_wrap: Some("always".to_string()),
        }),
        exclude: Vec::new(),
      }),
      ..Default::default()
    };

    let rust_cfg = LangConfig {
      indent_size: Some(4),
      rust: Some(options::RustOptions {
        edition: Some("2024".to_string()),
      }),
      ..Default::default()
    };
    config.lang.insert("rust".to_string(), rust_cfg);

    let py_cfg = LangConfig {
      python: Some(options::PythonOptions {
        quote_style: Some("double".to_string()),
        target_version: Some("py311".to_string()),
        ignore_rules: Some(vec!["E501".to_string()]),
      }),
      ..Default::default()
    };
    config.lang.insert("python".to_string(), py_cfg);

    let serialized = toml::to_string(&config).unwrap();
    let deserialized: FormalityConfig =
      FormalityConfig::parse_str(&serialized, path::Path::new("test.toml"))
        .unwrap();

    assert_eq!(config, deserialized);
  }

  #[test]
  fn language_options_merge_units() {
    let mut rust1 = options::RustOptions {
      edition: Some("2021".to_string()),
    };
    let rust2 = options::RustOptions {
      edition: Some("2024".to_string()),
    };
    rust1.merge(rust2);
    assert_eq!(rust1.edition.as_deref(), Some("2024"));

    let mut py1 = options::PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: None,
      ignore_rules: Some(vec!["E501".to_string()]),
    };
    let py2 = options::PythonOptions {
      quote_style: None,
      target_version: Some("py312".to_string()),
      ignore_rules: Some(vec!["F401".to_string()]),
    };
    py1.merge(py2);
    assert_eq!(py1.quote_style.as_deref(), Some("single"));
    assert_eq!(py1.target_version.as_deref(), Some("py312"));
    assert_eq!(py1.ignore_rules.as_deref(), Some(&["F401".to_string()][..]));

    let mut cpp1 = options::CppOptions {
      standard: Some("c++17".to_string()),
      column_limit: None,
      based_on_style: Some("LLVM".to_string()),
      pointer_alignment: None,
      break_before_braces: None,
      sort_includes: Some(true),
    };
    let cpp2 = options::CppOptions {
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

    let mut yaml1 = options::YamlOptions {
      indent_sequence: Some(true),
      document_start: Some(true),
      truthy: None,
    };
    let yaml2 = options::YamlOptions {
      indent_sequence: Some(false),
      document_start: None,
      truthy: Some(false),
    };
    yaml1.merge(yaml2);
    assert_eq!(yaml1.indent_sequence, Some(false));
    assert_eq!(yaml1.document_start, Some(true));
    assert_eq!(yaml1.truthy, Some(false));

    let mut toml1 = options::TomlOptions {
      align_entries: Some(true),
      indent_entries: Some(false),
      indent_tables: None,
    };
    let toml2 = options::TomlOptions {
      align_entries: None,
      indent_entries: Some(true),
      indent_tables: Some(true),
    };
    toml1.merge(toml2);
    assert_eq!(toml1.align_entries, Some(true));
    assert_eq!(toml1.indent_entries, Some(true));
    assert_eq!(toml1.indent_tables, Some(true));

    let mut layout1 = facets::LayoutFacet {
      indent_size: Some(2),
      line_length: None,
      use_tabs: None,
      prose_wrap: None,
    };
    let layout2 = facets::LayoutFacet {
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
  fn toml_options_alignment_and_indentation() {
    let toml = r"
        [lang.toml]
        align_entries = true
        indent_entries = true
        indent_tables = false
      ";
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let toml_lang = parsed.resolve_for_lang("toml");
    assert_eq!(
      toml_lang.toml,
      Some(options::TomlOptions {
        align_entries: Some(true),
        indent_entries: Some(true),
        indent_tables: Some(false),
      })
    );
  }

  #[test]
  fn yaml_options_document_start_and_truthy_rules() {
    let toml = r"
        [lang.yaml]
        indent_sequence = true
        document_start = false
        truthy = true
      ";
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let yaml = parsed.resolve_for_lang("yaml");
    assert_eq!(
      yaml.yaml,
      Some(options::YamlOptions {
        indent_sequence: Some(true),
        document_start: Some(false),
        truthy: Some(true),
      })
    );
  }

  #[test]
  fn generate_sample_omits_languages() {
    let sample = FormalityConfig::generate_sample();
    assert!(sample.contains("# formality configuration file"));
    assert!(sample.contains(
      "#:schema https://github.com/arvinduh/formality/releases/latest/download/formality.schema.json",
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
      FormalityConfig::parse_str(&sample, path::Path::new("formality.toml"))
        .unwrap();
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
  fn generate_init_template_omits_languages() {
    let template =
      FormalityConfig::generate_init_template(&["rust", "python", "toml"]);
    assert!(!template.contains("languages ="));
    assert!(template.contains("[global]"));

    let parsed =
      FormalityConfig::parse_str(&template, path::Path::new("formality.toml"))
        .unwrap();
    let global = parsed.resolve_global();
    assert_eq!(global.languages, None);
  }

  #[test]
  fn generate_init_template_emits_commented_lang_stubs_for_detected() {
    let template =
      FormalityConfig::generate_init_template(&["rust", "python", "toml"]);

    // Detected languages get a commented-out, ready-to-uncomment stub section
    // each, in deterministic sorted order.
    assert!(template.contains("# [lang.python]"));
    assert!(template.contains("# [lang.rust]"));
    assert!(template.contains("# [lang.toml]"));
    let python_pos = template.find("# [lang.python]").unwrap();
    let rust_pos = template.find("# [lang.rust]").unwrap();
    let toml_pos = template.find("# [lang.toml]").unwrap();
    assert!(python_pos < rust_pos);
    assert!(rust_pos < toml_pos);

    // A language that wasn't detected gets no stub.
    assert!(!template.contains("[lang.go]"));

    // Stubs are commented out, so the template still parses to an empty `lang`
    // map — they must not silently activate any override.
    let parsed =
      FormalityConfig::parse_str(&template, path::Path::new("formality.toml"))
        .unwrap();
    assert!(parsed.lang.is_empty());
  }

  #[test]
  fn generate_init_template_dedupes_langs() {
    let template = FormalityConfig::generate_init_template(&["rust", "rust"]);
    assert_eq!(template.matches("[lang.rust]").count(), 1);
  }

  #[test]
  fn generate_init_template_no_detected_langs_matches_sample() {
    let template = FormalityConfig::generate_init_template(&[]);
    assert_eq!(template, FormalityConfig::generate_sample());
  }

  #[test]
  fn unrecognized_lang_sections_flags_typo_but_not_valid_undetected() {
    let registry = surfaces::registry::SurfaceRegistry::default();

    // A genuine typo: "pythonn" names no surface, so it loads and is flagged.
    let toml = r"
      [lang.pythonn]
      indent_size = 4
    ";
    let cfg =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap();
    assert_eq!(
      cfg.unrecognized_lang_sections(&registry),
      vec!["pythonn"],
      "a typo'd section name should be flagged"
    );

    // A valid, recognized surface name that simply isn't active/detected in
    // the current workspace (pre-configuring for a language not yet in use)
    // must NOT be flagged — this is a legitimate, intentional override.
    let toml = r"
      [lang.rust]
      indent_size = 4
    ";
    let cfg =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap();
    assert!(
      cfg.unrecognized_lang_sections(&registry).is_empty(),
      "a valid but undetected surface name should not be flagged"
    );
  }

  #[test]
  fn load_file_missing_path_yields_io_error() {
    let missing = path::Path::new("this/path/definitely/does/not/exist.toml");
    let err = FormalityConfig::load_file(missing).unwrap_err();
    assert!(matches!(err, Error::Io { .. }));
    let msg = err.to_string();
    assert!(msg.contains("Failed to read config file at"));
    assert!(msg.contains("exist.toml"));
  }

  #[test]
  fn parse_str_malformed_toml_yields_parse_error() {
    // Missing closing bracket / invalid TOML syntax.
    let bad_toml = "[global\nindent_size = 2";
    let err =
      FormalityConfig::parse_str(bad_toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(matches!(err, Error::Parse { .. }));
    let msg = err.to_string();
    assert!(msg.contains("Failed to parse config file at"));
    assert!(msg.contains("formality.toml"));
  }

  #[test]
  fn java_aosp_style_defaults_indent_width_to_four() {
    // Java's indent_width is conditionally Fixed: google-java-format's
    // --aosp flag pins it to 4 spaces (vs. 2 for the default Google style),
    // resolved via `[lang.java] style` rather than a plain constant (see
    // docs/facet-rosetta.md, JavaSurface::facet_support). Neither the AOSP
    // branch nor its interaction with an explicit indent_size override had
    // any test coverage.
    let toml = r#"
        [lang.java]
        style = "aosp"
      "#;
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("test.toml")).unwrap();
    let java = parsed.resolve_for_lang("java");
    assert_eq!(java.indent_size, 4);

    // Default (Google) style keeps the ordinary global-inherited indent_size.
    let toml_google = r#"
        [lang.java]
        style = "google"
      "#;
    let parsed_google =
      FormalityConfig::parse_str(toml_google, path::Path::new("test.toml"))
        .unwrap();
    let java_google = parsed_google.resolve_for_lang("java");
    assert_eq!(java_google.indent_size, 2);

    // No [lang.java] section at all: not AOSP, so global default applies.
    let default_java =
      FormalityConfig::with_defaults().resolve_for_lang("java");
    assert_eq!(default_java.indent_size, 2);

    // An explicit indent_size override always wins over the AOSP inference.
    let toml_explicit = r#"
        [lang.java]
        style = "aosp"
        indent_size = 8
      "#;
    let parsed_explicit =
      FormalityConfig::parse_str(toml_explicit, path::Path::new("test.toml"))
        .unwrap();
    let java_explicit = parsed_explicit.resolve_for_lang("java");
    assert_eq!(java_explicit.indent_size, 8);
  }

  #[test]
  fn parse_str_rejects_non_canonical_lang_sections() {
    let toml = "[global]\nline_length = 100\n\n[lang.py]\nindent_size = 4\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "section `[lang.py]` in formality.toml:4 is not a canonical surface \
       name; rename it to `[lang.python]`."
    );

    let toml = "[lang.RUST]\nindent_size = 4\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(
        &err,
        Error::NonCanonicalLang { name, canonical: "rust", line: 1, .. }
          if name == "RUST"
      ),
      "{err:?}"
    );

    // The canonical spelling still loads, and its keys are applied.
    let toml = "[lang.python]\nindent_size = 3\n";
    let cfg =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap();
    assert_eq!(cfg.resolve_for_lang("python").indent_size, 3);
  }

  #[test]
  fn unrecognized_lang_sections_reports_every_unknown_name() {
    let registry = surfaces::registry::SurfaceRegistry::default();
    let toml = r"
      [lang.pythonn]
      indent_size = 4

      [lang.jaav]
      indent_size = 4
    ";
    let cfg =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap();
    let mut unrecognized = cfg.unrecognized_lang_sections(&registry);
    unrecognized.sort_unstable();
    assert_eq!(unrecognized, vec!["jaav", "pythonn"]);
  }

  #[test]
  fn corrupted_config_syntax_errors_and_recovery() {
    let temp = tempfile::TempDir::new().unwrap();

    // Test 1: Unclosed section header
    let path1 = temp.path().join("unclosed_sec.toml");
    fs::write(&path1, "[global\nindent_size = 2").unwrap();
    let err1 = FormalityConfig::load_file(&path1).unwrap_err();
    match &err1 {
      Error::Parse { path, source } => {
        assert_eq!(path, &path1);
        assert!(!source.to_string().is_empty());
      }
      other => panic!("Expected Parse error, got {other:?}"),
    }
    assert!(err1.to_string().contains("Failed to parse config file at"));

    // Test 2: Type mismatch (indent_size as string instead of int)
    let path2 = temp.path().join("type_mismatch.toml");
    fs::write(&path2, "[global]\nindent_size = \"two\"\n").unwrap();
    let err2 = FormalityConfig::load_file(&path2).unwrap_err();
    assert_eq!(
      err2.to_string(),
      format!(
        "invalid value for `global.indent_size` in {}:2: invalid type: \
         string \"two\", expected usize. Check `fml schema` for the type this \
         fml expects.",
        path2.display()
      )
    );

    // Test 3: Invalid TOML token / syntax error
    let path3 = temp.path().join("bad_syntax.toml");
    fs::write(&path3, "global = = = true\n").unwrap();
    let err3 = FormalityConfig::load_file(&path3).unwrap_err();
    assert!(matches!(err3, Error::Parse { .. }));

    // Test 4: File not found (Io error recovery)
    let path4 = temp.path().join("nonexistent_config.toml");
    let err4 = FormalityConfig::load_file(&path4).unwrap_err();
    assert!(matches!(err4, Error::Io { .. }));
    assert!(err4.to_string().contains("Failed to read config file at"));
  }

  #[test]
  fn corrupted_config_fuzzing_random_and_malformed_inputs() {
    let malformed_inputs = [
      "",
      "   \n\t   ",
      "\0\0\0\0",
      "[[[[[[[[[",
      "\"unclosed string",
      "[global]\nindent_size = -9999999999999999999999999999999999",
      "[lang.rust]\nindent_size = \"not a number\"",
      "[lang.python]\nextra_args = \"not an array\"",
      "true = false\n[123]\n===456",
    ];

    for input in malformed_inputs {
      let res = FormalityConfig::parse_str(input, path::Path::new("fuzz.toml"));
      if input.trim().is_empty() {
        assert!(res.is_ok(), "Empty input should parse to empty config");
      } else if let Err(err) = res {
        assert!(!err.to_string().is_empty());
      }
    }
  }

  #[test]
  fn layered_config_with_corrupted_project_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let bad_config = root.join("formality.toml");
    fs::write(&bad_config, "[global]\nline_length = \"invalid_length\"")
      .unwrap();

    let res = FormalityConfig::load_layered(Some(root));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
      err
        .to_string()
        .starts_with("invalid value for `global.line_length` in "),
      "{err}"
    );
    assert!(err.to_string().contains("formality.toml:2: "), "{err}");
  }

  #[test]
  fn parse_str_unknown_key_names_key_path_line_and_fix() {
    let toml = "[global]\nline_length = 80\nmax_width = 100\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `global.max_width` in formality.toml:3. It may need a \
       newer fml (`fml --version`), or it is misspelled or was removed; `fml \
       schema` lists the keys this fml accepts."
    );

    let nested = "[lang.python.python]\nquote_style = \"double\"\n\
                  ignore_rulez = [\"E501\"]\n";
    let err =
      FormalityConfig::parse_str(nested, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(
        &err,
        Error::UnknownKey { key, line: 3, .. }
          if key == "lang.python.python.ignore_rulez"
      ),
      "{err:?}"
    );
  }

  #[test]
  fn parse_str_unknown_root_key_is_unknown() {
    // A `[global]` key written at the root; `deny_unknown_fields` on
    // `FormalityConfig` is what reports it.
    let toml = "languages = [\"rust\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `languages` in formality.toml:1. It may need a newer fml \
       (`fml --version`), or it is misspelled or was removed; `fml schema` \
       lists the keys this fml accepts."
    );
  }

  #[test]
  fn parse_str_removed_flat_lang_key_is_unknown() {
    // `format_tool` left `LangConfig` in #280; the flattened `extra` map used
    // to swallow it without a word.
    let toml = "[lang.python]\nquote_style = \"double\"\nformat_tool = \
                \"black\"\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `lang.python.format_tool` in formality.toml:3. It may need \
       a newer fml (`fml --version`), or it is misspelled or was removed; \
       `fml schema` lists the keys this fml accepts."
    );
  }

  #[test]
  fn parse_str_flat_lang_keys_follow_their_own_surface() {
    // A key valid for one surface is unknown under another.
    let toml = "[lang.rust]\nedition = \"2024\"\nquote_style = \"double\"\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(
        &err,
        Error::UnknownKey { key, line: 3, .. }
          if key == "lang.rust.quote_style"
      ),
      "{err:?}"
    );

    let toml = "[lang.python]\ntarget_version = 310\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(
        &err,
        Error::InvalidValue { key, line: 2, .. }
          if key == "lang.python.target_version"
      ),
      "{err:?}"
    );

    let toml = "[lang.go.options]\nlocal_prefixes = \"example.com\"\n\
                shadow = true\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(
        &err,
        Error::UnknownKey { key, line: 3, .. }
          if key == "lang.go.options.shadow"
      ),
      "{err:?}"
    );
  }

  #[test]
  fn parse_str_non_table_lang_options_is_invalid_value() {
    let toml = "[lang.python]\nquote_style = \"double\"\noptions = \"oops\"\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "invalid value for `lang.python.options` in formality.toml:3: \
       invalid type: string \"oops\", expected struct PythonOptions. Check \
       `fml schema` for the type this fml expects."
    );
  }

  #[test]
  fn parse_str_wrong_type_names_nested_key_path_and_line() {
    let toml = "[global]\nline_length = 80\n\n[lang.rust.layout]\n\
                  indent_size = \"four\"\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "invalid value for `lang.rust.layout.indent_size` in formality.toml:5: \
           invalid type: string \"four\", expected usize. Check `fml schema` for \
           the type this fml expects."
    );
  }

  #[test]
  fn load_layered_with_path_matches_load_layered() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let config_file = root.join("formality.toml");
    fs::write(
      &config_file,
      "[global]\nindent_size = 4\nline_length = 120\n",
    )
    .unwrap();

    let (cfg_from_root, path_from_root) =
      FormalityConfig::load_layered(Some(root)).unwrap();
    let (cfg_from_path, path_from_path) =
      FormalityConfig::load_layered_with_path(Some(&config_file)).unwrap();

    assert_eq!(path_from_root, Some(config_file.clone()));
    assert_eq!(path_from_path, Some(config_file));
    assert_eq!(
      cfg_from_root.resolve_global().indent_size,
      cfg_from_path.resolve_global().indent_size
    );
    assert_eq!(
      cfg_from_root.resolve_global().line_length,
      cfg_from_path.resolve_global().line_length
    );
  }

  #[test]
  fn load_layered_with_path_none() {
    let (cfg, path) = FormalityConfig::load_layered_with_path(None).unwrap();
    assert_eq!(path, None);
    assert_eq!(cfg.resolve_global().indent_size, 2);
  }

  #[test]
  fn extra_args_table_routes_per_tool() {
    let toml = "[lang.python.extra_args]\nruff-format = [\"--preview\"]\n";
    let parsed =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap();
    let python = parsed.resolve_for_lang("python");
    assert_eq!(python.tool_args("ruff-format"), ["--preview"]);
    assert!(python.tool_args("ruff-check").is_empty());
  }

  #[test]
  fn extra_args_merge_replaces_per_tool() {
    let mut base = LangConfig {
      extra_args: Some(
        [
          ("prettier".to_string(), vec!["--a".to_string()]),
          ("markdownlint-cli2".to_string(), vec!["--b".to_string()]),
        ]
        .into(),
      ),
      ..LangConfig::default()
    };
    base.merge(LangConfig {
      extra_args: Some(
        [("prettier".to_string(), vec!["--c".to_string()])].into(),
      ),
      ..LangConfig::default()
    });
    assert_eq!(
      base.extra_args,
      Some(
        [
          ("prettier".to_string(), vec!["--c".to_string()]),
          ("markdownlint-cli2".to_string(), vec!["--b".to_string()]),
        ]
        .into()
      )
    );
  }

  #[test]
  fn extra_args_flat_list_names_table_form() {
    let toml = "[lang.markdown]\nline_length = 100\n\
                extra_args = [\"--prose-wrap\", \"always\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "`lang.markdown.extra_args` in formality.toml:3 is a list, but \
       extra_args takes one list per tool: write it as a table, e.g. \
       `[lang.markdown.extra_args]` then `markdownlint-cli2 = [\"--flag\"]`. \
       Tools for `markdown`: `markdownlint-cli2`, `prettier`."
    );
  }

  #[test]
  fn extra_args_flat_list_in_unknown_section_is_a_type_error() {
    // An unknown `[lang.<name>]` section has no tool keys to suggest, so a flat
    // list there gets the plain type error any mistyped key in it gets.
    let toml = "[lang.cobol]\nextra_args = [\"x\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert!(
      matches!(err, Error::InvalidValue { .. }),
      "expected InvalidValue, got: {err}"
    );
  }

  #[test]
  fn extra_args_unknown_tool_names_valid_keys() {
    let toml = "[lang.python.extra_args]\nruff-check = []\nruff = [\"-q\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `lang.python.extra_args.ruff` in formality.toml:3: \
       `python` runs no tool named `ruff`; its extra_args keys are \
       `ruff-check`, `ruff-format`."
    );
  }

  #[test]
  fn extra_args_unknown_tool_suggests_canonical_binary() {
    // Users type the familiar tool name; the key is the binary fml spawns.
    let toml = "[lang.rust.extra_args]\nclippy = [\"-Wclippy::pedantic\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `lang.rust.extra_args.clippy` in formality.toml:2: `rust` \
       runs no tool named `clippy`; its extra_args keys are `rustfmt`, \
       `clippy-driver`. Did you mean `clippy-driver`?"
    );

    let toml = "[lang.markdown.extra_args]\nmarkdownlint = [\"--fix\"]\n";
    let err =
      FormalityConfig::parse_str(toml, path::Path::new("formality.toml"))
        .unwrap_err();
    assert_eq!(
      err.to_string(),
      "unknown key `lang.markdown.extra_args.markdownlint` in \
       formality.toml:2: `markdown` runs no tool named `markdownlint`; its \
       extra_args keys are `markdownlint-cli2`, `prettier`. Did you mean \
       `markdownlint-cli2`?"
    );
  }
}
