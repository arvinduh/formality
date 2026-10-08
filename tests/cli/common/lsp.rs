//! Stdio JSON-RPC helpers for tests that drive the real `fml lsp` binary.
//!
//! Owns Content-Length framing in both directions and the spawn of the
//! server process. Each test file owns its own timeouts, exchange and
//! assertions; `tests/lsp_stdout.rs` keeps its strict byte-level parser,
//! since it must see stray bytes this reader would skip.

use std::fmt;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::path;
use std::process;
use std::sync::mpsc;

/// Builds an `fml lsp` command in `root` with piped stdin and stdout,
/// update checks off and user config read from `config_home`.
pub fn command(
  root: &path::Path,
  config_home: &path::Path,
) -> process::Command {
  let mut command = process::Command::new(env!("CARGO_BIN_EXE_fml"));
  command
    .arg("lsp")
    .current_dir(root)
    .env("XDG_CONFIG_HOME", config_home)
    .env("FORMALITY_NO_UPDATE_CHECK", "1")
    .stdin(process::Stdio::piped())
    .stdout(process::Stdio::piped());
  command
}

/// Disables colored output in `command` regardless of the caller's env.
pub fn no_color(command: &mut process::Command) -> &mut process::Command {
  command
    .env("NO_COLOR", "1")
    .env_remove("FORCE_COLOR")
    .env_remove("CLICOLOR_FORCE")
}

/// Writes framed JSON-RPC messages to the server in a single write.
pub fn send(stdin: &mut impl Write, messages: &[serde_json::Value]) {
  let mut framed = String::new();
  for message in messages {
    let text = message.to_string();
    let _ = fmt::Write::write_fmt(
      &mut framed,
      format_args!("Content-Length: {}\r\n\r\n{text}", text.len()),
    );
  }
  // A child that already exited closes the pipe; the asserts report it.
  let _ = stdin.write_all(framed.as_bytes());
  let _ = stdin.flush();
}

/// Takes the child's stdout and decodes it into messages on a thread.
///
/// The receiver disconnects once stdout closes, i.e. when the process exits.
///
/// # Panics
///
/// Panics if stdout was not piped or was already taken.
pub fn messages(
  child: &mut process::Child,
) -> mpsc::Receiver<serde_json::Value> {
  let (tx, rx) = mpsc::channel();
  let stdout = child.stdout.take().expect("piped stdout");
  std::thread::spawn(move || forward_messages(stdout, &tx));
  rx
}

/// Reads Content-Length framed JSON-RPC messages from `stdout` into `tx`
/// until the stream closes.
fn forward_messages(stdout: impl Read, tx: &mpsc::Sender<serde_json::Value>) {
  let mut reader = io::BufReader::new(stdout);
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

/// Kills the child and reaps it, returning its exit status if it had
/// already exited on its own.
pub fn stop(child: &mut process::Child) -> Option<process::ExitStatus> {
  let status = child.try_wait().unwrap();
  let _ = child.kill();
  let _ = child.wait();
  status
}
