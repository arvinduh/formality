import * as path from "path";
import {
  COMMAND_DESCRIPTORS,
  CommandDescriptor,
  DEFAULT_EXECUTABLE,
} from "./constants";

export function resolveFmlExecutable(configuredPath?: string | null): string {
  if (configuredPath && typeof configuredPath === "string") {
    const trimmed = configuredPath.trim();
    if (trimmed.length > 0) {
      return trimmed;
    }
  }
  return DEFAULT_EXECUTABLE;
}

export interface WorkspaceFolderLike {
  uri: { fsPath: string };
}

export interface UriLike {
  fsPath: string;
}

export function resolveWorkspaceFolder(options: {
  uri?: UriLike | null;
  activeEditorUri?: UriLike | null;
  workspaceFolders?: readonly WorkspaceFolderLike[] | null;
  getWorkspaceFolder?: (uri: UriLike) => WorkspaceFolderLike | undefined;
}): string | undefined {
  const { uri, activeEditorUri, workspaceFolders, getWorkspaceFolder } =
    options;

  if (uri && getWorkspaceFolder) {
    const folder = getWorkspaceFolder(uri);
    if (folder) {
      return folder.uri.fsPath;
    }
  }

  if (activeEditorUri && getWorkspaceFolder) {
    const folder = getWorkspaceFolder(activeEditorUri);
    if (folder) {
      return folder.uri.fsPath;
    }
  }

  if (workspaceFolders && workspaceFolders.length > 0) {
    return workspaceFolders[0].uri.fsPath;
  }

  return undefined;
}

export function getTempFilePath(filePath: string, baseDir?: string): string {
  const dir = baseDir || path.dirname(filePath);
  const randomSuffix = Math.random().toString(36).slice(2);
  return path.join(
    dir,
    `.tmp.${Date.now()}.${randomSuffix}.${path.basename(filePath)}`,
  );
}

export function formatCommandErrorMessage(
  exe: string,
  error?: { code?: string; message?: string } | null,
  stdout?: string,
  stderr?: string,
): string {
  if (error?.code === "ENOENT") {
    return `'${exe}' binary not found. Set formality.executablePath in VS Code settings.`;
  }
  const msg = (stderr || stdout || error?.message || "Unknown error").trim();
  return `Formality command failed: ${msg}`;
}

export function formatFormatErrorMessage(
  exe: string,
  error?: { code?: string; message?: string } | null,
  stdout?: string,
  stderr?: string,
): string {
  if (error?.code === "ENOENT") {
    return `'${exe}' binary not found. Set formality.executablePath in VS Code settings to the full path of the fml binary.`;
  }
  const rawMsg = (stderr || stdout || error?.message || "Unknown error").trim();
  const firstLine = rawMsg.split("\n")[0].trim();
  return `Formality format error: ${firstLine}`;
}

export function shouldAutoSync(
  autoSyncSetting: boolean | undefined | null,
): boolean {
  return autoSyncSetting !== false;
}

export function getCommandDescriptor(
  commandName: string,
): CommandDescriptor | undefined {
  return COMMAND_DESCRIPTORS[commandName];
}

export function getExecOptions(
  exe: string,
  cwd?: string,
): { cwd: string; shell: boolean } {
  const isWindows = process.platform === "win32";
  const isCmdOrBat =
    exe.toLowerCase().endsWith(".cmd") || exe.toLowerCase().endsWith(".bat");
  return {
    cwd: cwd || process.cwd(),
    shell: isWindows && isCmdOrBat,
  };
}
