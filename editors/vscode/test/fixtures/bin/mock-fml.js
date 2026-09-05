#!/usr/bin/env node

/**
 * Mock `fml` executable for testing VS Code extension commands and LSP integration.
 */
const fs = require("fs");
const path = require("path");

const args = process.argv.slice(2);
const command = args[0];

if (process.env.MOCK_FML_FAIL === "1") {
  console.error("Mock fml error: simulated command failure");
  process.exit(1);
}

if (command === "lsp") {
  // Simple JSON-RPC LSP handler for testing language client startup
  let buffer = Buffer.alloc(0);

  function sendResponse(id, result) {
    const json = JSON.stringify({ jsonrpc: "2.0", id, result });
    const message = `Content-Length: ${Buffer.byteLength(json, "utf8")}\r\n\r\n${json}`;
    process.stdout.write(message);
  }

  process.stdin.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;
      const header = buffer.subarray(0, headerEnd).toString("utf8");
      const match = header.match(/Content-Length: (\d+)/i);
      if (!match) break;
      const contentLength = parseInt(match[1], 10);
      const bodyStart = headerEnd + 4;
      if (buffer.length < bodyStart + contentLength) break;

      const bodyStr = buffer
        .subarray(bodyStart, bodyStart + contentLength)
        .toString("utf8");
      buffer = buffer.subarray(bodyStart + contentLength);

      try {
        const msg = JSON.parse(bodyStr);
        if (msg.method === "initialize") {
          sendResponse(msg.id, {
            capabilities: {
              documentFormattingProvider: true,
              textDocumentSync: 1,
            },
            serverInfo: {
              name: "mock-fml-lsp",
              version: "0.1.0",
            },
          });
        } else if (msg.method === "shutdown") {
          sendResponse(msg.id, null);
        } else if (msg.method === "exit") {
          process.exit(0);
        }
      } catch (e) {
        // ignore parse error
      }
    }
  });

  process.stdin.resume();
} else if (command === "fmt") {
  const targetFile = args[1];
  if (targetFile && fs.existsSync(targetFile)) {
    // Sibling temp file formatting test
    const content = fs.readFileSync(targetFile, "utf8");
    fs.writeFileSync(targetFile, content.trim() + "\n", "utf8");
    console.log(`Formatted ${targetFile}`);
  } else {
    console.log("Formatted workspace files.");
  }
  process.exit(0);
} else if (command === "lint") {
  if (args.includes("--fix")) {
    // Deprecated spelling: real `fml` prints a notice and runs `fml fix`.
    console.error(
      "[DEPRECATED] `fml lint --fix` is deprecated and will be removed in v0.4.0. Use `fml fix` instead",
    );
    console.log("Auto-fixed 0 lint violations. 0 warnings, 0 errors.");
  } else {
    console.log("0 warnings, 0 errors. Workspace clean.");
  }
  process.exit(0);
} else if (command === "fix") {
  if (args.includes("--check")) {
    console.log("0 surfaces would change. Workspace clean.");
  } else {
    console.log("Auto-fixed 0 lint violations, formatted 0 files.");
  }
  process.exit(0);
} else if (command === "sync") {
  console.log(
    "Synced 3 native config files (.rustfmt.toml, .prettierrc, taplo.toml).",
  );
  process.exit(0);
} else if (command === "doctor") {
  console.log(
    "[READY] rust: rustfmt, clippy\n[READY] toml: taplo\n[READY] javascript: biome",
  );
  process.exit(0);
} else {
  console.log(`fml ${args.join(" ")}`);
  process.exit(0);
}
