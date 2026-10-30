//! The workspace a run acts on: its root, its config layers and its ignore
//! patterns.
//!
//! Owns reading `formality.toml`, the user's `config.toml` and the root's
//! `.gitignore` from disk, so the libraries below see only their contents.

use std::env;
use std::fs;
use std::io;
use std::path;

use config;

use crate::cli;

/// The names of a workspace's own config file. Having both is an error.
const PROJECT: [&str; 2] = ["formality.toml", ".formality.toml"];

/// A workspace and its merged config.
pub struct Workspace {
  /// The absolute root.
  pub root: path::PathBuf,
  /// Every config layer, later winning.
  pub config: config::Config,
  /// Whether the workspace has its own config, or one was passed with
  /// `--config`, so fml's settings override the tools' own configs.
  pub project: bool,
  /// The user's config directory, such as `~/.config`.
  pub user_config: Option<path::PathBuf>,
  /// The gitignore patterns of the root's `.gitignore`.
  pub ignores: Vec<String>,
}

/// Finds the workspace around `cwd` and loads its config, with `extra` as
/// the last layer.
///
/// # Errors
///
/// Returns an error when a config file cannot be read or is invalid, or the
/// root holds both project config names.
pub fn load(
  cwd: &path::Path,
  extra: Option<&path::Path>,
) -> Result<Workspace, cli::Error> {
  let root = root(cwd);
  let user_config = user_config();
  let mut layers = Vec::new();
  if let Some(dir) = &user_config {
    let user = dir.join("formality").join("config.toml");
    layers.extend(read_optional(&user)?.map(|text| (user, text)));
  }
  let mut project = None;
  for name in PROJECT {
    let path = root.join(name);
    if let Some(text) = read_optional(&path)? {
      if project.is_some() {
        return Err(cli::Error::BothConfigs { root });
      }
      project = Some((path, text));
    }
  }
  let has_project = project.is_some() || extra.is_some();
  layers.extend(project);
  if let Some(path) = extra {
    let text = fs::read_to_string(path).map_err(|source| cli::Error::Read {
      path: path.to_path_buf(),
      source,
    })?;
    layers.push((path.to_path_buf(), text));
  }
  let ignores = read_optional(&root.join(".gitignore"))?
    .map(|text| patterns(&text))
    .unwrap_or_default();
  Ok(Workspace {
    config: config::Config::new(layers)?,
    root,
    project: has_project,
    user_config,
    ignores,
  })
}

/// The nearest directory at or above `cwd` holding a project config or
/// `.git`, else `cwd` itself.
fn root(cwd: &path::Path) -> path::PathBuf {
  cwd
    .ancestors()
    .find(|dir| {
      PROJECT
        .iter()
        .chain([".git"].iter())
        .any(|name| dir.join(name).exists())
    })
    .unwrap_or(cwd)
    .to_path_buf()
}

/// The user's config directory: `$XDG_CONFIG_HOME`, else `.config` in the
/// home directory.
fn user_config() -> Option<path::PathBuf> {
  if let Some(dir) = env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
    return Some(dir.into());
  }
  env::var_os("HOME")
    .or_else(|| env::var_os("USERPROFILE"))
    .map(|home| path::PathBuf::from(home).join(".config"))
}

/// Reads `path`, or `None` when it does not exist.
fn read_optional(path: &path::Path) -> Result<Option<String>, cli::Error> {
  match fs::read_to_string(path) {
    Ok(text) => Ok(Some(text)),
    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
    Err(source) => Err(cli::Error::Read {
      path: path.to_path_buf(),
      source,
    }),
  }
}

/// The patterns in a `.gitignore`: its lines without blanks and comments.
fn patterns(text: &str) -> Vec<String> {
  text
    .lines()
    .map(str::trim_end)
    .filter(|line| !line.is_empty() && !line.starts_with('#'))
    .map(str::to_owned)
    .collect()
}
