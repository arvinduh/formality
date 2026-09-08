//! Asserts that `channel` in `rust-toolchain.toml` matches every
//! `dtolnay/rust-toolchain@<version>` pin across `.github/workflows/*.yml`.
//!
//! The Rust toolchain version is pinned for local development in
//! `rust-toolchain.toml` (which rustup reads automatically) and in CI
//! workflows via `dtolnay/rust-toolchain` (which does not read
//! `rust-toolchain.toml`). When bumping the toolchain, both must be updated
//! together (see issues #181 and #189). If they drift, contributors compile
//! with one toolchain locally while CI builds and lints with another — and
//! because `cargo clippy --all-targets -- -D warnings` is a required status
//! check, this can produce failures in CI that cannot be reproduced locally.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct ToolchainPin {
  path: String,
  line_number: usize,
  version: String,
}

const NEEDLE: &str = "dtolnay/rust-toolchain@";

fn extract_pins(content: &str, file_path: &str) -> Vec<ToolchainPin> {
  let mut pins = Vec::new();

  for (line_idx, line) in content.lines().enumerate() {
    let trimmed = line.trim_start();
    // Skip whole-line comments
    if trimmed.starts_with('#') {
      continue;
    }

    if let Some(pos) = line.find(NEEDLE) {
      // Skip if the action reference occurs inside an inline comment
      if let Some(comment_pos) = line.find('#')
        && comment_pos < pos
      {
        continue;
      }

      let after = &line[pos + NEEDLE.len()..];
      let version = after
        .trim_start_matches(['\'', '"'])
        .split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '#'))
        .next()
        .unwrap_or("")
        .trim();

      assert!(
        !version.is_empty(),
        "{}:{}: found `{}` but could not extract version pin",
        file_path,
        line_idx + 1,
        NEEDLE
      );

      pins.push(ToolchainPin {
        path: file_path.to_string(),
        line_number: line_idx + 1,
        version: version.to_string(),
      });
    }
  }

  pins
}

#[test]
fn test_rust_toolchain_channel_matches_workflow_pins() {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

  let toolchain_toml_path = root.join("rust-toolchain.toml");
  assert!(
    toolchain_toml_path.exists(),
    "rust-toolchain.toml not found at {}",
    toolchain_toml_path.display()
  );
  let toolchain_toml = fs::read_to_string(&toolchain_toml_path)
    .expect("Failed to read rust-toolchain.toml");
  let toolchain_val: toml::Value = toml::from_str(&toolchain_toml)
    .expect("rust-toolchain.toml should be valid TOML");
  let expected_channel = toolchain_val
    .get("toolchain")
    .and_then(|t| t.get("channel"))
    .and_then(|c| c.as_str())
    .expect("rust-toolchain.toml must have a [toolchain].channel string");

  let workflows_dir = root.join(".github").join("workflows");
  assert!(
    workflows_dir.exists(),
    ".github/workflows directory not found at {}",
    workflows_dir.display()
  );

  let mut workflow_paths: Vec<PathBuf> = fs::read_dir(&workflows_dir)
    .expect("Failed to read .github/workflows directory")
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|p| {
      p.is_file()
        && p
          .extension()
          .and_then(|ext| ext.to_str())
          .is_some_and(|ext| ext == "yml" || ext == "yaml")
    })
    .collect();
  workflow_paths.sort();

  let mut pins = Vec::new();

  for path in &workflow_paths {
    let content = fs::read_to_string(path)
      .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));

    let rel_path = path
      .strip_prefix(&root)
      .unwrap_or(Path::new(path))
      .to_string_lossy()
      .replace('\\', "/");

    pins.extend(extract_pins(&content, &rel_path));
  }

  assert!(
    !pins.is_empty(),
    "Expected to find at least one `{NEEDLE}<version>` pin in .github/workflows/*.yml, but found none."
  );

  let mismatches: Vec<String> = pins
    .iter()
    .filter(|pin| pin.version != expected_channel)
    .map(|pin| {
      format!(
        "{}:{}: pinned to `{}`, expected `{}` (from rust-toolchain.toml)",
        pin.path, pin.line_number, pin.version, expected_channel
      )
    })
    .collect();

  assert!(
    mismatches.is_empty(),
    "\n\nRust toolchain pin drift detected! rust-toolchain.toml channel is `{expected_channel}`, \
     but the following workflow pin(s) do not match:\n\n  {}\n\n\
     Keep rust-toolchain.toml and all workflow toolchain pins in lockstep (see issues #181 and #189).\n",
    mismatches.join("\n  ")
  );
}

#[test]
fn test_extract_pins_handles_various_formats() {
  let yaml = r#"
    # Comment line: uses: dtolnay/rust-toolchain@1.90.0
    - name: Step 1
      uses: dtolnay/rust-toolchain@1.98.1
    - name: Step 2
      uses: 'dtolnay/rust-toolchain@1.98.1'
    - name: Step 3
      uses: "dtolnay/rust-toolchain@1.98.1" # with comment
    - name: Step 4 # uses: dtolnay/rust-toolchain@1.90.0
      run: echo hi
  "#;

  let pins = extract_pins(yaml, "test.yml");
  assert_eq!(pins.len(), 3);
  assert_eq!(pins[0].version, "1.98.1");
  assert_eq!(pins[0].line_number, 4);
  assert_eq!(pins[1].version, "1.98.1");
  assert_eq!(pins[1].line_number, 6);
  assert_eq!(pins[2].version, "1.98.1");
  assert_eq!(pins[2].line_number, 8);
}

#[test]
fn test_mismatch_detected_when_channels_differ() {
  let yaml = r#"
    - name: Step 1
      uses: dtolnay/rust-toolchain@1.97.1
  "#;
  let pins = extract_pins(yaml, ".github/workflows/ci.yml");
  let expected_channel = "1.98.1";
  let mismatches: Vec<String> = pins
    .iter()
    .filter(|pin| pin.version != expected_channel)
    .map(|pin| {
      format!(
        "{}:{}: pinned to `{}`, expected `{}` (from rust-toolchain.toml)",
        pin.path, pin.line_number, pin.version, expected_channel
      )
    })
    .collect();

  assert_eq!(mismatches.len(), 1);
  assert_eq!(
    mismatches[0],
    ".github/workflows/ci.yml:3: pinned to `1.97.1`, expected `1.98.1` (from rust-toolchain.toml)"
  );
}
