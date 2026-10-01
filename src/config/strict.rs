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

use super::{ConfigError, FormalityConfig};

/// Parses `content`, read from `path`, into a [`FormalityConfig`].
///
/// # Errors
///
/// Returns [`ConfigError::InvalidValue`] for a value of the wrong type, or
/// [`ConfigError::Parse`] for invalid TOML.
pub fn parse(
  content: &str,
  path: &Path,
) -> Result<FormalityConfig, ConfigError> {
  let doc = DeTable::parse(content).map_err(|source| ConfigError::Parse {
    path: path.to_path_buf(),
    source,
  })?;
  FormalityConfig::deserialize(toml::de::Deserializer::from(doc.clone()))
    .map_err(|source| locate(path, content, doc.get_ref(), source))
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
      let line = content
        .bytes()
        .take(span.start)
        .filter(|&b| b == b'\n')
        .count()
        + 1;
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
