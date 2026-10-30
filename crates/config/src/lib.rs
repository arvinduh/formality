//! Reads fml's layered `formality.toml` config.
//!
//! Owns parsing the layers it is given, merging them in order, and reading one
//! section as the caller's type. It reads no files and knows no languages: the
//! binary finds and reads the layers, and each language reads its own section.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::path;

use serde;
use thiserror;
use toml;

/// The config layers fml was given, lowest priority first.
pub struct Config {
  /// Each layer's file and text, kept so errors can quote the line.
  layers: Vec<Layer>,
}

/// One config file: where it came from and what it says.
struct Layer {
  /// The file the text was read from, named in errors.
  path: path::PathBuf,
  /// The file's TOML text.
  text: String,
}

/// A config file that is not valid TOML, or a section with a key the reader
/// does not accept or a value of the wrong type.
///
/// Its message names the file, then the line and the offending text.
#[derive(Debug, thiserror::Error)]
#[error("{}: {source}", path.display())]
pub struct Error {
  /// The file the problem is in.
  path: path::PathBuf,
  /// The parse or type error, with the file's text attached for its line.
  source: toml::de::Error,
}

impl Config {
  /// Parses `layers`, each a file's path and text, lowest priority first.
  ///
  /// # Errors
  ///
  /// Returns an error for the first layer that is not valid TOML.
  pub fn new(layers: Vec<(path::PathBuf, String)>) -> Result<Self, Error> {
    let layers: Vec<Layer> = layers
      .into_iter()
      .map(|(path, text)| Layer { path, text })
      .collect();
    for layer in &layers {
      layer.table()?;
    }
    Ok(Self { layers })
  }

  /// Reads section `name` as `T`, a later layer's key replacing an earlier
  /// one's. With no layer setting the section, it is `T::default()`.
  ///
  /// `T` sees each layer's section on its own first, so an unknown key or a
  /// wrong type is reported in the file and on the line that holds it.
  ///
  /// # Errors
  ///
  /// Returns an error when a layer's section has a key `T` rejects or a value
  /// of the wrong type.
  pub fn section<T: serde::de::DeserializeOwned + Default>(
    &self,
    name: &str,
  ) -> Result<T, Error> {
    let mut merged = toml::Table::new();
    let mut last = None;
    for layer in &self.layers {
      let Some(section) = layer.table()?.remove(name) else {
        continue;
      };
      layer.check::<T>(name)?;
      if let toml::Value::Table(section) = section {
        merged.extend(section);
      }
      last = Some(layer);
    }
    let Some(layer) = last else {
      return Ok(T::default());
    };
    let merged = toml::Value::Table(merged);
    merged.try_into().map_err(|source| layer.error(source))
  }
}

impl Layer {
  /// Parses the whole layer as a plain table.
  fn table(&self) -> Result<toml::Table, Error> {
    self.text.parse().map_err(|source| self.error(source))
  }

  /// Reads this layer's section `name` on its own as `T`, so an error carries
  /// the line it is on.
  fn check<T: serde::de::DeserializeOwned>(
    &self,
    name: &str,
  ) -> Result<(), Error> {
    let table = toml::de::DeTable::parse(&self.text)
      .map_err(|source| self.error(source))?;
    let section = table
      .into_inner()
      .into_iter()
      .find_map(|(key, value)| (key.get_ref() == name).then_some(value));
    let Some(section) = section else {
      return Ok(());
    };
    T::deserialize(toml::de::ValueDeserializer::from(section))
      .map(drop)
      .map_err(|source| self.error(source))
  }

  /// Attaches this layer's path and text to `source`.
  fn error(&self, mut source: toml::de::Error) -> Error {
    source.set_input(Some(&self.text));
    Error {
      path: self.path.clone(),
      source,
    }
  }
}

/// The settings every tool with the matching knob shares, from `[global]`.
///
/// A setting no layer gives takes fml's default: 2-space indent, 80 columns,
/// double quotes and `\n` line endings.
#[derive(serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Global {
  /// Spaces per indent level.
  pub indent_size: u8,
  /// The column a formatter wraps at.
  pub line_length: u16,
  /// The quote style for languages that have a choice.
  pub quote_style: Quote,
  /// The line ending formatters write.
  pub end_of_line: EndOfLine,
}

impl Default for Global {
  fn default() -> Self {
    Self {
      indent_size: 2,
      line_length: 80,
      quote_style: Quote::Double,
      end_of_line: EndOfLine::Lf,
    }
  }
}

/// A string quote style.
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quote {
  /// `'text'`.
  Single,
  /// `"text"`.
  Double,
}

/// A line ending.
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EndOfLine {
  /// `\n`.
  Lf,
  /// `\r\n`.
  Crlf,
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Builds a config from `(path, text)` layers, lowest priority first.
  fn config(layers: &[(&str, &str)]) -> Result<Config, Error> {
    let layers = layers
      .iter()
      .map(|(path, text)| (path::PathBuf::from(path), (*text).to_owned()))
      .collect();
    Config::new(layers)
  }

  #[test]
  fn a_missing_section_takes_the_defaults() {
    let config = config(&[("formality.toml", "")]).expect("config");
    let global: Global = config.section("global").expect("global");
    assert_eq!(global.indent_size, 2, "default indent");
    assert_eq!(global.line_length, 80, "default width");
    assert!(
      matches!(global.quote_style, Quote::Double),
      "default quotes"
    );
  }

  #[test]
  fn later_layers_replace_earlier_keys() {
    let config = config(&[
      (
        "user.toml",
        "[global]\nindent_size = 4\nline_length = 100\n",
      ),
      ("formality.toml", "[global]\nindent_size = 3\n"),
    ])
    .expect("config");
    let global: Global = config.section("global").expect("global");
    assert_eq!(global.indent_size, 3, "project layer wins");
    assert_eq!(global.line_length, 100, "user layer still applies");
  }

  #[test]
  fn an_unknown_key_names_its_file_and_line() {
    let text = "[global]\nindent_size = 2\ntab_width = 4\n";
    let config = config(&[("formality.toml", text)]).expect("config");
    let err = config.section::<Global>("global").err().expect("rejected");
    let message = err.to_string();
    assert!(message.starts_with("formality.toml: "), "{message}");
    assert!(message.contains("line 3"), "{message}");
    assert!(message.contains("tab_width"), "{message}");
  }

  #[test]
  fn invalid_toml_fails_when_loaded() {
    assert!(
      config(&[("formality.toml", "[global\n")]).is_err(),
      "bad toml"
    );
  }
}
