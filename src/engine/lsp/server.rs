//! The `tower_lsp` handlers for `fml lsp`.
//!
//! Owns the server's state (workspace root and cached config) and maps each
//! request onto one operation from `engine::lsp`. It runs on whatever
//! transport the caller connects; the CLI hosts it on stdio.
//!
//! An invalid config is never silently dropped: each failed load sends one
//! `window/showMessage` error. At initialize the server then uses the built-in
//! defaults; on a failed reload it keeps the previous config.

use std::path;

use log;
use tokio;
use tower_lsp;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types;

use crate::config;
use crate::engine::lsp;
use crate::surfaces::registry;

/// Formats and lints files for an editor, alongside its own language server.
pub struct Server {
  client: tower_lsp::Client,
  /// Workspace root from `initialize`.
  root: tokio::sync::RwLock<Option<path::PathBuf>>,
  /// The config from `initialize`, replaced only by a successful reload.
  config: tokio::sync::RwLock<config::FormalityConfig>,
}

impl Server {
  /// Creates a server that talks to the editor through `client`.
  #[must_use]
  pub fn new(client: tower_lsp::Client) -> Self {
    Self {
      client,
      root: tokio::sync::RwLock::new(None),
      config: tokio::sync::RwLock::new(config::FormalityConfig::with_defaults()),
    }
  }

  /// Loads the layered config for the workspace, reporting a failure to the
  /// editor with what the server uses instead.
  async fn load_config(
    &self,
    fallback: &str,
  ) -> Option<config::FormalityConfig> {
    let root = self.root.read().await.clone();
    match config::FormalityConfig::load_layered(root.as_deref()) {
      Ok((config, _)) => Some(config),
      Err(err) => {
        self
          .client
          .show_message(
            lsp_types::MessageType::ERROR,
            format!("[formality] invalid config, {fallback}: {err}"),
          )
          .await;
        None
      }
    }
  }

  /// The workspace root, or `path`'s directory before `initialize` set one.
  async fn root_for(&self, path: &path::Path) -> path::PathBuf {
    self.root.read().await.clone().unwrap_or_else(|| {
      path
        .parent()
        .map(path::Path::to_path_buf)
        .unwrap_or_default()
    })
  }
}

#[tower_lsp::async_trait]
impl tower_lsp::LanguageServer for Server {
  async fn initialize(
    &self,
    params: lsp_types::InitializeParams,
  ) -> jsonrpc::Result<lsp_types::InitializeResult> {
    let root = params
      .root_uri
      .as_ref()
      .and_then(|u| u.to_file_path().ok())
      .or_else(|| {
        #[expect(
          deprecated,
          reason = "fallback for clients that send root_path, not root_uri"
        )]
        params.root_path.as_ref().map(path::PathBuf::from)
      });
    log::info!("workspace root: {root:?}");
    *self.root.write().await = root;
    if let Some(config) = self.load_config("using the built-in defaults").await
    {
      *self.config.write().await = config;
    }

    Ok(lsp_types::InitializeResult {
      server_info: Some(lsp_types::ServerInfo {
        name: "formality".to_string(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
      }),
      capabilities: lsp_types::ServerCapabilities {
        document_formatting_provider: Some(lsp_types::OneOf::Left(true)),
        // Files are read from disk, so the server needs no document sync.
        text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
          lsp_types::TextDocumentSyncKind::NONE,
        )),
        ..Default::default()
      },
    })
  }

  async fn initialized(&self, _: lsp_types::InitializedParams) {
    let Some(root) = self.root.read().await.clone() else {
      return;
    };
    let config = self.config.read().await.clone();
    let names = tokio::task::spawn_blocking(move || {
      registry::detect_surfaces_smart(&root, &config)
        .iter()
        .map(|s| s.name())
        .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    log::info!("active surfaces: {}", names.join(", "));
  }

  async fn shutdown(&self) -> jsonrpc::Result<()> {
    Ok(())
  }

  async fn formatting(
    &self,
    params: lsp_types::DocumentFormattingParams,
  ) -> jsonrpc::Result<Option<Vec<lsp_types::TextEdit>>> {
    let path = params.text_document.uri.to_file_path().unwrap_or_default();
    let root = self.root_for(&path).await;
    let config = self.config.read().await.clone();
    Ok(lsp::format_file(&root, &path, &config))
  }

  async fn did_save(&self, params: lsp_types::DidSaveTextDocumentParams) {
    let uri = params.text_document.uri;
    let path = uri.to_file_path().unwrap_or_default();
    let root = self.root_for(&path).await;
    let config = self.config.read().await.clone();
    let diagnostics = lsp::diagnose_file(&root, &path, &config);
    self
      .client
      .publish_diagnostics(uri, diagnostics, None)
      .await;
  }

  async fn did_open(&self, params: lsp_types::DidOpenTextDocumentParams) {
    self
      .did_save(lsp_types::DidSaveTextDocumentParams {
        text_document: lsp_types::TextDocumentIdentifier {
          uri: params.text_document.uri,
        },
        text: None,
      })
      .await;
  }

  async fn did_change_watched_files(
    &self,
    params: lsp_types::DidChangeWatchedFilesParams,
  ) {
    let config_changed = params.changes.iter().any(|change| {
      change
        .uri
        .to_file_path()
        .is_ok_and(|p| lsp::is_config_file(&p))
    });
    // A failed reload keeps the previous config: a half-edited file must not
    // throw away a working setup.
    if config_changed
      && let Some(config) =
        self.load_config("keeping the previous config").await
    {
      *self.config.write().await = config;
      log::info!("configuration reloaded");
    }
  }
}
