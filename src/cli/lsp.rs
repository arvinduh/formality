//! `fml lsp`: hosts the library's language server on stdio.
//!
//! Owns the process side: the async runtime, the stdio transport, and turning
//! how the session ended into an exit status. What the server does lives in
//! `fml::engine::lsp`.

use std::path;
use std::task;

use log;
use tokio;
use tower_lsp;
use tower_lsp::jsonrpc;
use tower_service;

use fml::engine::lsp::server;
use fml::engine::runner;

/// Wraps the service to report the `exit` notification as it arrives.
///
/// tower-lsp handles `exit` in its own layer and `Server::serve` returns only
/// at stdin EOF, so a client that keeps stdin open would otherwise keep the
/// process alive. This wrapper sees every request first and fires `exited`.
struct ExitSignal {
  service: tower_lsp::LspService<server::Server>,
  /// Fired once, on the first `exit`; dropped with `serve` at stdin EOF.
  exited: Option<tokio::sync::oneshot::Sender<runner::ExitStatus>>,
  /// Set when a `shutdown` request is received, not when it is answered: an
  /// `exit` read in the same poll cancels the pending `shutdown` handler.
  shut_down: bool,
}

impl tower_service::Service<jsonrpc::Request> for ExitSignal {
  type Response = Option<jsonrpc::Response>;
  type Error = tower_lsp::ExitedError;
  type Future =
    <tower_lsp::LspService<server::Server> as tower_service::Service<
      jsonrpc::Request,
    >>::Future;

  fn poll_ready(
    &mut self,
    cx: &mut task::Context<'_>,
  ) -> task::Poll<Result<(), tower_lsp::ExitedError>> {
    tower_service::Service::poll_ready(&mut self.service, cx)
  }

  fn call(&mut self, request: jsonrpc::Request) -> Self::Future {
    let is_exit = request.method() == "exit";
    self.shut_down |= request.method() == "shutdown";
    let response = tower_service::Service::call(&mut self.service, request);
    if is_exit && let Some(exited) = self.exited.take() {
      // The LSP spec: exit 0 after `shutdown`, 1 otherwise. The receiver
      // lives until the process exits, so the send cannot fail.
      let status = if self.shut_down {
        runner::ExitStatus::Clean
      } else {
        runner::ExitStatus::Violations
      };
      let _ = exited.send(status);
    }
    response
  }
}

/// Serves until the client sends `exit` or closes stdin.
///
/// Returns the status the LSP spec prescribes for `exit`, `Clean` when stdin
/// closes first, or `Error` when a handler panics.
///
/// # Panics
///
/// Panics if the Tokio runtime cannot start.
pub fn run(root: &path::Path) -> runner::ExitStatus {
  log::info!(
    "formality LSP v{} on stdio, launched in {}",
    env!("CARGO_PKG_VERSION"),
    root.display()
  );
  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let (service, socket) = tower_lsp::LspService::new(server::Server::new);
  let (exited_tx, exited_rx) = tokio::sync::oneshot::channel();
  let service = ExitSignal {
    service,
    exited: Some(exited_tx),
    shut_down: false,
  };
  let server =
    tower_lsp::Server::new(tokio::io::stdin(), tokio::io::stdout(), socket);
  let serve = rt.spawn(server.serve(service));
  let status = match rt.block_on(exited_rx) {
    Ok(status) => status,
    // `serve` dropped the sender, so it has already finished or unwound and
    // this wait returns at once. A panic must not read as a clean exit.
    Err(_) => match rt.block_on(serve) {
      Ok(()) => runner::ExitStatus::Clean,
      Err(_) => runner::ExitStatus::Error,
    },
  };
  // After `exit`, `serve` is still parked on a stdin read on Tokio's blocking
  // pool, which dropping the runtime would wait for.
  rt.shutdown_background();
  status
}
