use std::fs;
use std::path::PathBuf;

/// Ensures `Cargo.toml`'s package version stays in lockstep with the
/// VS Code extension's `package.json` version. The binary and the
/// extension ship together under a single `v{semver}` tag rather than on
/// independent cadences (see `docs/adr/0003-two-tag-release-versioning.md`),
/// so the two files must never drift — this test only asserts they agree
/// with each other, not any particular value.
#[test]
fn test_cargo_and_vscode_extension_versions_match() {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

  let cargo_toml_path = root.join("Cargo.toml");
  let cargo_toml =
    fs::read_to_string(&cargo_toml_path).expect("Failed to read Cargo.toml");
  let cargo_val: toml::Value =
    toml::from_str(&cargo_toml).expect("Cargo.toml should be valid TOML");
  let cargo_version = cargo_val
    .get("package")
    .and_then(|p| p.get("version"))
    .and_then(|v| v.as_str())
    .expect("Cargo.toml must have a [package].version string");

  let vscode_package_json_path =
    root.join("editors").join("vscode").join("package.json");
  assert!(
    vscode_package_json_path.exists(),
    "editors/vscode/package.json not found; if the VS Code extension has \
     been moved or removed, update this test's path accordingly."
  );
  let vscode_package_json = fs::read_to_string(&vscode_package_json_path)
    .expect("Failed to read editors/vscode/package.json");
  let vscode_val: serde_json::Value =
    serde_json::from_str(&vscode_package_json)
      .expect("editors/vscode/package.json should be valid JSON");
  let vscode_version =
    vscode_val.get("version").and_then(|v| v.as_str()).expect(
      "editors/vscode/package.json must have a top-level \"version\" string",
    );

  assert_eq!(
    cargo_version, vscode_version,
    "Version drift detected! Cargo.toml version ({cargo_version}) does not match \
     editors/vscode/package.json version ({vscode_version}). Keep these in lockstep."
  );
}
