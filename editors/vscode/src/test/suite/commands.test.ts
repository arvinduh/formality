import * as assert from "assert";
import * as path from "path";
import * as vscode from "vscode";
import { COMMANDS } from "../../constants";

describe("VS Code Extension Integration & Commands Test Suite", () => {
  const extensionId = "arvinduh.formality";
  const mockExePath =
    process.platform === "win32"
      ? path.resolve(__dirname, "../../../test/fixtures/bin/mock-fml.cmd")
      : path.resolve(__dirname, "../../../test/fixtures/bin/mock-fml");

  before(async () => {
    // Configure formality.executablePath to use the mock fml binary
    const config = vscode.workspace.getConfiguration("formality");
    await config.update(
      "executablePath",
      mockExePath,
      vscode.ConfigurationTarget.Global,
    );

    // Ensure extension is activated
    const ext = vscode.extensions.getExtension(extensionId);
    if (ext && !ext.isActive) {
      await ext.activate();
    }
  });

  after(async () => {
    // Reset configuration
    const config = vscode.workspace.getConfiguration("formality");
    await config.update(
      "executablePath",
      undefined,
      vscode.ConfigurationTarget.Global,
    );
  });

  it("should be present and activated in VS Code extensions", () => {
    const ext = vscode.extensions.getExtension(extensionId);
    assert.ok(ext, `Extension ${extensionId} should be present`);
    assert.strictEqual(
      ext.isActive,
      true,
      `Extension ${extensionId} should be active`,
    );
  });

  it("should register all 5 formality commands matching package.json", async () => {
    const allCommands = await vscode.commands.getCommands(true);

    const expectedCommands = [
      "formality.formatWorkspace",
      "formality.lintWorkspace",
      "formality.lintFix",
      "formality.sync",
      "formality.doctor",
    ];

    for (const cmd of expectedCommands) {
      assert.ok(
        allCommands.includes(cmd),
        `Command '${cmd}' should be registered in VS Code`,
      );
    }
  });

  it("should execute formality.formatWorkspace without error", async () => {
    await vscode.commands.executeCommand("formality.formatWorkspace");
  });

  it("should execute formality.lintWorkspace without error", async () => {
    await vscode.commands.executeCommand("formality.lintWorkspace");
  });

  it("should execute formality.lintFix without error", async () => {
    await vscode.commands.executeCommand("formality.lintFix");
  });

  it("should execute formality.sync without error", async () => {
    await vscode.commands.executeCommand("formality.sync");
  });

  it("should execute formality.doctor without error", async () => {
    await vscode.commands.executeCommand("formality.doctor");
  });

  it("should handle command failure gracefully when executable is missing", async () => {
    const config = vscode.workspace.getConfiguration("formality");
    await config.update(
      "executablePath",
      "non_existent_fml_binary_12345",
      vscode.ConfigurationTarget.Global,
    );

    // Executing command should not throw unhandled exception
    await vscode.commands.executeCommand(COMMANDS.DOCTOR);

    // Restore mock executable
    await config.update(
      "executablePath",
      mockExePath,
      vscode.ConfigurationTarget.Global,
    );
  });
});
