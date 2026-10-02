import { runTests } from "@vscode/test-electron";
import * as path from "path";
import { readEngineVscodeFloor } from "./engineVersion";

async function main() {
  try {
    // The folder containing the Extension Manifest package.json
    // Passed to `--extensionDevelopmentPath`
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");

    // The path to test runner
    // Passed to --extensionTestsPath
    const extensionTestsPath = path.resolve(__dirname, "./suite/index");

    // The fixture workspace folder
    const testWorkspace = path.resolve(
      __dirname,
      "../../test/fixtures/workspace",
    );

    // Pin the integration run to the version declared in `engines.vscode`
    // (Fixes #251) rather than letting @vscode/test-electron default to
    // `stable`. Derived from package.json at test time so this can never
    // silently drift from the manifest's floor — see engineVersion.ts.
    const version = readEngineVscodeFloor();

    // Download that VS Code version, unzip it and run the integration test
    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [testWorkspace, "--disable-extensions"],
      version,
    });
  } catch (err) {
    console.error("Failed to run tests:", err);
    process.exit(1);
  }
}

main();
