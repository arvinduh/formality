import * as assert from "assert";
import * as path from "path";
import {
  COMMANDS,
  DEFAULT_EXECUTABLE,
  SUPPORTED_LANGUAGES,
} from "../../constants";
import {
  formatCommandErrorMessage,
  formatFormatErrorMessage,
  getCommandDescriptor,
  getTempFilePath,
  resolveFmlExecutable,
  resolveWorkspaceFolder,
  shouldAutoSync,
} from "../../helpers";

describe("Helpers & Pure Logic Unit Tests", () => {
  describe("resolveFmlExecutable", () => {
    it("should return default executable when no path provided", () => {
      assert.strictEqual(resolveFmlExecutable(undefined), DEFAULT_EXECUTABLE);
      assert.strictEqual(resolveFmlExecutable(null), DEFAULT_EXECUTABLE);
      assert.strictEqual(resolveFmlExecutable(""), DEFAULT_EXECUTABLE);
      assert.strictEqual(resolveFmlExecutable("   "), DEFAULT_EXECUTABLE);
    });

    it("should trim and return custom configured path", () => {
      assert.strictEqual(
        resolveFmlExecutable("/usr/local/bin/fml"),
        "/usr/local/bin/fml",
      );
      assert.strictEqual(
        resolveFmlExecutable("  C:\\tools\\fml.exe  "),
        "C:\\tools\\fml.exe",
      );
    });
  });

  describe("resolveWorkspaceFolder", () => {
    const mockFolderA = { uri: { fsPath: "/workspace/projectA" } };
    const mockFolderB = { uri: { fsPath: "/workspace/projectB" } };

    it("should resolve folder from target URI if provided", () => {
      const result = resolveWorkspaceFolder({
        uri: { fsPath: "/workspace/projectA/file.rs" },
        workspaceFolders: [mockFolderA, mockFolderB],
        getWorkspaceFolder: (u) =>
          u.fsPath.startsWith("/workspace/projectA") ? mockFolderA : undefined,
      });
      assert.strictEqual(result, "/workspace/projectA");
    });

    it("should resolve folder from active editor URI when target URI is omitted", () => {
      const result = resolveWorkspaceFolder({
        activeEditorUri: { fsPath: "/workspace/projectB/src/lib.rs" },
        workspaceFolders: [mockFolderA, mockFolderB],
        getWorkspaceFolder: (u) =>
          u.fsPath.startsWith("/workspace/projectB") ? mockFolderB : undefined,
      });
      assert.strictEqual(result, "/workspace/projectB");
    });

    it("should fallback to first workspace folder if URI matching fails", () => {
      const result = resolveWorkspaceFolder({
        workspaceFolders: [mockFolderA, mockFolderB],
      });
      assert.strictEqual(result, "/workspace/projectA");
    });

    it("should return undefined when no workspace folders are open", () => {
      const result = resolveWorkspaceFolder({
        workspaceFolders: [],
      });
      assert.strictEqual(result, undefined);
    });
  });

  describe("getTempFilePath", () => {
    it("should generate a sibling temp file in the same directory", () => {
      const target = path.join("project", "src", "main.rs");
      const tempPath = getTempFilePath(target);

      assert.strictEqual(path.dirname(tempPath), path.dirname(target));
      assert.ok(path.basename(tempPath).startsWith(".tmp."));
      assert.ok(path.basename(tempPath).endsWith(".main.rs"));
    });

    it("should respect custom baseDir override", () => {
      const target = path.join("project", "src", "main.rs");
      const tempDir = path.join("tmp", "scratch");
      const tempPath = getTempFilePath(target, tempDir);

      assert.strictEqual(path.dirname(tempPath), tempDir);
      assert.ok(path.basename(tempPath).startsWith(".tmp."));
      assert.ok(path.basename(tempPath).endsWith(".main.rs"));
    });
  });

  describe("formatCommandErrorMessage", () => {
    it("should return friendly message for ENOENT error", () => {
      const msg = formatCommandErrorMessage("fml", { code: "ENOENT" });
      assert.strictEqual(
        msg,
        "'fml' binary not found. Set formality.executablePath in VS Code settings.",
      );
    });

    it("should format stderr when present", () => {
      const msg = formatCommandErrorMessage(
        "fml",
        null,
        "",
        "syntax error in formality.toml",
      );
      assert.strictEqual(
        msg,
        "Formality command failed: syntax error in formality.toml",
      );
    });

    it("should fallback to stdout or error message", () => {
      const msgStd = formatCommandErrorMessage("fml", null, "lint warning", "");
      assert.strictEqual(msgStd, "Formality command failed: lint warning");

      const msgErr = formatCommandErrorMessage("fml", {
        message: "Command failed",
      });
      assert.strictEqual(msgErr, "Formality command failed: Command failed");
    });
  });

  describe("formatFormatErrorMessage", () => {
    it("should return friendly message for ENOENT error", () => {
      const msg = formatFormatErrorMessage("fml", { code: "ENOENT" });
      assert.strictEqual(
        msg,
        "'fml' binary not found. Set formality.executablePath in VS Code settings to the full path of the fml binary.",
      );
    });

    it("should extract the first line of stderr", () => {
      const multiLineStderr =
        "error: parsing failed\n  --> src/main.rs:1:1\n  details...";
      const msg = formatFormatErrorMessage("fml", null, "", multiLineStderr);
      assert.strictEqual(msg, "Formality format error: error: parsing failed");
    });
  });

  describe("shouldAutoSync", () => {
    it("should default to true for undefined or null", () => {
      assert.strictEqual(shouldAutoSync(undefined), true);
      assert.strictEqual(shouldAutoSync(null), true);
      assert.strictEqual(shouldAutoSync(true), true);
    });

    it("should return false only when explicitly disabled", () => {
      assert.strictEqual(shouldAutoSync(false), false);
    });
  });

  describe("Command Descriptors & Constants", () => {
    it("should have correct descriptors for all 5 registered commands", () => {
      const formatDesc = getCommandDescriptor(COMMANDS.FORMAT_WORKSPACE);
      assert.ok(formatDesc);
      assert.deepStrictEqual(formatDesc.args, ["fmt"]);
      assert.strictEqual(formatDesc.showOutput, false);

      const lintDesc = getCommandDescriptor(COMMANDS.LINT_WORKSPACE);
      assert.ok(lintDesc);
      assert.deepStrictEqual(lintDesc.args, ["lint"]);
      assert.strictEqual(lintDesc.showOutput, true);

      const fixDesc = getCommandDescriptor(COMMANDS.LINT_FIX);
      assert.ok(fixDesc);
      assert.deepStrictEqual(fixDesc.args, ["fix"]);
      assert.strictEqual(fixDesc.showOutput, true);

      const syncDesc = getCommandDescriptor(COMMANDS.SYNC);
      assert.ok(syncDesc);
      assert.deepStrictEqual(syncDesc.args, ["sync"]);
      assert.strictEqual(syncDesc.showOutput, false);

      const doctorDesc = getCommandDescriptor(COMMANDS.DOCTOR);
      assert.ok(doctorDesc);
      assert.deepStrictEqual(doctorDesc.args, ["doctor", "--all"]);
      assert.strictEqual(doctorDesc.showOutput, true);
    });

    it("should cover all 15 supported language surfaces", () => {
      assert.strictEqual(SUPPORTED_LANGUAGES.length, 15);
      const expected = [
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
        "java",
        "go",
        "kotlin",
        "javascript",
        "typescript",
      ];
      assert.deepStrictEqual([...SUPPORTED_LANGUAGES], expected);
    });

    it("should return undefined for unregistered command descriptor", () => {
      assert.strictEqual(
        getCommandDescriptor("formality.nonExistent"),
        undefined,
      );
    });
  });
});
