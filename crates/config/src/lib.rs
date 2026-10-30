//! Reads fml's layered `formality.toml` config.
//!
//! Owns parsing the layers it is given, merging them in order, and reading one
//! section as the caller's type. It reads no files and knows no languages: the
//! binary finds and reads the layers, and each language reads its own section.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::path;

use thiserror;
use toml;

/// The config layers fml was given, lowest priority first.
pub struct Config {
  /// Each layer's file and text, kept so errors can quote the line.
  #[expect(dead_code, reason = "`section` reads it in the next commit")]
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
}

impl Layer {
  /// Parses the whole layer as a plain table.
  fn table(&self) -> Result<toml::Table, Error> {
    self.text.parse().map_err(|source| self.error(source))
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
