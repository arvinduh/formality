//! Guards the install paths dist bakes into the release installers.
//!
//! Only `[workspace.metadata.dist] install-path` in `Cargo.toml` is checked;
//! the Windows leg of `install-regression.yml` runs the generated installer.

use std::fs;
use std::path;

/// Asserts no `install-path` entry names an environment variable. dist turns
/// `$VAR/...` into `$env:VAR` in `fml-installer.ps1`, and Windows sets no
/// `HOME`, so a `$HOME/` entry left the installer with nowhere to install
/// (#548). `~/` resolves to the user's home on every OS.
#[test]
fn dist_install_paths_name_no_environment_variable() {
  let cargo_toml = fs::read_to_string(
    path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
  )
  .expect("Failed to read Cargo.toml");
  let manifest: toml::Value =
    toml::from_str(&cargo_toml).expect("Cargo.toml should be valid TOML");
  let paths = manifest
    .get("workspace")
    .and_then(|w| w.get("metadata"))
    .and_then(|m| m.get("dist"))
    .and_then(|d| d.get("install-path"))
    .and_then(toml::Value::as_array)
    .expect("Cargo.toml must set [workspace.metadata.dist] install-path");

  let env_paths: Vec<&str> = paths
    .iter()
    .filter_map(toml::Value::as_str)
    .filter(|p| p.starts_with('$'))
    .collect();
  assert!(
    env_paths.is_empty(),
    "install-path entries {env_paths:?} read an environment variable, which \
     the PowerShell installer looks up as $env:VAR; spell the home dir `~/`."
  );
}
