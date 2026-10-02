//! End-to-end check that `fml lsp` writes nothing to stdout but JSON-RPC
//! frames while it formats and lints in-process (issue #484).
//!
//! The server runs the same runner as `fml fmt`/`fml lint`, whose results
//! table once went to stdout and corrupted the transport. This test drives the
//! real binary over stdio and parses stdout strictly; the in-process unit
//! tests in `src/commands/lsp.rs` never see the process's stdout.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the server gets for the whole exchange before the test fails.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Splits complete JSON-RPC messages off the front of `buf`, strictly.
///
/// An incomplete trailing frame stays in `buf`.
///
/// # Errors
///
/// Returns the offending bytes when anything other than a
/// `Content-Length`-framed JSON body appears.
fn take_frames(buf: &mut Vec<u8>) -> Result<Vec<serde_json::Value>, String> {
  let junk = |buf: &[u8]| String::from_utf8_lossy(buf).into_owned();
  let mut messages = Vec::new();
  loop {
    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
      // A partial header must still be a prefix of a valid one.
      let prefix = b"Content-Length: ";
      let n = buf.len().min(prefix.len());
      if buf[..n] != prefix[..n] {
        return Err(junk(buf));
      }
      return Ok(messages);
    };
    let header =
      std::str::from_utf8(&buf[..header_end]).map_err(|_| junk(buf))?;
    let mut length = None;
    for line in header.split("\r\n") {
      match line.split_once(": ") {
        Some(("Content-Length", value)) => length = value.parse().ok(),
        Some(("Content-Type", _)) => {}
        _ => return Err(junk(buf)),
      }
    }
    let length: usize = length.ok_or_else(|| junk(buf))?;
    let body_end = header_end + 4 + length;
    if buf.len() < body_end {
      return Ok(messages);
    }
    let message = serde_json::from_slice(&buf[header_end + 4..body_end])
      .map_err(|e| format!("{e}: {}", junk(buf)))?;
    messages.push(message);
    buf.drain(..body_end);
  }
}

/// The server's stdout as seen by a strict client.
struct Transcript {
  chunks: mpsc::Receiver<Vec<u8>>,
  buf: Vec<u8>,
  deadline: Instant,
}

impl Transcript {
  /// Reads messages until `done` returns true for one of them.
  ///
  /// # Errors
  ///
  /// Returns the stray bytes if stdout carries anything but JSON-RPC frames,
  /// or a timeout notice if the deadline passes first.
  fn until(
    &mut self,
    mut done: impl FnMut(&serde_json::Value) -> bool,
  ) -> Result<(), String> {
    loop {
      let remaining = self.deadline.saturating_duration_since(Instant::now());
      let chunk = self
        .chunks
        .recv_timeout(remaining)
        .map_err(|e| format!("no expected message ({e})"))?;
      self.buf.extend_from_slice(&chunk);
      let messages = take_frames(&mut self.buf)
        .map_err(|junk| format!("non-JSON-RPC bytes on stdout:\n{junk}"))?;
      if messages.iter().any(&mut done) {
        return Ok(());
      }
    }
  }
}

/// Writes one framed JSON-RPC message to the server.
fn send(stdin: &mut impl Write, message: &serde_json::Value) {
  let text = message.to_string();
  // A child that already exited closes the pipe; the asserts report it.
  let _ = write!(stdin, "Content-Length: {}\r\n\r\n{text}", text.len());
  let _ = stdin.flush();
}

/// Drives initialize, didOpen, formatting and didSave for `file`.
fn exchange(
  stdin: &mut impl Write,
  transcript: &mut Transcript,
  root: &std::path::Path,
  file: &std::path::Path,
) -> Result<(), String> {
  let root_uri = tower_lsp::lsp_types::Url::from_file_path(root).unwrap();
  let file_uri = tower_lsp::lsp_types::Url::from_file_path(file).unwrap();
  let is_diagnostics =
    |m: &serde_json::Value| m["method"] == "textDocument/publishDiagnostics";

  send(
    stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": { "capabilities": {}, "rootUri": root_uri },
    }),
  );
  transcript.until(|m| m["id"] == 1)?;
  send(
    stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "method": "initialized", "params": {},
    }),
  );
  send(
    stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "method": "textDocument/didOpen",
      "params": { "textDocument": {
        "uri": file_uri, "languageId": "python", "version": 1,
        "text": "x=1\n",
      } },
    }),
  );
  transcript.until(is_diagnostics)?;
  send(
    stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting",
      "params": {
        "textDocument": { "uri": file_uri },
        "options": { "tabSize": 4, "insertSpaces": true },
      },
    }),
  );
  transcript.until(|m| m["id"] == 2)?;
  send(
    stdin,
    &serde_json::json!({
      "jsonrpc": "2.0", "method": "textDocument/didSave",
      "params": { "textDocument": { "uri": file_uri } },
    }),
  );
  transcript.until(is_diagnostics)
}

#[test]
fn test_lsp_stdout_carries_only_json_rpc_frames() {
  let dir = tempfile::tempdir().expect("tempdir");
  let root = dir.path();
  let file = root.join("main.py");
  std::fs::write(&file, "x=1\n").unwrap();
  // An empty PATH makes every tool missing: formatting still runs the
  // runner, and the structured lint parser cannot run, so `didOpen` and
  // `didSave` take the in-process `fml lint` fallback.
  let empty_path = root.join("bin");
  let home = root.join("home");
  std::fs::create_dir(&empty_path).unwrap();
  std::fs::create_dir(&home).unwrap();

  let mut child = Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("lsp")
    .current_dir(root)
    .env("PATH", &empty_path)
    .env("HOME", &home)
    .env("XDG_CONFIG_HOME", &home)
    .env("XDG_DATA_HOME", &home)
    .env("XDG_CACHE_HOME", &home)
    .env("FORMALITY_NO_UPDATE_CHECK", "1")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("failed to spawn fml lsp");

  let (tx, chunks) = mpsc::channel();
  let mut stdout = child.stdout.take().unwrap();
  std::thread::spawn(move || {
    let mut chunk = [0; 4096];
    while let Ok(n) = stdout.read(&mut chunk) {
      if n == 0 || tx.send(chunk[..n].to_vec()).is_err() {
        return;
      }
    }
  });
  let mut transcript = Transcript {
    chunks,
    buf: Vec::new(),
    deadline: Instant::now() + TIMEOUT,
  };
  let mut stdin = child.stdin.take().unwrap();
  // Drained on its own thread so a full pipe never stalls the server; it
  // ends once the child is killed below.
  let mut stderr = child.stderr.take().unwrap();
  let stderr_reader = std::thread::spawn(move || {
    let mut log = String::new();
    let _ = stderr.read_to_string(&mut log);
    log
  });

  let outcome = exchange(&mut stdin, &mut transcript, root, &file);
  let status = child.try_wait().unwrap();
  let _ = child.kill();
  let _ = child.wait();
  let log = stderr_reader.join().unwrap();

  if let Err(reason) = outcome {
    panic!("{reason}\nchild exit status: {status:?}");
  }
  // The report moved off stdout, not out of existence: the lint fallback's
  // diagnostic points the user at this log.
  assert!(
    log.contains("fml lint (1 surface)"),
    "run report missing from stderr:\n{log}"
  );
}
