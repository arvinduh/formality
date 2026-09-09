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
//!    `did_change_watched_files` and invalidates the cached configuration
//!    when the canonical config changes.
//!
//! This server is a formatting and diagnostics provider, meant to run
//! *alongside* the user's existing language servers (rust-analyzer, pyright,
//! clangd, …) — it does not spawn, proxy, or route requests to them. See
//! `README.md`'s "Editor setup" section for how to wire `fml lsp` in
//! alongside a primary language server.
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
  Diagnostic, DiagnosticSeverity, DidChangeWatchedFilesParams,
  DidOpenTextDocumentParams, DidSaveTextDocumentParams,
  DocumentFormattingParams, InitializeParams, InitializeResult,
  InitializedParams, MessageType, OneOf, Position, Range, ServerCapabilities,
  ServerInfo, TextDocumentIdentifier, TextDocumentSyncCapability,
  TextDocumentSyncKind, TextEdit,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::config::FormalityConfig;

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
  /// Cached formality configuration, loaded at initialize/initialized time
  /// and invalidated when `formality.toml` / `.formality.toml` changes.
  config: Arc<tokio::sync::RwLock<Option<FormalityConfig>>>,
}

impl FormalityLsp {
  /// Creates a new [`FormalityLsp`] instance with the provided client handle.
  #[must_use]
  pub fn new(client: Client) -> Self {
    Self {
      client,
      root: tokio::sync::Mutex::new(None),
      config: Arc::new(tokio::sync::RwLock::new(None)),
    }
  }

  /// Returns the cached configuration, or loads and caches it if not yet present.
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
    let loaded = FormalityConfig::load_layered(root)
      .map_or_else(|_| FormalityConfig::with_defaults(), |(c, _)| c);
    *lock = Some(loaded.clone());
    loaded
  }

  /// Returns a clone of the cached config, if present.
  pub async fn cached_config(&self) -> Option<FormalityConfig> {
    self.config.read().await.clone()
  }

  /// Invalidates the cached configuration.
  pub async fn invalidate_config(&self) {
    *self.config.write().await = None;
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

    // Cache resolved config at initialize time.
    let config = FormalityConfig::load_layered(root.as_deref())
      .map_or_else(|_| FormalityConfig::with_defaults(), |(c, _)| c);
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
      path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
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

    let status = crate::commands::fmt::run_fmt(
      &root,
      &config,
      false,
      false,
      false,
      vec![],
      false,
      vec![path.clone()],
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
      path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
    });
    let uri = params.text_document.uri.clone();
    let config = self.get_or_load_config(Some(&root)).await;

    // For surfaces with structured-output support wired up (rust via
    // clippy, python via ruff — see `lsp_diagnostics`), publish one
    // `Diagnostic` per real violation with correct range/message/severity —
    // but only when that structured tool actually ran. `diagnostics_for_file`
    // returns `None` both for surfaces with no structured parser at all and
    // for ones whose parser couldn't run this time (binary missing, no
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
      let status = crate::commands::lint::run_lint(
        &root,
        &config,
        false,
        false,
        vec![],
        false,
        vec![path.clone()],
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
      self.invalidate_config().await;
      let root = self.root.lock().await.clone();
      let _ = self.get_or_load_config(root.as_deref()).await;
      self
        .client
        .log_message(
          MessageType::INFO,
          "[formality] configuration invalidated and reloaded",
        )
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

/// Start the formality LSP server on stdio.
///
/// Blocks until the client disconnects. Intended to be called from `fml lsp`.
///
/// # Panics
///
/// Panics if the underlying Tokio runtime fails to initialize.
// Takes owned Option<PathBuf> from the top-level CLI command runner for uniform handler signature.
#[allow(clippy::needless_pass_by_value)]
pub fn run_lsp_server(root: Option<std::path::PathBuf>) {
  // Print a startup banner to stderr (not stdout — that's the LSP channel).
  eprintln!(
    "{} LSP server starting (stdio transport, v{SERVER_VERSION})",
    "formality".cyan().bold()
  );
  if let Some(ref r) = root {
    eprintln!("  workspace root: {}", r.display());
  }

  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  rt.block_on(async {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(FormalityLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;
  });
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
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

    assert!(server.cached_config().await.is_none());

    server
      .initialize(InitializeParams {
        root_uri: tower_lsp::lsp_types::Url::from_file_path(temp.path()).ok(),
        ..Default::default()
      })
      .await
      .unwrap();

    let cached = server.cached_config().await;
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

    let cfg_before = server.cached_config().await.unwrap();
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

    let cfg_after = server.cached_config().await.unwrap();
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

    let cfg_before = server.cached_config().await.unwrap();
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

    let cfg_after = server.cached_config().await.unwrap();
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
    let cfg = server.cached_config().await.unwrap();
    assert_eq!(cfg.global.as_ref().and_then(|g| g.indent_size), Some(4));
  }

  #[test]
  fn test_lsp_module_does_not_spawn_fml_child_process() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
}
