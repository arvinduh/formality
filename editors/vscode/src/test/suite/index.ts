import { glob } from "glob";
import Mocha from "mocha";
import * as path from "path";

export async function run(): Promise<void> {
  // Create the mocha test instance
  const mocha = new Mocha({
    ui: "bdd",
    color: true,
    timeout: 20000,
  });

  const testsRoot = path.resolve(__dirname, "..");

  const files = await glob("**/*.test.js", { cwd: testsRoot });

  // Add files to the test suite
  files.forEach((f) => {
    // Only include suite tests in the VS Code extension host runner
    if (f.startsWith("suite" + path.sep) || f.startsWith("suite/")) {
      mocha.addFile(path.resolve(testsRoot, f));
    }
  });

  return new Promise((resolve, reject) => {
    try {
      mocha.run((failures) => {
        if (failures > 0) {
          reject(new Error(`${failures} tests failed.`));
        } else {
          resolve();
        }
      });
    } catch (err) {
      reject(err);
    }
  });
}
