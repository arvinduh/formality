/// `fml lsp` — Language Server Protocol passthrough server.
///
/// Architecture
/// ============
/// The formality LSP server runs as a single process that:
///
/// 1. **Accepts** LSP requests from the editor (via stdio).
/// 2. **Detects** which language surfaces are active in the workspace.
/// 3. **Spawns** the appropriate child LSP processes (rust-analyzer, pyright,
///    clangd, …) lazily, on demand.
/// 4. **Routes** each incoming request to the correct child server and
///    multiplexes responses back to the editor.
/// 5. **Intercepts** formatting requests to route them through `fml fmt`
///    instead of the child LSP's formatter, ensuring formality's unified config
///    is always respected.
/// 6. **Injects** `fml lint` diagnostics alongside any diagnostics published
///    by child servers.
/// 7. **Watches** `formality.toml` / `.formality.toml` and runs `fml sync`
///    when the canonical config changes, then notifies the editor to reload
///    affected file diagnostics.
///
/// Child LSP discovery
/// ===================
/// | Surface  | Child LSP binary          | Install source          |
/// |----------|---------------------------|-------------------------|
/// | rust     | `rust-analyzer`           | rustup component add    |
/// | python   | `pyright-langserver`      | npm / pip               |
/// | cpp      | `clangd`                  | apt / brew / llvm.org   |
/// | go       | `gopls`                   | go install               |
/// | typst    | `tinymist` / `typst-lsp`  | cargo / npm             |
/// | markdown | none (diagnostics only)   | —                       |
/// | yaml     | `yaml-language-server`    | npm                     |
/// | json     | `vscode-json-languageserver` | npm                  |
/// | toml     | `taplo lsp`               | cargo / npm             |
///
/// The routing layer is the core of this module. Each child server runs as a
/// subprocess with its own stdin/stdout JSON-RPC channel. The multiplexer
/// assigns monotonically increasing request IDs per-child (to avoid ID
/// collisions across servers) and maps response IDs back to the originating
/// editor request ID.
use colored::Colorize;
use std::path::PathBuf;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// Capabilities that the formality LSP layer adds or overrides on top of what
/// child servers provide. Formatting is always handled by `fml fmt`; all other
/// capabilities are delegated.
const SERVER_NAME: &str = "formality";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Child LSP registry
// ---------------------------------------------------------------------------

/// A description of a child LSP server for one language surface.
#[derive(Debug, Clone)]
pub struct ChildLsp {
  /// Human-readable surface name (matches `LanguageSurface::name()`).
  pub surface: &'static str,
  /// Binary to spawn. Checked with `which` before attempting to start.
  pub binary: &'static str,
  /// Arguments to pass to the child server (e.g. `["--stdio"]`).
  pub args: &'static [&'static str],
  /// Install hint shown in doctor output when the binary is missing.
  pub install_hint: &'static str,
}

/// The canonical set of child LSP servers formality knows about.
///
/// Entries are tried in order; the first binary that exists on PATH wins.
/// Extend this list as new surfaces are added.
pub const CHILD_LSP_REGISTRY: &[ChildLsp] = &[
  ChildLsp {
    surface: "rust",
    binary: "rust-analyzer",
    args: &[],
    install_hint: "rustup component add rust-analyzer",
  },
  ChildLsp {
    surface: "python",
    binary: "pyright-langserver",
    args: &["--stdio"],
    install_hint: "npm install -g pyright  OR  pip install pyright",
  },
  ChildLsp {
    surface: "cpp",
    binary: "clangd",
    args: &[],
    install_hint: "sudo apt install clangd  OR  brew install llvm",
  },
  ChildLsp {
    surface: "go",
    binary: "gopls",
    args: &[],
    install_hint: "go install golang.org/x/tools/gopls@latest",
  },
  ChildLsp {
    surface: "typst",
    binary: "tinymist",
    args: &[],
    install_hint: "cargo binstall tinymist  OR  npm install -g @myriaddreamin/tinymist  OR  brew install tinymist",
  },
  ChildLsp {
    surface: "yaml",
    binary: "yaml-language-server",
    args: &["--stdio"],
    install_hint: "npm install -g yaml-language-server",
  },
  ChildLsp {
    surface: "json",
    binary: "vscode-json-languageserver",
    args: &["--stdio"],
    install_hint: "npm install -g vscode-langservers-extracted",
  },
  ChildLsp {
    surface: "toml",
    binary: "taplo",
    args: &["lsp", "stdio"],
    install_hint: "cargo binstall taplo-cli  OR  npm install -g @taplo/cli  OR  brew install taplo",
  },
];

/// Returns the child LSP descriptor for a given surface, if one is registered.
pub fn child_lsp_for_surface(surface: &str) -> Option<&'static ChildLsp> {
  CHILD_LSP_REGISTRY
    .iter()
    .find(|c| c.surface.eq_ignore_ascii_case(surface))
}

// ---------------------------------------------------------------------------
// LSP server backend
// ---------------------------------------------------------------------------

/// The formality LSP backend.
///
/// The `routing_root` is the workspace root used to detect active surfaces and
/// locate `formality.toml`. The actual child-process management and JSON-RPC
/// multiplexing live behind a `Mutex<RouterState>` so that the async handler
/// methods can mutate shared state safely under `tower-lsp`'s runtime.
pub struct FormalityLsp {
  client: Client,
  /// Workspace root detected at `initialize` time.
  root: tokio::sync::Mutex<Option<PathBuf>>,
}

impl FormalityLsp {
  fn new(client: Client) -> Self {
    Self {
      client,
      root: tokio::sync::Mutex::new(None),
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

    *self.root.lock().await = root;

    Ok(InitializeResult {
      server_info: Some(ServerInfo {
        name: SERVER_NAME.to_string(),
        version: Some(SERVER_VERSION.to_string()),
      }),
      capabilities: ServerCapabilities {
        // formality always handles formatting itself via `fml fmt`.
        document_formatting_provider: Some(OneOf::Left(true)),
        // Range formatting delegates to the child LSP (not yet implemented).
        document_range_formatting_provider: None,
        // Diagnostics are pushed via publishDiagnostics after each save.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
          TextDocumentSyncKind::INCREMENTAL,
        )),
        // Everything else (hover, completion, go-to-definition, …) is
        // handled by child LSPs. The routing layer (not yet wired) will
        // merge and forward those capabilities.
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

    // Detect active surfaces and log which child LSPs are available.
    if let Some(root) = self.root.lock().await.clone() {
      let config = crate::config::FormalityConfig::load_layered(Some(&root))
        .map(|(c, _)| c)
        .unwrap_or_else(|_| crate::config::FormalityConfig::with_defaults());

      let detected = crate::surfaces::detect_surfaces_smart(&root, &config);
      for surface in &detected {
        match child_lsp_for_surface(surface.name()) {
          Some(child) if which::which(child.binary).is_ok() => {
            self
              .client
              .log_message(
                MessageType::INFO,
                format!(
                  "[formality] surface '{}' → child LSP '{}'",
                  surface.name(),
                  child.binary
                ),
              )
              .await;
          }
          Some(child) => {
            self
              .client
              .log_message(
                MessageType::WARNING,
                format!(
                  "[formality] surface '{}': child LSP '{}' not found — {}",
                  surface.name(),
                  child.binary,
                  child.install_hint
                ),
              )
              .await;
          }
          None => {
            self
              .client
              .log_message(
                MessageType::LOG,
                format!(
                  "[formality] surface '{}': no child LSP registered (diagnostics only)",
                  surface.name()
                ),
              )
              .await;
          }
        }
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
      path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
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

    // Run `fml fmt <file>` in-place.
    let result = std::process::Command::new("fml")
      .arg("fmt")
      .arg(&path)
      .current_dir(&root)
      .output();

    match result {
      Ok(out) if out.status.success() => {
        let after = std::fs::read_to_string(&path).unwrap_or_default();
        if before == after {
          // No changes needed.
          return Ok(Some(vec![]));
        }
        // Return a single whole-document replacement edit.
        let line_count = before.lines().count() as u32;
        let last_col =
          before.lines().last().map(|l| l.len() as u32).unwrap_or(0);
        Ok(Some(vec![TextEdit {
          range: Range {
            start: Position {
              line: 0,
              character: 0,
            },
            end: Position {
              line: line_count,
              character: last_col,
            },
          },
          new_text: after,
        }]))
      }
      Ok(out) => {
        let stderr = String::from_utf8_lossy(&out.stderr);
        self
          .client
          .log_message(
            MessageType::ERROR,
            format!("[formality] fml fmt failed:\n{stderr}"),
          )
          .await;
        Ok(None)
      }
      Err(e) => {
        self
          .client
          .log_message(
            MessageType::ERROR,
            format!("[formality] could not run fml: {e}"),
          )
          .await;
        Ok(None)
      }
    }
  }

  // -------------------------------------------------------------------------
  // Document sync — used to trigger `fml lint` diagnostics on save.
  // -------------------------------------------------------------------------

  async fn did_save(&self, params: DidSaveTextDocumentParams) {
    let path = params.text_document.uri.to_file_path().unwrap_or_default();
    let root = self.root.lock().await.clone().unwrap_or_else(|| {
      path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
    });
    let uri = params.text_document.uri.clone();

    // Run `fml lint <file>` and publish diagnostics.
    // TODO: parse structured output per-tool (ruff --output-format=json,
    //       clippy --message-format=json, etc.) and map to LSP Diagnostics.
    //       For now we clear diagnostics on save to avoid stale markers.
    let result = std::process::Command::new("fml")
      .arg("lint")
      .arg(&path)
      .current_dir(&root)
      .output();

    let diagnostics = match result {
      Ok(out) if out.status.success() => vec![],
      Ok(_out) => {
        // Non-zero exit = violations found. Until we parse per-tool JSON,
        // publish a single workspace-level note pointing users to the output
        // channel rather than individual squiggles.
        vec![Diagnostic {
          range: Range::default(),
          severity: Some(DiagnosticSeverity::WARNING),
          source: Some("formality".to_string()),
          message: "fml lint found issues — see the Formality output channel."
            .to_string(),
          ..Default::default()
        }]
      }
      Err(e) => {
        self
          .client
          .log_message(
            MessageType::ERROR,
            format!("[formality] fml lint error: {e}"),
          )
          .await;
        vec![]
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
}

// ---------------------------------------------------------------------------
// Entry point called from lib.rs / Commands::Lsp
// ---------------------------------------------------------------------------

/// Start the formality LSP server on stdio.
///
/// Blocks until the client disconnects. Intended to be called from `fml lsp`.
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
