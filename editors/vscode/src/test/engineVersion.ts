import * as fs from "fs";
import * as path from "path";

/**
 * Reads `engines.vscode` from the extension's package.json and strips the
 * range operator (`^`/`~`), returning a bare semver string suitable for
 * `@vscode/test-electron`'s `runTests({ version })` option.
 *
 * This exists so the integration suite's pinned VS Code version can never
 * silently drift from the manifest's declared floor (Fixes #251) — the
 * version passed to `runTests` is derived from `package.json`, not
 * hardcoded, so bumping `engines.vscode` (#247) automatically moves the
 * pinned test version with it.
 *
 * Only a simple `^x.y.z`, `~x.y.z`, or bare `x.y.z` range is accepted; any
 * other range shape (comparator ranges, `x`/`*` wildcards, `>=`, OR ranges,
 * etc.) throws rather than guessing which concrete version to pin to.
 */
export function readEngineVscodeFloor(
  packageJsonPath: string = path.resolve(__dirname, "../../package.json"),
): string {
  const raw = fs.readFileSync(packageJsonPath, "utf8");
  const manifest = JSON.parse(raw) as { engines?: { vscode?: string } };
  const range = manifest.engines?.vscode;

  if (!range) {
    throw new Error(
      `Could not find "engines.vscode" in ${packageJsonPath}`,
    );
  }

  const match = /^([\^~]?)(\d+\.\d+\.\d+)$/.exec(range.trim());
  if (!match) {
    throw new Error(
      `Unsupported "engines.vscode" range "${range}" in ${packageJsonPath}: ` +
        `expected a simple "^x.y.z", "~x.y.z", or "x.y.z" range so the ` +
        `floor version can be derived unambiguously.`,
    );
  }

  return match[2];
}
