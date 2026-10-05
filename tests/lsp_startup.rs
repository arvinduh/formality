//! End-to-end coverage for how `fml lsp` starts in a workspace whose
//! `formality.toml` is invalid (issue #474).
//!
//! Editors spawn `fml lsp` with the workspace root as the working directory,
//! so the CLI's own config handling runs against the broken file before the
//! server does. These tests drive the real binary over stdio; the in-process
//! unit tests in `src/engine/lsp.rs` cannot see that layer.

mod common;

use std::process;
use std::time;

use tower_lsp::lsp_types;

use common::lsp;

/// How long the server gets to answer before the test fails.
const TIMEOUT: time::Duration = time::Duration::from_secs(30);

#[test]
fn test_lsp_serves_initialize_with_invalid_root_config() {
  let dir = tempfile::tempdir().expect("tempdir");
  let root = dir.path();
  std::fs::write(root.join("formality.toml"), "[global]\nbogus = 1\n").unwrap();
  let xdg = root.join("xdg");
  std::fs::create_dir(&xdg).unwrap();

  let mut child = lsp::command(root, &xdg)
    .stderr(process::Stdio::null())
    .spawn()
    .expect("failed to spawn fml lsp");

  let root_uri = lsp_types::Url::from_file_path(root).unwrap();
  let mut stdin = child.stdin.take().unwrap();
  lsp::send(
    &mut stdin,
    &[serde_json::json!({
      "jsonrpc": "2.0",
      "id": 1,
      "method": "initialize",
      "params": { "capabilities": {}, "rootUri": root_uri },
    })],
  );
  let rx = lsp::messages(&mut child);

  let deadline = time::Instant::now() + TIMEOUT;
  let mut answered = false;
  let mut error_shown = None;
  while !(answered && error_shown.is_some()) {
    let remaining = deadline.saturating_duration_since(time::Instant::now());
    let Ok(message) = rx.recv_timeout(remaining) else {
      break;
    };
    if message["id"] == 1 && message.get("result").is_some() {
      answered = true;
    } else if message["method"] == "window/showMessage"
      && message["params"]["type"] == 1
    {
      error_shown = message["params"]["message"].as_str().map(String::from);
    }
  }
  let status = lsp::stop(&mut child);

  assert!(
    answered,
    "no initialize result; child exit status: {status:?}"
  );
  let error_shown = error_shown.expect("no showMessage ERROR");
  assert!(
    error_shown.starts_with("[formality] invalid config, using the built-in")
      && error_shown.contains("bogus"),
    "unexpected showMessage text: {error_shown}"
  );
}
