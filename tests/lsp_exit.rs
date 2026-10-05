//! End-to-end check that `fml lsp` exits on the `exit` notification while
//! the client still holds stdin open (issue #487).
//!
//! tower-lsp's `Server::serve` returns only on stdin EOF, so the process must
//! act on `exit` itself. The in-process unit tests in `src/commands/lsp.rs`
//! never run `serve` or the process exit path.

mod common;

use std::path;
use std::process;
use std::sync::mpsc;
use std::time;

use tower_lsp::lsp_types;

use common::lsp;

/// How long the server gets to answer a request before the test fails.
const RESPONSE_TIMEOUT: time::Duration = time::Duration::from_secs(30);

/// How long the server gets to exit after `exit` before the test fails.
const EXIT_TIMEOUT: time::Duration = time::Duration::from_secs(5);

/// How the client sends `shutdown` before `exit`.
enum Shutdown {
  /// No `shutdown` at all.
  Skip,
  /// `shutdown`, then `exit` once the response arrives (spec-following).
  Awaited,
  /// `shutdown` and `exit` in one write, without awaiting the response.
  SameWriteAsExit,
}

/// Spawns `fml lsp` in an empty workspace with piped stdio.
fn spawn_server(root: &path::Path) -> process::Child {
  lsp::no_color(&mut lsp::command(root, root))
    .stderr(process::Stdio::null())
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
fn exit_code_after_exit(shutdown: &Shutdown) -> Option<i32> {
  let dir = tempfile::tempdir().expect("tempdir");
  let mut child = spawn_server(dir.path());
  let mut stdin = child.stdin.take().unwrap();
  let rx = lsp::messages(&mut child);

  let root_uri = lsp_types::Url::from_file_path(dir.path()).unwrap();
  lsp::send(
    &mut stdin,
    &[serde_json::json!({
      "jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": { "capabilities": {}, "rootUri": root_uri },
    })],
  );
  await_response(&rx, 1);
  lsp::send(
    &mut stdin,
    &[serde_json::json!({
      "jsonrpc": "2.0", "method": "initialized", "params": {},
    })],
  );
  let shutdown_request =
    serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" });
  let exit = serde_json::json!({ "jsonrpc": "2.0", "method": "exit" });
  match shutdown {
    Shutdown::Skip => lsp::send(&mut stdin, &[exit]),
    Shutdown::Awaited => {
      lsp::send(&mut stdin, &[shutdown_request]);
      await_response(&rx, 2);
      lsp::send(&mut stdin, &[exit]);
    }
    Shutdown::SameWriteAsExit => {
      lsp::send(&mut stdin, &[shutdown_request, exit]);
    }
  }

  // The forwarder drops its sender when stdout closes, which happens only
  // when the process exits; stdin stays open throughout.
  let deadline = time::Instant::now() + EXIT_TIMEOUT;
  let exited = loop {
    let remaining = deadline.saturating_duration_since(time::Instant::now());
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
  assert_eq!(exit_code_after_exit(&Shutdown::Awaited), Some(0));
}

#[test]
fn test_lsp_exits_with_zero_on_exit_in_same_write_as_shutdown() {
  // The spec keys code 0 on `shutdown` being received, not answered.
  assert_eq!(exit_code_after_exit(&Shutdown::SameWriteAsExit), Some(0));
}

#[test]
fn test_lsp_exits_with_one_on_exit_without_shutdown() {
  assert_eq!(exit_code_after_exit(&Shutdown::Skip), Some(1));
}
