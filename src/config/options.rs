use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// Typed formatting and linting options for JavaScript/TypeScript (Biome).
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct JavaScriptOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub quote_style: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trailing_comma: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub semicolons: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub organize_imports: Option<bool>,
}

impl JavaScriptOptions {
  pub fn merge(&mut self, other: JavaScriptOptions) {
    if other.quote_style.is_some() {
      self.quote_style = other.quote_style;
    }
    if other.trailing_comma.is_some() {
      self.trailing_comma = other.trailing_comma;
    }
    if other.semicolons.is_some() {
      self.semicolons = other.semicolons;
    }
    if other.organize_imports.is_some() {
      self.organize_imports = other.organize_imports;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.quote_style.is_none()
      && self.trailing_comma.is_none()
      && self.semicolons.is_none()
      && self.organize_imports.is_none()
  }
}

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

impl GoOptions {
  pub fn merge(&mut self, other: GoOptions) {
    if other.local_prefixes.is_some() {
      self.local_prefixes = other.local_prefixes;
    }
    if other.linters.is_some() {
      self.linters = other.linters;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.local_prefixes.is_none() && self.linters.is_none()
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

impl KotlinOptions {
  pub fn merge(&mut self, _other: KotlinOptions) {}

  pub fn is_empty(&self) -> bool {
    true
  }
}
