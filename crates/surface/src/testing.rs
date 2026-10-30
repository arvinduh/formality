//! Helpers the language tests share.

use std::fs;
use std::path;

use config;
use tempfile;

use crate::Context;

/// Writes `files` (name and text) into a new temp dir and returns it.
pub fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
  let dir = tempfile::tempdir().expect("tempdir");
  for (name, text) in files {
    fs::write(dir.path().join(name), text).expect("write");
  }
  dir
}

/// Runs `pass` against a context rooted at `root` with no fml config.
pub fn with_context<T>(root: &path::Path, pass: impl Fn(&Context) -> T) -> T {
  let config = config::Config::new(Vec::new()).expect("config");
  pass(&Context {
    root,
    config: &config,
    project: false,
    user_config: None,
  })
}
