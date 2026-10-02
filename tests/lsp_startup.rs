//! End-to-end coverage for how `fml lsp` starts in a workspace whose
//! `formality.toml` is invalid (issue #474).
//!
//! Editors spawn `fml lsp` with the workspace root as the working directory,
//! so the CLI's own config handling runs against the broken file before the
//! server does. These tests drive the real binary over stdio; the in-process
//! unit tests in `src/commands/lsp.rs` cannot see that layer.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the server gets to answer before the test fails.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Reads Content-Length framed JSON-RPC messages from `stdout` into `tx`
/// until the stream closes.
fn forward_messages(stdout: impl Read, tx: &mpsc::Sender<serde_json::Value>) {
  let mut reader = BufReader::new(stdout);
  loop {
    let mut length = None;
    loop {
      let mut line = String::new();
      if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
      }
      let line = line.trim_end();
      if line.is_empty() {
        break;
      }
      if let Some(value) = line.strip_prefix("Content-Length: ") {
        length = value.parse::<usize>().ok();
      }
    }
    let mut body = vec![0; length.expect("Content-Length header")];
    if reader.read_exact(&mut body).is_err() {
      return;
    }
    let message = serde_json::from_slice(&body).expect("JSON-RPC body");
    if tx.send(message).is_err() {
      return;
    }
  }
}

#[test]
fn test_lsp_serves_initialize_with_invalid_root_config() {
  let dir = tempfile::tempdir().expect("tempdir");
  let root = dir.path();
  std::fs::write(root.join("formality.toml"), "[global]\nbogus = 1\n").unwrap();
  let xdg = root.join("xdg");
  std::fs::create_dir(&xdg).unwrap();

  let mut child = Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("lsp")
    .current_dir(root)
    .env("XDG_CONFIG_HOME", &xdg)
    .env("FORMALITY_NO_UPDATE_CHECK", "1")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .expect("failed to spawn fml lsp");

  let root_uri = tower_lsp::lsp_types::Url::from_file_path(root).unwrap();
  let request = serde_json::json!({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": { "capabilities": {}, "rootUri": root_uri },
  })
  .to_string();
  let mut stdin = child.stdin.take().unwrap();
  // A child that already exited closes the pipe; the asserts report it.
  let _ = write!(stdin, "Content-Length: {}\r\n\r\n{request}", request.len());
  let _ = stdin.flush();

  let (tx, rx) = mpsc::channel();
  let stdout = child.stdout.take().unwrap();
  std::thread::spawn(move || forward_messages(stdout, &tx));

  let deadline = Instant::now() + TIMEOUT;
  let mut answered = false;
  let mut error_shown = None;
  while !(answered && error_shown.is_some()) {
    let remaining = deadline.saturating_duration_since(Instant::now());
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
  let status = child.try_wait().unwrap();
  let _ = child.kill();
  let _ = child.wait();

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
