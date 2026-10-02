//! Strict parsing of one config document into a [`FormalityConfig`],
//! turning a rejected key or value into a [`ConfigError`] that names its key
//! path and line.
//!
//! The typed structs in `super` decide what a config may contain; this module
//! only locates what they rejected. Reading files and layering configs stay in
//! `resolve`.

use serde::Deserialize;
use std::path::Path;
use toml::de::{DeTable, DeValue};

use super::lang_table::lang_options_table;
use super::options::MarkdownOptions;
use super::{ConfigError, FormalityConfig};
use crate::surfaces::SurfaceRegistry;

/// Parses `content`, read from `path`, into a [`FormalityConfig`].
///
/// # Errors
///
/// Returns [`ConfigError::UnknownKey`] for a key this `fml` does not accept,
/// [`ConfigError::InvalidValue`] for a value of the wrong type,
/// [`ConfigError::NonCanonicalLang`] for a `[lang.<name>]` section spelled
/// as an alias or case variant, or [`ConfigError::Parse`] for invalid TOML.
pub fn parse(
  content: &str,
  path: &Path,
) -> Result<FormalityConfig, ConfigError> {
  let doc = DeTable::parse(content).map_err(|source| ConfigError::Parse {
    path: path.to_path_buf(),
    source,
  })?;
  check_lang_names(content, path, doc.get_ref())?;
  check_extra_args(content, path, doc.get_ref())?;
  FormalityConfig::deserialize(toml::de::Deserializer::from(doc.clone()))
    .and_then(|config| {
      check_lang_options(&config, doc.get_ref()).map(|()| config)
    })
    .map_err(|source| locate(path, content, doc.get_ref(), source))
}

/// Rejects a `[lang.<name>]` section whose name resolves to a surface only
/// as an alias or case variant: every reader looks sections up by exact
/// canonical name, so its keys would be silently ignored. A name that
/// resolves to no surface is left to the unrecognized-section warning.
fn check_lang_names(
  content: &str,
  path: &Path,
  doc: &DeTable<'_>,
) -> Result<(), ConfigError> {
  let Some(DeValue::Table(sections)) =
    doc.get("lang").map(toml::Spanned::get_ref)
  else {
    return Ok(());
  };
  let registry = SurfaceRegistry::default();
  for key in sections.keys() {
    let name: &str = key.get_ref();
    match registry.resolve_canonical_name(name) {
      Some(canonical) if canonical != name => {
        return Err(ConfigError::NonCanonicalLang {
          path: path.to_path_buf(),
          name: name.to_owned(),
          canonical,
          line: line_at(content, key.span().start),
        });
      }
      _ => {}
    }
  }
  Ok(())
}

/// Rejects a `[lang.<name>] extra_args` written as a flat list, and a key
/// in its table naming no tool the surface runs: either would otherwise
/// fail as a bare type mismatch or reach no invocation at all. A section
/// naming no surface has no tool list, so only its flat form is checked;
/// a value of any other type is left to deserialization.
fn check_extra_args(
  content: &str,
  path: &Path,
  doc: &DeTable<'_>,
) -> Result<(), ConfigError> {
  let Some(DeValue::Table(sections)) =
    doc.get("lang").map(toml::Spanned::get_ref)
  else {
    return Ok(());
  };
  let registry = SurfaceRegistry::default();
  for (name, section) in sections {
    let DeValue::Table(table) = section.get_ref() else {
      continue;
    };
    let Some((key, value)) = table.get_key_value("extra_args") else {
      continue;
    };
    let lang: &str = name.get_ref();
    let tools = registry
      .get_surface_by_name(lang)
      .map(|s| s.extra_args_tools());
    match value.get_ref() {
      DeValue::Array(_) => {
        return Err(ConfigError::FlatExtraArgs {
          path: path.to_path_buf(),
          lang: lang.to_owned(),
          line: line_at(content, key.span().start),
          tools: tools.unwrap_or_default(),
        });
      }
      DeValue::Table(by_tool) => {
        let Some(tools) = tools else { continue };
        if let Some(tool) = by_tool
          .keys()
          .find(|tool| !tools.contains(&tool.get_ref().as_ref()))
        {
          return Err(ConfigError::UnknownTool {
            path: path.to_path_buf(),
            lang: lang.to_owned(),
            tool: tool.get_ref().to_string(),
            line: line_at(content, tool.span().start),
            tools,
          });
        }
      }
      _ => {}
    }
  }
  Ok(())
}

/// Deserializes each `[lang.<name>]` section's surface-specific keys, the
/// flattened ones `LangConfig` collects into `extra` and its `options`
/// value, into that surface's typed options. The lenient accessors that
/// read them later drop what does not fit; this rejects it up front.
fn check_lang_options(
  config: &FormalityConfig,
  doc: &DeTable<'_>,
) -> Result<(), toml::de::Error> {
  let Some(DeValue::Table(sections)) =
    doc.get("lang").map(toml::Spanned::get_ref)
  else {
    return Ok(());
  };
  for (name, section) in sections {
    let (DeValue::Table(table), Some(lang)) =
      (section.get_ref(), config.lang.get(name.get_ref().as_ref()))
    else {
      continue;
    };
    let flat = table
      .iter()
      .filter(|(key, _)| lang.extra.contains_key(key.get_ref().as_ref()))
      .map(|(key, value)| (key.clone(), value.clone()))
      .collect();
    check_options(
      name.get_ref(),
      toml::Spanned::new(section.span(), DeValue::Table(flat)),
    )?;
    if let Some(options) = table.get("options") {
      check_options(name.get_ref(), options.clone())?;
    }
  }
  Ok(())
}

/// Expands to a `match` on a surface name that deserializes into that row's
/// typed options, plus `markdown`, which `lang_options_table!` leaves out.
macro_rules! check_by_name {
  ([$name:expr, $de:expr] $( $lang:ident { $ty:ty, $accessor:ident, $is_empty:expr } )*) => {
    match $name {
      $( stringify!($lang) => <$ty>::deserialize($de).map(drop), )*
      "markdown" => MarkdownOptions::deserialize($de).map(drop),
      _ => Ok(()),
    }
  };
}

/// Deserializes `value` into surface `name`'s typed options, so a value
/// that is not a table fails as a wrong type at its own span. A name with
/// none, or an unknown section, is not checked.
fn check_options(
  name: &str,
  value: toml::Spanned<DeValue<'_>>,
) -> Result<(), toml::de::Error> {
  let de = toml::de::ValueDeserializer::from(value);
  lang_options_table!(check_by_name, name, de)
}

/// Attributes a deserialization error to the key it occurred under, falling
/// back to [`ConfigError::Parse`] when its span matches no key.
fn locate(
  path: &Path,
  content: &str,
  doc: &DeTable<'_>,
  mut source: toml::de::Error,
) -> ConfigError {
  let mut key = Vec::new();
  match source.span() {
    Some(span) if key_path_at(doc, span.start, &mut key) => {
      let line = line_at(content, span.start);
      // serde's `de::Error::unknown_field` wording; the typed structs
      // reject extra keys with `deny_unknown_fields`.
      if source.message().starts_with("unknown field ") {
        return ConfigError::UnknownKey {
          path: path.to_path_buf(),
          key: key.join("."),
          line,
        };
      }
      ConfigError::InvalidValue {
        path: path.to_path_buf(),
        key: key.join("."),
        line,
        reason: source.message().to_owned(),
      }
    }
    _ => {
      source.set_input(Some(content));
      ConfigError::Parse {
        path: path.to_path_buf(),
        source,
      }
    }
  }
}

/// Returns the one-based line of byte `offset` in `content`.
fn line_at(content: &str, offset: usize) -> usize {
  content.bytes().take(offset).filter(|&b| b == b'\n').count() + 1
}

/// Pushes onto `path` the keys leading to the innermost entry whose key or
/// value spans byte `offset`, and returns whether one was found.
fn key_path_at(
  table: &DeTable<'_>,
  offset: usize,
  path: &mut Vec<String>,
) -> bool {
  for (key, value) in table {
    path.push(key.get_ref().to_string());
    let inner = match value.get_ref() {
      DeValue::Table(inner) => key_path_at(inner, offset, path),
      _ => false,
    };
    if inner || key.span().contains(&offset) || value.span().contains(&offset) {
      return true;
    }
    path.pop();
  }
  false
}
