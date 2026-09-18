import * as assert from "assert";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { readEngineVscodeFloor } from "../engineVersion";

describe("readEngineVscodeFloor", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "engine-version-test-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function writePackageJson(engineVscode: string | undefined): string {
    const pkgPath = path.join(tmpDir, "package.json");
    const manifest =
      engineVscode === undefined
        ? { name: "formality" }
        : { name: "formality", engines: { vscode: engineVscode } };
    fs.writeFileSync(pkgPath, JSON.stringify(manifest), "utf8");
    return pkgPath;
  }

  it("strips a caret range to the bare version", () => {
    const pkgPath = writePackageJson("^1.91.0");
    assert.strictEqual(readEngineVscodeFloor(pkgPath), "1.91.0");
  });

  it("strips a tilde range to the bare version", () => {
    const pkgPath = writePackageJson("~1.91.0");
    assert.strictEqual(readEngineVscodeFloor(pkgPath), "1.91.0");
  });

  it("accepts a bare version with no range operator", () => {
    const pkgPath = writePackageJson("1.91.0");
    assert.strictEqual(readEngineVscodeFloor(pkgPath), "1.91.0");
  });

  it("throws on an unsupported range shape", () => {
    const pkgPath = writePackageJson(">=1.91.0");
    assert.throws(
      () => readEngineVscodeFloor(pkgPath),
      /Unsupported "engines\.vscode" range/,
    );
  });

  it("throws when engines.vscode is missing", () => {
    const pkgPath = writePackageJson(undefined);
    assert.throws(
      () => readEngineVscodeFloor(pkgPath),
      /Could not find "engines\.vscode"/,
    );
  });
});
