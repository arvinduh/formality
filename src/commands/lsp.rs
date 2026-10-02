//! `fml lsp` — document formatter and diagnostics publisher.
//!
//! Architecture
//! ============
//! The formality LSP server runs as a single process that:
//!
//! 1. **Accepts** LSP requests from the editor (via stdio).
//! 2. **Handles** `textDocument/formatting` by running `fml fmt` in-process
//!    against the requested file and returning the resulting edits.
//! 3. **Publishes diagnostics** on `did_save` / `did_open` by running
//!    `fml lint` (or a structured per-surface parser, see
//!    `lsp_diagnostics.rs`) in-process against the changed file.
//! 4. **Watches** `formality.toml` / `.formality.toml` via
//!    `did_change_watched_files` and reloads the cached configuration
//!    when the canonical config changes.
//!
//! An invalid config is never silently dropped: each failed load sends one
//! `window/showMessage` error. At initialize the server then uses the
//! built-in defaults; on a failed reload it keeps the previous config.
//!
//! This server is a formatting and diagnostics provider, meant to run
//! *alongside* the user's existing language servers (rust-analyzer, pyright,
//! clangd, …) — it does not spawn, proxy, or route requests to them. See
//! `README.md`'s "Editor setup" section for how to wire `fml lsp` in
//! alongside a primary language server.
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::jsonrpc::{Request, Response, Result as LspResult};
use tower_lsp::lsp_types::{
  Diagnostic, DiagnosticSeverity, DidChangeWatchedFilesParams,
  DidOpenTextDocumentParams, DidSaveTextDocumentParams,
  DocumentFormattingParams, InitializeParams, InitializeResult,
  InitializedParams, MessageType, OneOf, Position, Range, ServerCapabilities,
  ServerInfo, TextDocumentIdentifier, TextDocumentSyncCapability,
  TextDocumentSyncKind, TextEdit,
};
use tower_lsp::{Client, ExitedError, LanguageServer, LspService, Server};

use crate::config::FormalityConfig;
use crate::errors::ExitStatus;

/// Server identity reported in `initialize`'s `ServerInfo`.
const SERVER_NAME: &str = "formality";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns whether the specified path points to a formality configuration file (`formality.toml` or `.formality.toml`).
#[must_use]
pub fn is_formality_config_file(path: &Path) -> bool {
  path
    .file_name()
    .and_then(|n| n.to_str())
    .is_some_and(|name| crate::config::CONFIG_FILE_CANDIDATES.contains(&name))
}

// ---------------------------------------------------------------------------
// LSP server backend
// ---------------------------------------------------------------------------

/// The formality LSP backend: a document formatter and diagnostics publisher.
///
/// `root` is the workspace root, resolved at `initialize` time and used to
/// locate `formality.toml` and to run `fml fmt` / `fml lint` in-process
/// against the correct working directory.
pub struct FormalityLsp {
  client: Client,
  /// Workspace root detected at `initialize` time.
  root: tokio::sync::Mutex<Option<PathBuf>>,
  /// Cached formality configuration, loaded at initialize time and replaced
  /// only by a successful reload after `formality.toml` /
  /// `.formality.toml` changes.
  config: Arc<tokio::sync::RwLock<Option<FormalityConfig>>>,
  /// Set once a `shutdown` request has been handled; decides the exit code.
  shut_down: std::sync::atomic::AtomicBool,
}

impl FormalityLsp {
  /// Creates a new [`FormalityLsp`] instance with the provided client handle.
  #[must_use]
  pub fn new(client: Client) -> Self {
    Self {
      client,
      root: tokio::sync::Mutex::new(None),
      config: Arc::new(tokio::sync::RwLock::new(None)),
      shut_down: std::sync::atomic::AtomicBool::new(false),
    }
  }

  /// Returns the cached configuration, or loads and caches it if not yet present.
  ///
  /// An invalid config is reported once (see [`Self::load_config`]) and the
  /// built-in defaults are cached in its place, so later requests reuse them
  /// instead of re-reporting the same error.
  ///
  /// The load branch only runs before `initialize`, which always fills the
  /// cache. tower-lsp rejects requests and notifications that arrive before
  /// `initialize`, so in practice only tests reach it.
  pub async fn get_or_load_config(
    &self,
    root: Option<&Path>,
  ) -> FormalityConfig {
    if let Some(config) = self.config.read().await.as_ref() {
      return config.clone();
    }
    let mut lock = self.config.write().await;
    if let Some(config) = lock.as_ref() {
      return config.clone();
    }
    let loaded = self
      .load_config(root, "using the built-in defaults")
      .await
      .unwrap_or_else(FormalityConfig::with_defaults);
    *lock = Some(loaded.clone());
    loaded
  }

  /// Loads the layered config for `root`, reporting a failure to the client.
  ///
  /// `fallback` names what the server uses instead; it is part of the message.
  ///
  /// # Side Effects
  ///
  /// On failure, sends one `window/showMessage` (ERROR) carrying the
  /// [`crate::config::ConfigError`] text and returns `None`.
  async fn load_config(
    &self,
    root: Option<&Path>,
    fallback: &str,
  ) -> Option<FormalityConfig> {
    match FormalityConfig::load_layered(root) {
      Ok((config, _)) => Some(config),
      Err(err) => {
        let message = format!("[formality] invalid config, {fallback}: {err}");
        self.client.show_message(MessageType::ERROR, message).await;
        None
      }
    }
  }
}

#[tower_lsp::async_trait]
impl LanguageServer for FormalityLsp {
  async fn initialize(
    &self,
    params: InitializeParams,
  ) -> LspResult<InitializeResult> {
    // Resolve workspace root from the initialize params.
    let root = params
      .root_uri
      .as_ref()
      .and_then(|u| u.to_file_path().ok())
      .or_else(|| {
        #[allow(deprecated)]
        params.root_path.as_ref().map(PathBuf::from)
      });

    *self.root.lock().await = root.clone();

    // Cache resolved config at initialize time. An invalid config has no
    // earlier valid one to fall back to, so the defaults stand in.
    let config = self
      .load_config(root.as_deref(), "using the built-in defaults")
      .await
      .unwrap_or_else(FormalityConfig::with_defaults);
    *self.config.write().await = Some(config);

    Ok(InitializeResult {
      server_info: Some(ServerInfo {
        name: SERVER_NAME.to_string(),
        version: Some(SERVER_VERSION.to_string()),
      }),
      capabilities: ServerCapabilities {
        // formality handles formatting itself via `fml fmt` — no child
        // server is spawned or delegated to.
        document_formatting_provider: Some(OneOf::Left(true)),
        // Not implemented — formality only formats whole documents.
        document_range_formatting_provider: None,
        // Document sync capability: NONE matches disk-reading behavior.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
          TextDocumentSyncKind::NONE,
        )),
        // Nothing else is provided. Hover, completion, go-to-definition,
        // and every other language-intelligence capability are left to
        // whatever primary language server the editor already runs
        // alongside `fml lsp`.
        ..Default::default()
      },
    })
  }

  async fn initialized(&self, _: InitializedParams) {
    self
      .client
      .log_message(
        MessageType::INFO,
        format!("formality LSP v{SERVER_VERSION} initialized"),
      )
      .await;

    // Detect active surfaces and log which ones formality will format and
    // lint in this workspace.
    let root = self.root.lock().await.clone();
    let config = self.get_or_load_config(root.as_deref()).await;

    if let Some(ref root_path) = root {
      let detected = crate::surfaces::detect_surfaces_smart(root_path, &config);
      let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();
      if !names.is_empty() {
        self
          .client
          .log_message(
            MessageType::INFO,
            format!("[formality] active surfaces: {}", names.join(", ")),
          )
          .await;
      }
    }
  }

  async fn shutdown(&self) -> LspResult<()> {
    self
      .shut_down
      .store(true, std::sync::atomic::Ordering::Release);
    Ok(())
  }

  // -------------------------------------------------------------------------
  // Formatting — always handled by `fml fmt`, never delegated.
  // -------------------------------------------------------------------------

  async fn formatting(
    &self,
    params: DocumentFormattingParams,
  ) -> LspResult<Option<Vec<TextEdit>>> {
    let path = params.text_document.uri.to_file_path().unwrap_or_default();

    let root = self.root.lock().await.clone().unwrap_or_else(|| {
      path.parent().map(Path::to_path_buf).unwrap_or_default()
    });

    // Read the current file content so we can diff it after formatting.
    let before = match std::fs::read_to_string(&path) {
      Ok(s) => s,
      Err(e) => {
        self
          .client
          .log_message(
            MessageType::ERROR,
            format!("[formality] cannot read {}: {e}", path.display()),
          )
          .await;
        return Ok(None);
      }
    };

    let config = self.get_or_load_config(Some(&root)).await;

    // stdout is the JSON-RPC transport, so the report goes to stderr.
    let status = super::run_resolved(
      &mut std::io::stderr(),
      &root,
      &config,
      &[],
      std::slice::from_ref(&path),
      &crate::engine::Plan::fmt(false, false),
    );

    if status.is_clean() {
      let after = std::fs::read_to_string(&path).unwrap_or_default();
      Ok(Some(compute_formatting_edits(&before, &after)))
    } else {
      self
        .client
        .log_message(
          MessageType::ERROR,
          format!("[formality] fml fmt failed for {}", path.display()),
        )
        .await;
      Ok(None)
    }
  }

  // -------------------------------------------------------------------------
  // Document sync — used to trigger `fml lint` diagnostics on save.
  // -------------------------------------------------------------------------

  async fn did_save(&self, params: DidSaveTextDocumentParams) {
    let path = params.text_document.uri.to_file_path().unwrap_or_default();
    let root = self.root.lock().await.clone().unwrap_or_else(|| {
      path.parent().map(Path::to_path_buf).unwrap_or_default()
    });
    let uri = params.text_document.uri.clone();
    let config = self.get_or_load_config(Some(&root)).await;

    // For surfaces with structured-output support wired up (rust via
    // clippy, python via ruff — see `lsp_diagnostics`), publish one
    // `Diagnostic` per real violation with correct range/message/severity —
    // but only when that structured tool actually ran.
    // `diagnostics_for_file_with_config` returns `None` both for surfaces
    // with no structured parser at all and for ones whose parser couldn't run this time (binary missing, no
    // project marker file, spawn failure, required config missing) — either
    // way this falls back to running in-process `fml lint` and, on non-zero
    // exit, a single generic warning pointing at the output channel — the
    // same behavior this module had before #159 [pre-recreation]. This is what keeps a file
    // from being published "clean" when the structured tool never actually
    // ran (#177 [pre-recreation]).
    let diagnostics = if let Some(diags) =
      crate::commands::lsp_diagnostics::diagnostics_for_file_with_config(
        &root,
        &path,
        Some(&config),
      ) {
      diags
    } else {
      // stderr is the server log the fallback diagnostic points at; stdout
      // is the JSON-RPC transport.
      let status = super::run_resolved(
        &mut std::io::stderr(),
        &root,
        &config,
        &[],
        std::slice::from_ref(&path),
        &crate::engine::Plan::lint(false),
      );

      if status.is_clean() {
        vec![]
      } else {
        vec![Diagnostic {
          range: Range::default(),
          severity: Some(DiagnosticSeverity::WARNING),
          source: Some("formality".to_string()),
          message: "fml lint found issues — see the Formality output channel."
            .to_string(),
          ..Default::default()
        }]
      }
    };

    self
      .client
      .publish_diagnostics(uri, diagnostics, None)
      .await;
  }

  async fn did_open(&self, params: DidOpenTextDocumentParams) {
    // Trigger lint on open so diagnostics appear immediately.
    self
      .did_save(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier {
          uri: params.text_document.uri,
        },
        text: None,
      })
      .await;
  }

  async fn did_change_watched_files(
    &self,
    params: DidChangeWatchedFilesParams,
  ) {
    let has_config_change = params.changes.iter().any(|change| {
      change
        .uri
        .to_file_path()
        .ok()
        .is_some_and(|p| is_formality_config_file(&p))
    });

    if has_config_change {
      let root = self.root.lock().await.clone();
      // A failed reload keeps the previous config; editing the file mid-way
      // must not throw away a working setup.
      let Some(config) = self
        .load_config(root.as_deref(), "keeping the previous config")
        .await
      else {
        return;
      };
      *self.config.write().await = Some(config);
      self
        .client
        .log_message(MessageType::INFO, "[formality] configuration reloaded")
        .await;
    }
  }
}

// ---------------------------------------------------------------------------
// Formatting helper functions
// ---------------------------------------------------------------------------

/// Computes the whole-document LSP [`Range`] for the given document content.
///
/// Per the Language Server Protocol specification:
/// - Line bounds are 0-indexed, so the end line is `line_count.saturating_sub(1)`.
/// - Character offsets are based on UTF-16 code units, not UTF-8 byte lengths or
///   Unicode scalar values.
#[must_use]
pub fn full_document_range(text: &str) -> Range {
  let line_count = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
  let last_col = text.lines().last().map_or(0, |l| {
    u32::try_from(l.encode_utf16().count()).unwrap_or(u32::MAX)
  });

  Range {
    start: Position {
      line: 0,
      character: 0,
    },
    end: Position {
      line: line_count.saturating_sub(1),
      character: last_col,
    },
  }
}

/// Computes the [`TextEdit`] list required to replace the document with formatted content.
///
/// Returns an empty vector if `before == after`.
#[must_use]
pub fn compute_formatting_edits(before: &str, after: &str) -> Vec<TextEdit> {
  if before == after {
    return Vec::new();
  }
  vec![TextEdit {
    range: full_document_range(before),
    new_text: after.to_string(),
  }]
}

// ---------------------------------------------------------------------------
// Entry point called from lib.rs / Commands::Lsp
// ---------------------------------------------------------------------------

/// Wraps the [`LspService`] to report the `exit` notification as it arrives.
///
/// tower-lsp handles `exit` in its own layer and `Server::serve` returns only
/// at stdin EOF, so a client that keeps stdin open would otherwise keep the
/// process alive. This wrapper sees every request first and fires `exited`.
struct ExitSignal {
  service: LspService<FormalityLsp>,
  /// Fired once, on the first `exit`; dropped with `serve` at stdin EOF.
  exited: Option<tokio::sync::oneshot::Sender<ExitStatus>>,
}

impl tower_service::Service<Request> for ExitSignal {
  type Response = Option<Response>;
  type Error = ExitedError;
  type Future =
    <LspService<FormalityLsp> as tower_service::Service<Request>>::Future;

  fn poll_ready(
    &mut self,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Result<(), ExitedError>> {
    tower_service::Service::poll_ready(&mut self.service, cx)
  }

  fn call(&mut self, request: Request) -> Self::Future {
    let is_exit = request.method() == "exit";
    let response = tower_service::Service::call(&mut self.service, request);
    if is_exit && let Some(exited) = self.exited.take() {
      let shut_down = self
        .service
        .inner()
        .shut_down
        .load(std::sync::atomic::Ordering::Acquire);
      // The receiver lives until the process exits.
      let _ = exited.send(exit_status(shut_down));
    }
    response
  }
}

/// Maps the LSP spec's exit-code rule onto [`ExitStatus`].
///
/// The spec: "The server should exit with `success` code 0 if the shutdown
/// request has been received before; otherwise with `error` code 1." Code 1
/// is [`ExitStatus::Violations`]; here it means `exit` came without
/// `shutdown`, not lint violations.
fn exit_status(shut_down: bool) -> ExitStatus {
  if shut_down {
    ExitStatus::Clean
  } else {
    ExitStatus::Violations
  }
}

/// Maps how the `serve` task ended, absent an `exit`, onto [`ExitStatus`].
///
/// `Ok` is stdin EOF, a normal stop. A [`tokio::task::JoinError`] is a
/// handler panic (the panic hook has already printed it), which must not read
/// as a clean exit to the client.
fn serve_status(joined: &Result<(), tokio::task::JoinError>) -> ExitStatus {
  match joined {
    Ok(()) => ExitStatus::Clean,
    Err(_) => ExitStatus::Error,
  }
}

/// Start the formality LSP server on stdio.
///
/// Blocks until the client sends `exit` or closes stdin. Intended to be called
/// from `fml lsp`.
///
/// Returns the exit status the LSP spec prescribes for `exit` (see
/// [`exit_status`]), [`ExitStatus::Clean`] when stdin closes first, or
/// [`ExitStatus::Error`] when a handler panics (see [`serve_status`]).
///
/// # Panics
///
/// Panics if the underlying Tokio runtime fails to initialize.
pub fn run_lsp_server(root: Option<&Path>) -> ExitStatus {
  // Print a startup banner to stderr (not stdout — that's the LSP channel).
  eprintln!(
    "{} LSP server starting (stdio transport, v{SERVER_VERSION})",
    "formality".cyan().bold()
  );
  if let Some(r) = root {
    eprintln!("  workspace root: {}", r.display());
  }

  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let (service, socket) = LspService::new(FormalityLsp::new);
  let (exited_tx, exited_rx) = tokio::sync::oneshot::channel();
  let service = ExitSignal {
    service,
    exited: Some(exited_tx),
  };
  let server = Server::new(tokio::io::stdin(), tokio::io::stdout(), socket);
  // `serve` owns the sender, so the receiver resolves on `exit` or, when
  // `serve` returns at stdin EOF, on the dropped sender.
  let serve = rt.spawn(server.serve(service));
  let status = match rt.block_on(exited_rx) {
    Ok(status) => status,
    // `serve` dropped the sender, so it has already finished or unwound and
    // this second wait returns at once. Never `resume_unwind` a panic here:
    // dropping the runtime while unwinding would block on the stdin read.
    Err(_) => serve_status(&rt.block_on(serve)),
  };
  // After `exit`, `serve` is still parked on a stdin read on Tokio's blocking
  // pool, which dropping the runtime would wait for. Shutting down in the
  // background abandons it; the process exit that follows ends the thread.
  rt.shutdown_background();
  status
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_full_document_range_empty_document() {
    let range = full_document_range("");
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 0,
        character: 0
      }
    );
  }

  #[test]
  fn test_full_document_range_single_line() {
    let range = full_document_range("hello world");
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 0,
        character: 11
      }
    );
  }

  #[test]
  fn test_full_document_range_single_line_with_trailing_newline() {
    let range = full_document_range("hello world\n");
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 0,
        character: 11
      }
    );
  }

  #[test]
  fn test_full_document_range_multiline() {
    let text = "fn main() {\n    println!(\"hello\");\n}";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 2,
        character: 1
      }
    );
  }

  #[test]
  fn test_full_document_range_multiline_with_trailing_newline() {
    let text = "line 1\nline 2\nline 3\n";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 2,
        character: 6
      }
    );
  }

  #[test]
  fn test_full_document_range_multibyte_unicode_utf16_counts() {
    // 🦀 is 4 UTF-8 bytes, but 2 UTF-16 code units (surrogate pair)
    // 🚀 is 4 UTF-8 bytes, but 2 UTF-16 code units
    let text = "let crab = \"🦀 🚀\";";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    // "let crab = \"" = 12
    // "🦀" = 2
    // " " = 1
    // "🚀" = 2
    // "\";" = 2
    // Total = 19 UTF-16 code units (vs 23 UTF-8 bytes)
    assert_eq!(
      range.end,
      Position {
        line: 0,
        character: 19
      }
    );

    // Chinese characters: 3 UTF-8 bytes each, 1 UTF-16 code unit each
    let chinese = "你好世界";
    let range_chinese = full_document_range(chinese);
    assert_eq!(
      range_chinese.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range_chinese.end,
      Position {
        line: 0,
        character: 4
      }
    );
  }

  #[test]
  fn test_full_document_range_multiline_with_multibyte_unicode() {
    let text = "fn main() {\n    // 🦀 🚀\n}";
    let range = full_document_range(text);
    assert_eq!(
      range.start,
      Position {
        line: 0,
        character: 0
      }
    );
    assert_eq!(
      range.end,
      Position {
        line: 2,
        character: 1
      }
    );

    let text_unicode_last_line = "fn main() {\n    let s = \"你好 🌍\";";
    let range_unicode_last = full_document_range(text_unicode_last_line);
    assert_eq!(
      range_unicode_last.start,
      Position {
        line: 0,
        character: 0
      }
    );
    // Line 1: "    let s = \"你好 🌍\";" -> 13 + 2 + 1 + 2 + 2 = 20 UTF-16 code units
    assert_eq!(
      range_unicode_last.end,
      Position {
        line: 1,
        character: 20
      }
    );
  }

  #[test]
  fn test_compute_formatting_edits_no_change() {
    let content = "fn main() {}\n";
    let edits = compute_formatting_edits(content, content);
    assert!(edits.is_empty());
  }

  #[test]
  fn test_compute_formatting_edits_with_changes() {
    let before = "fn main(){\nprintln!(\"hello\");\n}";
    let after = "fn main() {\n    println!(\"hello\");\n}\n";
    let edits = compute_formatting_edits(before, after);
    assert_eq!(edits.len(), 1);
    assert_eq!(
      edits[0].range,
      Range {
        start: Position {
          line: 0,
          character: 0
        },
        end: Position {
          line: 2,
          character: 1
        },
      }
    );
    assert_eq!(edits[0].new_text, after);
  }

  #[test]
  fn test_compute_formatting_edits_multibyte_unicode() {
    let before = "fn main() {\nlet msg = \"🦀 世界\";\n}";
    let after = "fn main() {\n    let msg = \"🦀 世界\";\n}\n";
    let edits = compute_formatting_edits(before, after);
    assert_eq!(edits.len(), 1);
    assert_eq!(
      edits[0].range,
      Range {
        start: Position {
          line: 0,
          character: 0
        },
        end: Position {
          line: 2,
          character: 1
        },
      }
    );
    assert_eq!(edits[0].new_text, after);
  }

  #[tokio::test]
  async fn test_lsp_formatting_nonexistent_file_returns_none() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(root).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let missing_path = root.join("nonexistent.rs");
    let missing_uri =
      tower_lsp::lsp_types::Url::from_file_path(&missing_path).unwrap();

    let result = server
      .formatting(DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: missing_uri },
        options: tower_lsp::lsp_types::FormattingOptions::default(),
        work_done_progress_params:
          tower_lsp::lsp_types::WorkDoneProgressParams::default(),
      })
      .await
      .unwrap();

    assert!(result.is_none());
  }

  #[tokio::test]
  async fn test_lsp_formatting_inprocess_unmatched_file_returns_empty_edits() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(root).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let file_path = root.join("notes.txt");
    std::fs::write(&file_path, "plain text without code formatting\n").unwrap();
    let file_uri =
      tower_lsp::lsp_types::Url::from_file_path(&file_path).unwrap();

    let result = server
      .formatting(DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: file_uri },
        options: tower_lsp::lsp_types::FormattingOptions::default(),
        work_done_progress_params:
          tower_lsp::lsp_types::WorkDoneProgressParams::default(),
      })
      .await
      .unwrap();

    assert_eq!(result, Some(vec![]));
  }

  #[tokio::test]
  async fn test_lsp_did_save_inprocess_execution() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(root).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let file_path = root.join("notes.txt");
    std::fs::write(&file_path, "clean notes\n").unwrap();
    let file_uri =
      tower_lsp::lsp_types::Url::from_file_path(&file_path).unwrap();

    // did_save dispatches in-process lint fallback without spawning an fml child process
    server
      .did_save(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: file_uri },
        text: None,
      })
      .await;
  }

  #[test]
  fn test_is_formality_config_file() {
    assert!(is_formality_config_file(Path::new("formality.toml")));
    assert!(is_formality_config_file(Path::new(".formality.toml")));
    assert!(is_formality_config_file(Path::new(
      "/path/to/project/formality.toml"
    )));
    assert!(is_formality_config_file(Path::new(
      "/path/to/project/.formality.toml"
    )));
    #[cfg(windows)]
    assert!(is_formality_config_file(Path::new(
      "C:\\path\\to\\project\\.formality.toml"
    )));

    assert!(!is_formality_config_file(Path::new("other.toml")));
    assert!(!is_formality_config_file(Path::new("Cargo.toml")));
    assert!(!is_formality_config_file(Path::new("notes.txt")));
  }

  #[tokio::test]
  async fn test_lsp_initialize_capabilities_sync_kind_none() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();

    let init_result = server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    assert_eq!(
      init_result.capabilities.text_document_sync,
      Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::NONE))
    );
  }

  #[tokio::test]
  async fn test_lsp_config_caching_on_initialize() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 4\n").unwrap();

    assert!(server.config.read().await.clone().is_none());

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let cached = server.config.read().await.clone();
    assert!(cached.is_some());
    let cfg = cached.unwrap();
    assert_eq!(cfg.global.as_ref().and_then(|g| g.indent_size), Some(4));
  }

  #[tokio::test]
  async fn test_lsp_watcher_invalidation_on_did_change_watched_files() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 4\n").unwrap();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let cfg_before = server.config.read().await.clone().unwrap();
    assert_eq!(
      cfg_before.global.as_ref().and_then(|g| g.indent_size),
      Some(4)
    );

    // Modify formality.toml on disk
    std::fs::write(&config_path, "[global]\nindent_size = 8\n").unwrap();

    // Trigger watcher event
    let uri = tower_lsp::lsp_types::Url::from_file_path(&config_path).unwrap();
    server
      .did_change_watched_files(DidChangeWatchedFilesParams {
        changes: vec![tower_lsp::lsp_types::FileEvent {
          uri,
          typ: tower_lsp::lsp_types::FileChangeType::CHANGED,
        }],
      })
      .await;

    let cfg_after = server.config.read().await.clone().unwrap();
    assert_eq!(
      cfg_after.global.as_ref().and_then(|g| g.indent_size),
      Some(8)
    );
  }

  #[tokio::test]
  async fn test_lsp_watcher_invalidation_hidden_formality_toml() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join(".formality.toml");
    std::fs::write(&config_path, "[global]\nline_length = 100\n").unwrap();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let cfg_before = server.config.read().await.clone().unwrap();
    assert_eq!(
      cfg_before.global.as_ref().and_then(|g| g.line_length),
      Some(100)
    );

    // Modify .formality.toml on disk
    std::fs::write(&config_path, "[global]\nline_length = 120\n").unwrap();

    let uri = tower_lsp::lsp_types::Url::from_file_path(&config_path).unwrap();
    server
      .did_change_watched_files(DidChangeWatchedFilesParams {
        changes: vec![tower_lsp::lsp_types::FileEvent {
          uri,
          typ: tower_lsp::lsp_types::FileChangeType::CHANGED,
        }],
      })
      .await;

    let cfg_after = server.config.read().await.clone().unwrap();
    assert_eq!(
      cfg_after.global.as_ref().and_then(|g| g.line_length),
      Some(120)
    );
  }

  #[tokio::test]
  async fn test_lsp_watcher_ignores_non_config_file_changes() {
    let (service, _) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 4\n").unwrap();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    // Modify formality.toml on disk without triggering watcher for it
    std::fs::write(&config_path, "[global]\nindent_size = 8\n").unwrap();

    // Trigger watcher for an unrelated file
    let other_path = temp.path().join("src/main.rs");
    let uri = tower_lsp::lsp_types::Url::from_file_path(&other_path).unwrap();
    server
      .did_change_watched_files(DidChangeWatchedFilesParams {
        changes: vec![tower_lsp::lsp_types::FileEvent {
          uri,
          typ: tower_lsp::lsp_types::FileChangeType::CHANGED,
        }],
      })
      .await;

    // Cached config should still hold old values because invalidation was not triggered
    let cfg = server.config.read().await.clone().unwrap();
    assert_eq!(cfg.global.as_ref().and_then(|g| g.indent_size), Some(4));
  }

  #[test]
  fn test_lsp_module_does_not_spawn_fml_child_process() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lsp_rs_path = manifest_dir.join("src/commands/lsp.rs");
    let content = std::fs::read_to_string(lsp_rs_path).unwrap();

    let (prod_code, _) = content.split_once("#[cfg(test)]").unwrap();

    // Verify there are no std::env::current_exe() calls or subprocess re-spawning of fml in production code
    assert!(
      !prod_code.contains("current_exe"),
      "src/commands/lsp.rs production code must not call current_exe() — dispatch in-process instead"
    );
    assert!(
      !prod_code.contains("Command::new"),
      "src/commands/lsp.rs production code must not spawn child processes"
    );
  }

  #[test]
  fn test_serve_status_normal_stop_is_clean() {
    assert_eq!(serve_status(&Ok(())), ExitStatus::Clean);
  }

  #[test]
  fn test_serve_status_handler_panic_is_error() {
    let rt = tokio::runtime::Builder::new_current_thread()
      .build()
      .unwrap();
    let joined = rt.block_on(rt.spawn(async { panic!("handler panic") }));
    assert!(joined.as_ref().is_err_and(tokio::task::JoinError::is_panic));
    assert_eq!(serve_status(&joined), ExitStatus::Error);
  }

  /// Drains the `window/showMessage` notifications the server has sent so far.
  fn drain_show_messages(
    socket: &mut tower_lsp::ClientSocket,
  ) -> Vec<tower_lsp::lsp_types::ShowMessageParams> {
    let mut shown = Vec::new();
    while let Some(Some(request)) =
      futures::FutureExt::now_or_never(futures::StreamExt::next(socket))
    {
      if request.method() == "window/showMessage" {
        let params = request.params().unwrap().clone();
        shown.push(serde_json::from_value(params).unwrap());
      }
    }
    shown
  }

  #[tokio::test]
  async fn test_lsp_invalid_config_at_initialize_reports_and_uses_defaults() {
    let (service, mut socket) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 4\nbogus = 1\n")
      .unwrap();
    let expected = FormalityConfig::load_layered(Some(temp.path()))
      .unwrap_err()
      .to_string();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();
    // Requests reuse the cached defaults and must not re-report.
    let cfg = server.get_or_load_config(Some(temp.path())).await;
    server.get_or_load_config(Some(temp.path())).await;

    let defaults = FormalityConfig::with_defaults();
    let indent = |c: &FormalityConfig| c.global.as_ref()?.indent_size;
    assert_ne!(indent(&cfg), Some(4));
    assert_eq!(indent(&cfg), indent(&defaults));
    let shown = drain_show_messages(&mut socket);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].typ, MessageType::ERROR);
    assert!(shown[0].message.contains(&expected), "{}", shown[0].message);
  }

  #[tokio::test]
  async fn test_lsp_invalid_config_on_reload_reports_and_keeps_previous() {
    let (service, mut socket) = LspService::new(FormalityLsp::new);
    let server = service.inner();
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 4\n").unwrap();

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();
    assert!(drain_show_messages(&mut socket).is_empty());

    std::fs::write(&config_path, "[global]\nindent_size = 8\nbogus = 1\n")
      .unwrap();
    let expected = FormalityConfig::load_layered(Some(temp.path()))
      .unwrap_err()
      .to_string();
    let uri = tower_lsp::lsp_types::Url::from_file_path(&config_path).unwrap();
    server
      .did_change_watched_files(DidChangeWatchedFilesParams {
        changes: vec![tower_lsp::lsp_types::FileEvent {
          uri,
          typ: tower_lsp::lsp_types::FileChangeType::CHANGED,
        }],
      })
      .await;
    let cfg = server.get_or_load_config(Some(temp.path())).await;

    assert_eq!(cfg.global.as_ref().and_then(|g| g.indent_size), Some(4));
    let shown = drain_show_messages(&mut socket);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].typ, MessageType::ERROR);
    assert!(shown[0].message.contains(&expected), "{}", shown[0].message);
  }
}
