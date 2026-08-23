//! X-macro table describing each language surface's typed-options wiring.
//!
//! Before this module existed, [`super::LangConfig`]'s per-language
//! typed-options plumbing was hand-maintained in lockstep across four
//! locations: `LangConfig::merge`'s `merge_option!` calls, the
//! `LangConfig::xxx_options()` accessor methods, `resolve_for_lang`'s
//! struct-literal assembly, and `default_tools_for_lang`'s match. Adding a
//! 13th language surface with typed options meant touching all four by
//! hand, in sync — easy to drift.
//!
//! [`lang_options_table!`] is now the single source of truth for those
//! four call sites. It doesn't generate code itself — it's an "X-macro":
//! it just hands its table rows (plus any call-site-specific arguments,
//! see below) to whichever `$callback` macro is passed in, and each
//! callback below emits the logic specific to one call site from the same
//! rows. Adding a language surface with typed options now means adding
//! one row here, plus hand-adding the two struct fields on
//! `LangConfig`/`ResolvedLangConfig` (deliberately kept hand-written, see
//! below) — nothing else.
//!
//! ## Table columns
//!
//! Each row has the shape `$lang { $ty, $accessor, $is_empty, $fmt, $lint }`:
//!
//! - `$lang` — the shared field name on `LangConfig`/`ResolvedLangConfig`
//!   and the `[lang.<name>]` TOML key. Also used, via `stringify!`, as the
//!   language-name string matched in `default_tools_for_lang` and
//!   `resolve_for_lang`.
//! - `$ty` — the per-language typed options struct (e.g. `RustOptions`),
//!   fully qualified so this table can be invoked from any module. Must be
//!   `Default`, have an inherent `merge(&mut self, Self)`, and implement
//!   `Deserialize`.
//! - `$accessor` — the generated `LangConfig::<accessor>()` method name.
//! - `$is_empty` — the emptiness check `extract_options` uses to decide
//!   whether a freshly-deserialized-but-all-`None` value should still be
//!   recorded. Most languages pass their real `Type::is_empty`; `json`,
//!   `toml`, `typst`, and `kotlin` have no meaningful fields today, so
//!   (preserved unchanged from the pre-macro code) they pass the
//!   `|_| false` sentinel instead of a real `is_empty` check.
//! - `$fmt` / `$lint` — default tool names as string literals, or the
//!   bare `NONE` sentinel (json has no default lint tool).
//!
//! ## Why callbacks take explicit `self`/`other`/`lang_cfg`/`lang_name` args
//!
//! `macro_rules!` is hygienic: an identifier like `self` or `lang_cfg`
//! written literally inside a *different* macro's definition does not
//! resolve to the caller's local variable of the same name. So rather
//! than the callback macros assuming those names are in scope, the call
//! sites below pass them in explicitly as macro arguments — ordinary
//! hygienic macro argument passing, no special tricks.
//!
//! ## Why `resolve_for_lang` and `default_tools_for_lang` generate a whole
//! expression/function instead of a fragment
//!
//! Rust's grammar does not allow a macro invocation to expand into *part*
//! of a struct-literal field list or *part* of a `match`'s arm list — a
//! macro used there must produce one complete field/arm each, not "the
//! rest of the fields" or "the rest of the arms" as a spliced-in
//! fragment. So `build_resolved_lang_config!` produces the entire
//! `ResolvedLangConfig { .. }` literal (fixed fields passed in, table rows
//! plus `markdown` handled inside), and `impl_default_tools_fn!` produces
//! the entire `default_tools_for_lang` function (table rows plus the
//! hand-written `markdown` arm and catch-all, all inside).
//!
//! ## The markdown exception
//!
//! `markdown` is deliberately **not** a row in this table. Its accessor
//! (`LangConfig::markdown_options`) has bespoke fallback logic — falling
//! back to the top-level `prose_wrap` / `layout.prose_wrap` settings when
//! no explicit `[lang.markdown]` options are set — that doesn't fit the
//! uniform per-language pattern the other eleven rows share. Rather than
//! bolt a one-off escape hatch onto the table for a single row, markdown
//! stays hand-written at all four call sites, right alongside the
//! macro-generated code for the rest.
macro_rules! lang_options_table {
  (@rows $callback:ident [$($arg:tt)*]) => {
    $callback! {
      [$($arg)*]
      rust       { crate::config::options::RustOptions,       rust_options,       crate::config::options::RustOptions::is_empty,       "cargo-fmt",          "clippy" }
      python     { crate::config::options::PythonOptions,     python_options,     crate::config::options::PythonOptions::is_empty,     "ruff-format",        "ruff-check" }
      cpp        { crate::config::options::CppOptions,        cpp_options,        crate::config::options::CppOptions::is_empty,        "clang-format",       "clang-tidy" }
      java       { crate::config::options::JavaOptions,       java_options,       crate::config::options::JavaOptions::is_empty,       "google-java-format", "checkstyle" }
      go         { crate::config::options::GoOptions,         go_options,         crate::config::options::GoOptions::is_empty,         "goimports",          "golangci-lint" }
      yaml       { crate::config::options::YamlOptions,       yaml_options,       crate::config::options::YamlOptions::is_empty,       "prettier",           "yamllint" }
      json       { crate::config::options::JsonOptions,       json_options,       |_: &crate::config::options::JsonOptions| false,     "prettier",           NONE }
      toml       { crate::config::options::TomlOptions,       toml_options,       |_: &crate::config::options::TomlOptions| false,     "taplo",              "taplo" }
      typst      { crate::config::options::TypstOptions,      typst_options,      |_: &crate::config::options::TypstOptions| false,    "typstyle",           "typstyle" }
      javascript { crate::config::options::JavaScriptOptions, javascript_options, crate::config::options::JavaScriptOptions::is_empty, "biome",              "biome" }
      kotlin     { crate::config::options::KotlinOptions,     kotlin_options,     crate::config::options::KotlinOptions::is_empty,     "ktlint",             "ktlint" }
    }
  };
  ($callback:ident) => {
    lang_options_table! {@rows $callback []}
  };
  ($callback:ident, $($arg:tt)*) => {
    lang_options_table! {@rows $callback [$($arg)*]}
  };
}

/// Resolves the `$fmt`/`$lint` table cell into `Option<&'static str>`: a
/// string literal becomes `Some(...)`, the bare `NONE` sentinel becomes
/// `None` (distinguishing "no default tool" from a row that forgot to
/// fill the cell in).
macro_rules! default_tool_opt {
  (NONE) => {
    None
  };
  ($tool:literal) => {
    Some($tool)
  };
}

/// Generates `LangConfig::merge`'s per-field merge-or-overwrite logic for
/// every table row. Takes `self`/`other` explicitly (see module docs on
/// hygiene) and is invoked as a statement inside `LangConfig::merge`'s
/// body.
macro_rules! impl_lang_merge {
  ([$self_:expr, $other:expr] $( $lang:ident { $ty:ty, $accessor:ident, $is_empty:expr, $fmt:tt, $lint:tt } )*) => {
    $(
      if let Some(other_val) = $other.$lang {
        if let Some(ref mut our_val) = $self_.$lang {
          our_val.merge(other_val);
        } else {
          $self_.$lang = Some(other_val);
        }
      }
    )*
  };
}

/// Generates the `LangConfig::xxx_options()` accessor methods for every
/// table row. Invoked as an item directly inside `impl LangConfig { .. }`.
macro_rules! impl_lang_accessors {
  ([] $( $lang:ident { $ty:ty, $accessor:ident, $is_empty:expr, $fmt:tt, $lint:tt } )*) => {
    $(
      /// Extracts resolved per-language typed options for this surface.
      ///
      /// Generated by `lang_options_table!` — see `src/config/lang_table.rs`.
      #[must_use]
      pub fn $accessor(&self) -> Option<$ty> {
        extract_options(
          self.$lang.clone(),
          self.options.as_ref(),
          &self.extra,
          <$ty>::merge,
          $is_empty,
        )
      }
    )*
  };
}

/// Generates the whole `ResolvedLangConfig { .. }` struct literal used by
/// `resolve_for_lang`: the fixed (non-table) fields are passed in
/// explicitly as macro arguments, `markdown` is spliced in from its own
/// hand-resolved value, and every table row contributes its
/// `and_then(..).or_else(..)` resolution inline as that field's value —
/// this is also where each row's fallback default (`<$ty>::default()`)
/// comes from, replacing the old per-field `resolve_opt!` local macro.
macro_rules! build_resolved_lang_config {
  ([
    $lang_cfg:expr, $lang_name:expr,
    $name:expr, $format_tool:expr, $lint_tool:expr,
    $indent_size:expr, $line_length:expr, $use_tabs:expr, $prose_wrap:expr,
    $layout:expr, $enabled:expr, $extra_args:expr, $files:expr, $exclude:expr,
    $markdown:expr, $extra:expr
  ] $( $lang:ident { $ty:ty, $accessor:ident, $is_empty:expr, $fmt:tt, $lint:tt } )*) => {
    ResolvedLangConfig {
      name: $name,
      format_tool: $format_tool,
      lint_tool: $lint_tool,
      indent_size: $indent_size,
      line_length: $line_length,
      use_tabs: $use_tabs,
      prose_wrap: $prose_wrap,
      layout: $layout,
      enabled: $enabled,
      extra_args: $extra_args,
      files: $files,
      exclude: $exclude,
      $(
        $lang: $lang_cfg
          .and_then(crate::config::LangConfig::$accessor)
          .or_else(|| {
            if $lang_name == stringify!($lang) {
              Some(<$ty>::default())
            } else {
              None
            }
          }),
      )*
      markdown: $markdown,
      extra: $extra,
    }
  };
}

/// Generates the entire `default_tools_for_lang` function: every table
/// row's match arm plus the hand-written `markdown` arm and catch-all,
/// all produced in one go since Rust doesn't allow a macro to expand into
/// only *part* of a `match`'s arm list.
macro_rules! impl_default_tools_fn {
  ([] $( $lang:ident { $ty:ty, $accessor:ident, $is_empty:expr, $fmt:tt, $lint:tt } )*) => {
    fn default_tools_for_lang(
      lang_name: &str,
    ) -> (Option<&'static str>, Option<&'static str>) {
      match lang_name {
        $(
          stringify!($lang) => (default_tool_opt!($fmt), default_tool_opt!($lint)),
        )*
        // markdown is excluded from `lang_options_table!` (see this
        // module's docs), so it stays a hand-written arm.
        "markdown" => (Some("prettier"), Some("markdownlint")),
        _ => (None, None),
      }
    }
  };
}

pub(crate) use {
  build_resolved_lang_config, default_tool_opt, impl_default_tools_fn,
  impl_lang_accessors, impl_lang_merge, lang_options_table,
};
