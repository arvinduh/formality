//! Per-language strongly typed formatting/linting option structs (one per
//! surface, e.g. [`RustOptions`], [`PythonOptions`]) — the `[lang.*]` shape
//! `formality.toml` deserializes into before [`super::resolve`] merges it
//! with global config and surface defaults.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Implements `merge` and `is_empty` methods for a typed options struct.
macro_rules! impl_options_methods {
  ($ty:ident) => {
    impl $ty {
      /// Merges `other` options into `self`.
      #[allow(clippy::needless_pass_by_value)]
      pub fn merge(&mut self, _other: Self) {}

      /// Returns `true` if all fields are `None`.
      #[must_use]
      pub fn is_empty(&self) -> bool {
        true
      }
    }
  };
  ($ty:ident, $($field:ident),+ $(,)?) => {
    impl $ty {
      /// Merges `other` options into `self`.
      #[allow(clippy::needless_pass_by_value)]
      pub fn merge(&mut self, other: Self) {
        $(
          if other.$field.is_some() {
            self.$field = other.$field;
          }
        )*
      }

      /// Returns `true` if all fields are `None`.
      #[must_use]
      pub fn is_empty(&self) -> bool {
        $( self.$field.is_none() )&&*
      }
    }
  };
}

/// Typed formatting and linting options for Rust.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct RustOptions {
  /// Rust edition (e.g. `"2021"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub edition: Option<String>,
}

impl_options_methods!(RustOptions, edition);

/// Typed formatting and linting options for Python.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct PythonOptions {
  /// Quote style for strings (`"single"` or `"double"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub quote_style: Option<String>,
  /// Python target version (e.g. `"py310"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target_version: Option<String>,
}

impl_options_methods!(PythonOptions, quote_style, target_version);

/// Typed formatting and linting options for C/C++.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct CppOptions {
  /// C++ language standard version (e.g. `"c++20"`).
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "Standard",
    alias = "std"
  )]
  pub standard: Option<String>,
  /// Column limit for line wrapping.
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "ColumnLimit",
    alias = "column-limit"
  )]
  pub column_limit: Option<usize>,
  /// Base clang-format style (e.g. `"Google"`).
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "BasedOnStyle",
    alias = "based-on-style"
  )]
  pub based_on_style: Option<String>,
  /// Pointer alignment style (`"Left"`, `"Right"`, or `"Middle"`).
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "PointerAlignment",
    alias = "pointer-alignment"
  )]
  pub pointer_alignment: Option<String>,
  /// Brace breaking style.
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "BreakBeforeBraces",
    alias = "break-before-braces"
  )]
  pub break_before_braces: Option<String>,
  /// Whether to sort `#include` directives alphabetically.
  #[serde(
    skip_serializing_if = "Option::is_none",
    alias = "SortIncludes",
    alias = "sort-includes"
  )]
  pub sort_includes: Option<bool>,
}

impl_options_methods!(
  CppOptions,
  standard,
  column_limit,
  based_on_style,
  pointer_alignment,
  break_before_braces,
  sort_includes,
);

/// Typed formatting and linting options for Java.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct JavaOptions {
  /// Formatting style used by google-java-format: "google" (default,
  /// 2-space indent) or "aosp" (4-space indent).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub style: Option<String>,
}

impl_options_methods!(JavaOptions, style);

/// Typed formatting and linting options for JavaScript/TypeScript (Biome).
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct JavaScriptOptions {
  /// Preferred string quote style (`"single"` or `"double"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub quote_style: Option<String>,
  /// Trailing comma policy (`"all"`, `"es5"`, or `"none"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trailing_comma: Option<String>,
  /// Semicolon policy (`"always"` or `"as-needed"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub semicolons: Option<String>,
  /// Whether to automatically organize import statements.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub organize_imports: Option<bool>,
}

impl_options_methods!(
  JavaScriptOptions,
  quote_style,
  trailing_comma,
  semicolons,
  organize_imports,
);

/// Typed formatting and linting options for Go.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct GoOptions {
  /// Prefix(es) passed to `goimports -local` so first-party imports are
  /// grouped separately from third-party ones (e.g. "example.com/myorg").
  #[serde(skip_serializing_if = "Option::is_none")]
  pub local_prefixes: Option<String>,
  /// Linters to enable in the generated `.golangci.yml`. Defaults to
  /// golangci-lint's own well-known default set when unset.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub linters: Option<Vec<String>>,
}

impl_options_methods!(GoOptions, local_prefixes, linters);

/// Typed formatting and linting options for Markdown.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct MarkdownOptions {
  /// Prose wrapping strategy string (`"always"`, `"never"`, or `"preserve"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
}

impl_options_methods!(MarkdownOptions, prose_wrap);

/// Typed formatting and linting options for YAML.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct YamlOptions {
  /// Whether to indent sequence items under mapping keys.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_sequence: Option<bool>,
  /// Whether to require `---` document start markers.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub document_start: Option<bool>,
  /// Whether to enforce strict boolean truthy values.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub truthy: Option<bool>,
}

impl_options_methods!(YamlOptions, indent_sequence, document_start, truthy);

/// Typed formatting and linting options for JSON.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct JsonOptions {}

impl_options_methods!(JsonOptions);

/// Typed formatting and linting options for TOML.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct TomlOptions {
  /// Whether to align entries across lines.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub align_entries: Option<bool>,
  /// Whether to indent table entry keys.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_entries: Option<bool>,
  /// Whether to indent table contents.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_tables: Option<bool>,
}

impl_options_methods!(
  TomlOptions,
  align_entries,
  indent_entries,
  indent_tables
);

/// Typed formatting and linting options for Typst.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct TypstOptions {}

impl_options_methods!(TypstOptions);

/// Typed formatting and linting options for Kotlin.
///
/// Kotlin's tooling (ktlint) reads layout facets (indent size, line length,
/// etc.) exclusively from `.editorconfig` rather than a dedicated config
/// file, so this struct is intentionally empty for now — it exists to keep
/// Kotlin consistent with the rest of the fleet's per-language options
/// wiring and as a home for future knobs (e.g. `ktlint_code_style`).
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct KotlinOptions {}

impl_options_methods!(KotlinOptions);

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_options_merge_and_is_empty() {
    // 0 fields (JsonOptions, TypstOptions, KotlinOptions)
    let mut json = JsonOptions::default();
    assert!(json.is_empty());
    json.merge(JsonOptions::default());
    assert!(json.is_empty());

    let mut typst = TypstOptions::default();
    assert!(typst.is_empty());
    typst.merge(TypstOptions::default());
    assert!(typst.is_empty());

    let mut kotlin = KotlinOptions::default();
    assert!(kotlin.is_empty());
    kotlin.merge(KotlinOptions::default());
    assert!(kotlin.is_empty());

    // 1 field (RustOptions, JavaOptions, MarkdownOptions)
    let mut rust = RustOptions::default();
    assert!(rust.is_empty());
    rust.merge(RustOptions {
      edition: Some("2021".to_string()),
    });
    assert!(!rust.is_empty());
    assert_eq!(rust.edition.as_deref(), Some("2021"));
    rust.merge(RustOptions::default());
    assert_eq!(rust.edition.as_deref(), Some("2021"));

    let mut java = JavaOptions::default();
    assert!(java.is_empty());
    java.merge(JavaOptions {
      style: Some("aosp".to_string()),
    });
    assert!(!java.is_empty());
    assert_eq!(java.style.as_deref(), Some("aosp"));

    let mut md = MarkdownOptions::default();
    assert!(md.is_empty());
    md.merge(MarkdownOptions {
      prose_wrap: Some("always".to_string()),
    });
    assert!(!md.is_empty());
    assert_eq!(md.prose_wrap.as_deref(), Some("always"));

    // 2 fields (PythonOptions, GoOptions)
    let mut py = PythonOptions::default();
    assert!(py.is_empty());
    py.merge(PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: None,
    });
    assert!(!py.is_empty());
    assert_eq!(py.quote_style.as_deref(), Some("single"));
    assert_eq!(py.target_version, None);
    py.merge(PythonOptions {
      quote_style: None,
      target_version: Some("py311".to_string()),
    });
    assert_eq!(py.quote_style.as_deref(), Some("single"));
    assert_eq!(py.target_version.as_deref(), Some("py311"));

    let mut go = GoOptions::default();
    assert!(go.is_empty());
    go.merge(GoOptions {
      local_prefixes: Some("example.com".to_string()),
      linters: None,
    });
    assert!(!go.is_empty());
    assert_eq!(go.local_prefixes.as_deref(), Some("example.com"));
    assert_eq!(go.linters, None);
    go.merge(GoOptions {
      local_prefixes: None,
      linters: Some(vec!["errcheck".to_string()]),
    });
    assert_eq!(go.local_prefixes.as_deref(), Some("example.com"));
    assert_eq!(go.linters.as_deref(), Some(&["errcheck".to_string()][..]));

    // Multi-field (CppOptions, JavaScriptOptions, YamlOptions, TomlOptions)
    let mut cpp = CppOptions::default();
    assert!(cpp.is_empty());
    cpp.merge(CppOptions {
      standard: Some("c++20".to_string()),
      column_limit: Some(100),
      ..Default::default()
    });
    assert!(!cpp.is_empty());
    assert_eq!(cpp.standard.as_deref(), Some("c++20"));
    assert_eq!(cpp.column_limit, Some(100));

    let mut js = JavaScriptOptions::default();
    assert!(js.is_empty());
    js.merge(JavaScriptOptions {
      quote_style: Some("double".to_string()),
      trailing_comma: Some("all".to_string()),
      semicolons: Some("always".to_string()),
      organize_imports: Some(true),
    });
    assert!(!js.is_empty());
    assert_eq!(js.quote_style.as_deref(), Some("double"));
    assert_eq!(js.trailing_comma.as_deref(), Some("all"));
    assert_eq!(js.semicolons.as_deref(), Some("always"));
    assert_eq!(js.organize_imports, Some(true));

    let mut yaml = YamlOptions::default();
    assert!(yaml.is_empty());
    yaml.merge(YamlOptions {
      indent_sequence: Some(true),
      document_start: None,
      truthy: Some(false),
    });
    assert!(!yaml.is_empty());
    assert_eq!(yaml.indent_sequence, Some(true));
    assert_eq!(yaml.document_start, None);
    assert_eq!(yaml.truthy, Some(false));

    let mut toml = TomlOptions::default();
    assert!(toml.is_empty());
    toml.merge(TomlOptions {
      align_entries: Some(true),
      indent_entries: None,
      indent_tables: Some(false),
    });
    assert!(!toml.is_empty());
    assert_eq!(toml.align_entries, Some(true));
    assert_eq!(toml.indent_entries, None);
    assert_eq!(toml.indent_tables, Some(false));
    toml.merge(TomlOptions {
      align_entries: None,
      indent_entries: Some(true),
      indent_tables: None,
    });
    assert_eq!(toml.align_entries, Some(true));
    assert_eq!(toml.indent_entries, Some(true));
    assert_eq!(toml.indent_tables, Some(false));
  }
}
