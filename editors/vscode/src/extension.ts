import { execFile } from "child_process";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

const SUPPORTED_LANGUAGES = [
  "rust",
  "python",
  "cpp",
  "c",
  "markdown",
  "yaml",
  "json",
  "jsonc",
  "toml",
  "typst",
];

let outputChannel: vscode.OutputChannel;
let client: LanguageClient | undefined;
// Fallback formatting provider, only registered per-language if the LSP
// client fails to start (e.g. an old or missing `fml` binary that doesn't
// support `fml lsp`). Keeps "Format Document" working either way.
let fallbackFormattingProviders: vscode.Disposable[] = [];

export async function activate(context: vscode.ExtensionContext) {
  outputChannel = vscode.window.createOutputChannel("Formality");
  context.subscriptions.push(outputChannel);

  // Status bar item
  const statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  );
  statusBarItem.text = "$(sparkle) Formality";
  statusBarItem.tooltip = "Formality Multi-Language Orchestrator";
  statusBarItem.command = "formality.formatWorkspace";
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  await startLanguageClient(context);

  // Command: Format Entire Workspace
  context.subscriptions.push(
    vscode.commands.registerCommand("formality.formatWorkspace", () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      runFmlCommand(["fmt"], workspaceFolder, "Formatting workspace...");
    }),
  );

  // Command: Lint Entire Workspace
  context.subscriptions.push(
    vscode.commands.registerCommand("formality.lintWorkspace", () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      runFmlCommand(["lint"], workspaceFolder, "Linting workspace...", true);
    }),
  );

  // Command: Lint Entire Workspace with Auto-Fix
  context.subscriptions.push(
    vscode.commands.registerCommand("formality.lintFix", () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      runFmlCommand(
        ["lint", "--fix"],
        workspaceFolder,
        "Linting workspace (auto-fix)...",
        true,
      );
    }),
  );

  // Command: Sync Native Configs
  context.subscriptions.push(
    vscode.commands.registerCommand("formality.sync", () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      runFmlCommand(["sync"], workspaceFolder, "Syncing native configs...");
    }),
  );

  // Command: Run Doctor
  context.subscriptions.push(
    vscode.commands.registerCommand("formality.doctor", () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      runFmlCommand(
        ["doctor", "--all"],
        workspaceFolder,
        "Running Formality toolchain doctor...",
        true,
      );
    }),
  );

  // Watch formality.toml / .formality.toml for changes and creation.
  const configWatcher = vscode.workspace.createFileSystemWatcher(
    "**/{formality.toml,.formality.toml}",
  );

  const onConfigChange = (uri: vscode.Uri) => {
    const autoSync = vscode.workspace
      .getConfiguration("formality")
      .get<boolean>("autoSyncOnConfigSave", true);

    if (autoSync) {
      const workspaceFolder =
        vscode.workspace.getWorkspaceFolder(uri)?.uri.fsPath ||
        path.dirname(uri.fsPath);
      runFmlCommand(
        ["sync"],
        workspaceFolder,
        "Auto-syncing native configs...",
      );
    }
  };

  configWatcher.onDidChange(onConfigChange);
  configWatcher.onDidCreate(onConfigChange);

  context.subscriptions.push(configWatcher);
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

/// Launches `fml lsp` as a language server and connects a `LanguageClient` to
/// it, covering `SUPPORTED_LANGUAGES`. This gives us "Format Document" (via
/// the server's `documentFormattingProvider` capability) and Problems-panel
/// diagnostics (the server publishes them via `textDocument/publishDiagnostics`
/// on open/save) for free, without any extension-side parsing.
///
/// If the client fails to start (e.g. `fml` isn't installed, or is an old
/// version without an `lsp` subcommand), we fall back to the previous
/// execFile-based `DocumentFormattingEditProvider` so formatting still works.
async function startLanguageClient(
  context: vscode.ExtensionContext,
): Promise<void> {
  const exe = getFmlExecutable();

  const serverOptions: ServerOptions = {
    command: exe,
    args: ["lsp"],
    transport: TransportKind.stdio,
    options: { cwd: getWorkspaceRoot() },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: SUPPORTED_LANGUAGES.map((language) => ({
      scheme: "file",
      language,
    })),
    outputChannel,
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/{formality.toml,.formality.toml}",
      ),
    },
  };

  client = new LanguageClient(
    "formality",
    "Formality Language Server",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
    context.subscriptions.push({
      dispose: () => {
        void client?.stop();
      },
    });
  } catch (err) {
    client = undefined;
    outputChannel.appendLine(
      `[Formality] Could not start 'fml lsp': ${
        err instanceof Error ? err.message : String(err)
      }`,
    );
    outputChannel.appendLine(
      "[Formality] Falling back to per-command formatting. Set formality.executablePath " +
        "if 'fml' isn't on PATH, or upgrade fml if it predates the 'lsp' subcommand.",
    );
    registerFallbackFormattingProviders(context);
  }
}

/// Registers the legacy execFile-based formatting provider. Only used when
/// the LSP client could not be started, so "Format Document" keeps working.
function registerFallbackFormattingProviders(
  context: vscode.ExtensionContext,
): void {
  for (const lang of SUPPORTED_LANGUAGES) {
    const provider = vscode.languages.registerDocumentFormattingEditProvider(
      lang,
      {
        provideDocumentFormattingEdits(
          document: vscode.TextDocument,
        ): Promise<vscode.TextEdit[]> {
          return formatDocument(document);
        },
      },
    );
    fallbackFormattingProviders.push(provider);
    context.subscriptions.push(provider);
  }
}

function getFmlExecutable(): string {
  return vscode.workspace
    .getConfiguration("formality")
    .get<string>("executablePath", "fml");
}

function getWorkspaceRoot(): string | undefined {
  if (
    vscode.workspace.workspaceFolders &&
    vscode.workspace.workspaceFolders.length > 0
  ) {
    return vscode.workspace.workspaceFolders[0].uri.fsPath;
  }
  return undefined;
}

function formatDocument(
  document: vscode.TextDocument,
): Promise<vscode.TextEdit[]> {
  return new Promise((resolve, reject) => {
    // Save document first if dirty so fml formats the on-disk content.
    if (document.isDirty) {
      document.save().then(() => doFormat(document, resolve, reject));
    } else {
      doFormat(document, resolve, reject);
    }
  });
}

function doFormat(
  document: vscode.TextDocument,
  resolve: (edits: vscode.TextEdit[]) => void,
  reject: (err: unknown) => void,
) {
  const filePath = document.uri.fsPath;
  const workspaceRoot =
    vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath ||
    path.dirname(filePath);
  const exe = getFmlExecutable();

  execFile(
    exe,
    ["fmt", filePath],
    { cwd: workspaceRoot },
    (error, stdout, stderr) => {
      if (error) {
        const msg = stderr || stdout || error.message;
        // Surface a friendly message if the fml binary was not found.
        const friendlyMsg =
          (error as NodeJS.ErrnoException).code === "ENOENT"
            ? `'${exe}' binary not found. Set formality.executablePath in VS Code settings to the full path of the fml binary.`
            : `Formality format error: ${msg.split("\n")[0]}`;
        outputChannel.appendLine(`[Format Error] ${filePath}:\n${msg}`);
        vscode.window.showErrorMessage(friendlyMsg);
        return reject(error);
      }
      // fml formats the file on disk in-place; return empty edits so VS Code
      // reloads the saved content.
      resolve([]);
    },
  );
}

function runFmlCommand(
  args: string[],
  cwd?: string,
  progressTitle?: string,
  showOutput: boolean = false,
) {
  const exe = getFmlExecutable();

  const task = (progress?: vscode.Progress<{ message?: string }>) => {
    void progress;
    return new Promise<void>((resolve) => {
      execFile(
        exe,
        args,
        { cwd: cwd || process.cwd() },
        (error, stdout, stderr) => {
          outputChannel.appendLine(`\n$ ${exe} ${args.join(" ")}`);
          if (stdout) {
            outputChannel.appendLine(stdout);
          }
          if (stderr) {
            outputChannel.appendLine(stderr);
          }

          if (error) {
            const friendlyMsg =
              (error as NodeJS.ErrnoException).code === "ENOENT"
                ? `'${exe}' binary not found. Set formality.executablePath in VS Code settings.`
                : `Formality command failed: ${stderr || stdout || error.message}`;
            vscode.window.showErrorMessage(friendlyMsg);
          } else {
            vscode.window.setStatusBarMessage(
              `✔ Formality: ${args.join(" ")} complete`,
              3000,
            );
          }

          if (showOutput || error) {
            outputChannel.show(true);
          }
          resolve();
        },
      );
    });
  };

  if (progressTitle) {
    vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: progressTitle,
        cancellable: false,
      },
      task,
    );
  } else {
    task();
  }
}
