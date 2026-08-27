import { execFile } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import {
  COMMAND_DESCRIPTORS,
  COMMANDS,
  SUPPORTED_LANGUAGES,
} from "./constants";
import {
  formatCommandErrorMessage,
  formatFormatErrorMessage,
  getExecOptions,
  getTempFilePath,
  resolveFmlExecutable,
  resolveWorkspaceFolder,
  shouldAutoSync,
} from "./helpers";

export { COMMAND_DESCRIPTORS, COMMANDS, SUPPORTED_LANGUAGES };

let outputChannel: vscode.LogOutputChannel;
let client: LanguageClient | undefined;
// Fallback formatting provider, only registered per-language if the LSP
// client fails to start (e.g. an old or missing `fml` binary that doesn't
// support `fml lsp`). Keeps "Format Document" working either way.
const fallbackFormattingProviders: vscode.Disposable[] = [];

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  outputChannel = vscode.window.createOutputChannel("Formality", {
    log: true,
  });
  context.subscriptions.push(outputChannel);

  // Status bar item
  const statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100,
  );
  statusBarItem.text = "$(sparkle) Formality";
  statusBarItem.tooltip = "Formality Multi-Language Orchestrator";
  statusBarItem.command = COMMANDS.FORMAT_WORKSPACE;
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  await startLanguageClient(context);

  // Command: Format Entire Workspace
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMANDS.FORMAT_WORKSPACE, () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      const desc = COMMAND_DESCRIPTORS[COMMANDS.FORMAT_WORKSPACE];
      return runFmlCommand(
        desc.args,
        workspaceFolder,
        desc.title,
        desc.showOutput,
      );
    }),
  );

  // Command: Lint Entire Workspace
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMANDS.LINT_WORKSPACE, () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      const desc = COMMAND_DESCRIPTORS[COMMANDS.LINT_WORKSPACE];
      return runFmlCommand(
        desc.args,
        workspaceFolder,
        desc.title,
        desc.showOutput,
      );
    }),
  );

  // Command: Lint Entire Workspace with Auto-Fix
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMANDS.LINT_FIX, () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      const desc = COMMAND_DESCRIPTORS[COMMANDS.LINT_FIX];
      return runFmlCommand(
        desc.args,
        workspaceFolder,
        desc.title,
        desc.showOutput,
      );
    }),
  );

  // Command: Sync Native Configs
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMANDS.SYNC, () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      const desc = COMMAND_DESCRIPTORS[COMMANDS.SYNC];
      return runFmlCommand(
        desc.args,
        workspaceFolder,
        desc.title,
        desc.showOutput,
      );
    }),
  );

  // Command: Run Doctor
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMANDS.DOCTOR, () => {
      const workspaceFolder = getWorkspaceRoot();
      if (!workspaceFolder) {
        vscode.window.showWarningMessage("No workspace folder open.");
        return;
      }
      const desc = COMMAND_DESCRIPTORS[COMMANDS.DOCTOR];
      return runFmlCommand(
        desc.args,
        workspaceFolder,
        desc.title,
        desc.showOutput,
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

    if (shouldAutoSync(autoSync)) {
      const workspaceFolder = getWorkspaceRoot(uri) || path.dirname(uri.fsPath);
      return runFmlCommand(
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
export async function startLanguageClient(
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
export function registerFallbackFormattingProviders(
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

export function getFmlExecutable(): string {
  const rawPath = vscode.workspace
    .getConfiguration("formality")
    .get<string>("executablePath", "fml");
  return resolveFmlExecutable(rawPath);
}

export function getWorkspaceRoot(uri?: vscode.Uri): string | undefined {
  return resolveWorkspaceFolder({
    uri,
    activeEditorUri: vscode.window.activeTextEditor?.document.uri,
    workspaceFolders: vscode.workspace.workspaceFolders,
    getWorkspaceFolder: (u) =>
      vscode.workspace.getWorkspaceFolder(u as vscode.Uri),
  });
}

export function formatDocument(
  document: vscode.TextDocument,
): Promise<vscode.TextEdit[]> {
  return doFormat(document);
}

export async function doFormat(
  document: vscode.TextDocument,
): Promise<vscode.TextEdit[]> {
  if (document.uri.scheme !== "file") {
    return [];
  }

  const filePath = document.uri.fsPath;
  const workspaceRoot =
    getWorkspaceRoot(document.uri) || path.dirname(filePath);
  const exe = getFmlExecutable();
  const originalText = document.getText();

  // Write the in-memory content to a temporary sibling file so fml formats
  // the exact buffer state and resolves configuration from the same directory
  // tree, without triggering formatOnSave loops or buffer clobbering races.
  let tempFilePath = getTempFilePath(filePath);

  try {
    try {
      await fs.promises.writeFile(tempFilePath, originalText, "utf8");
    } catch {
      // Fallback to os.tmpdir() if the document directory is not writable.
      tempFilePath = getTempFilePath(filePath, os.tmpdir());
      await fs.promises.writeFile(tempFilePath, originalText, "utf8");
    }

    const formattedText = await new Promise<string>((resolve, reject) => {
      execFile(
        exe,
        ["fmt", tempFilePath],
        getExecOptions(exe, workspaceRoot),
        async (error, stdout, stderr) => {
          if (error) {
            const friendlyMsg = formatFormatErrorMessage(
              exe,
              error as NodeJS.ErrnoException,
              stdout,
              stderr,
            );
            const msg = stderr || stdout || error.message;
            outputChannel.appendLine(`[Format Error] ${filePath}:\n${msg}`);
            vscode.window.showErrorMessage(friendlyMsg);
            return reject(error);
          }

          try {
            const result = await fs.promises.readFile(tempFilePath, "utf8");
            resolve(result);
          } catch (readErr) {
            reject(readErr);
          }
        },
      );
    });

    if (formattedText === originalText) {
      return [];
    }

    const fullRange = new vscode.Range(
      document.positionAt(0),
      document.positionAt(originalText.length),
    );

    return [vscode.TextEdit.replace(fullRange, formattedText)];
  } catch {
    return [];
  } finally {
    try {
      await fs.promises.unlink(tempFilePath);
    } catch {
      // Ignore cleanup error if temp file does not exist.
    }
  }
}

export function runFmlCommand(
  args: string[],
  cwd?: string,
  progressTitle?: string,
  showOutput: boolean = false,
): Promise<void> {
  const exe = getFmlExecutable();

  const task = (progress?: vscode.Progress<{ message?: string }>) => {
    void progress;
    return new Promise<void>((resolve) => {
      execFile(exe, args, getExecOptions(exe, cwd), (error, stdout, stderr) => {
        outputChannel.appendLine(`\n$ ${exe} ${args.join(" ")}`);
        if (stdout) {
          outputChannel.appendLine(stdout);
        }
        if (stderr) {
          outputChannel.appendLine(stderr);
        }

        if (error) {
          const friendlyMsg = formatCommandErrorMessage(
            exe,
            error as NodeJS.ErrnoException,
            stdout,
            stderr,
          );
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
      });
    });
  };

  if (progressTitle) {
    return Promise.resolve(
      vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: progressTitle,
          cancellable: false,
        },
        task,
      ),
    );
  } else {
    return task();
  }
}
