//! End-to-end check that `fml lsp` exits on the `exit` notification while
//! the client still holds stdin open (issue #487).
//!
//! tower-lsp's `Server::serve` returns only on stdin EOF, so the process must
//! act on `exit` itself. The in-process unit tests in `src/commands/lsp.rs`
//! never run `serve` or the process exit path.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the server gets to answer a request before the test fails.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the server gets to exit after `exit` before the test fails.
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Writes one framed JSON-RPC message to the server.
fn send(stdin: &mut impl Write, message: &serde_json::Value) {
  let text = message.to_string();
  // A child that already exited closes the pipe; the asserts report it.
  let _ = write!(stdin, "Content-Length: {}\r\n\r\n{text}", text.len());
  let _ = stdin.flush();
}

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

/// Spawns `fml lsp` in an empty workspace with piped stdio.
fn spawn_server(root: &std::path::Path) -> Child {
  Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("lsp")
    .current_dir(root)
    .env("XDG_CONFIG_HOME", root)
    .env("FORMALITY_NO_UPDATE_CHECK", "1")
    .env("NO_COLOR", "1")
    .env_remove("FORCE_COLOR")
    .env_remove("CLICOLOR_FORCE")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .expect("failed to spawn fml lsp")
}

/// Waits up to [`RESPONSE_TIMEOUT`] for the response to request `id`.
fn await_response(rx: &mpsc::Receiver<serde_json::Value>, id: u64) {
  loop {
    let message = rx
      .recv_timeout(RESPONSE_TIMEOUT)
      .unwrap_or_else(|e| panic!("no response to request {id} ({e})"));
    if message["id"] == id {
      return;
    }
  }
}

/// Runs the lifecycle, sends `exit` with stdin held open, and returns the
/// child's exit code, or `None` if it was still running after
/// [`EXIT_TIMEOUT`].
fn exit_code_after_exit(send_shutdown: bool) -> Option<i32> {
  let dir = tempfile::tempdir().expect("tempdir");
  let mut child = spawn_server(dir.path());
  let mut stdin = child.stdin.take().unwrap();
  let (tx, rx) = mpsc::channel();
  let stdout = child.stdout.take().unwrap();
  std::thread::spawn(move || forward_messages(stdout, &tx));

  let root_uri = tower_lsp::lsp_types::Url::from_file_path(dir.path()).unwrap();
  send(
    &mut stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": { "capabilities": {}, "rootUri": root_uri },
    }),
  );
  await_response(&rx, 1);
  send(
    &mut stdin,
    &serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
  );
  if send_shutdown {
    send(
      &mut stdin,
      &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }),
    );
    await_response(&rx, 2);
  }
  send(
    &mut stdin,
    &serde_json::json!({ "jsonrpc": "2.0", "method": "exit" }),
  );

  // The forwarder drops its sender when stdout closes, which happens only
  // when the process exits; stdin stays open throughout.
  let deadline = Instant::now() + EXIT_TIMEOUT;
  let exited = loop {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match rx.recv_timeout(remaining) {
      Ok(_) => {}
      Err(mpsc::RecvTimeoutError::Disconnected) => break true,
      Err(mpsc::RecvTimeoutError::Timeout) => break false,
    }
  };
  if !exited {
    let _ = child.kill();
  }
  let status = child.wait().unwrap();
  drop(stdin);
  exited.then(|| status.code().expect("exit code, not a signal"))
}

#[test]
fn test_lsp_exits_with_zero_on_exit_after_shutdown() {
  assert_eq!(exit_code_after_exit(true), Some(0));
}

#[test]
fn test_lsp_exits_with_one_on_exit_without_shutdown() {
  assert_eq!(exit_code_after_exit(false), Some(1));
}
