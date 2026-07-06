#!/usr/bin/env node
"use strict";

// Thin launcher: exec the platform-native `stepcheck` binary fetched by
// install.js, forwarding all CLI arguments, stdio, and the process exit code so
// the npm-installed command behaves exactly like the native binary in CI.

const fs = require("fs");
const { spawnSync } = require("child_process");
const { binaryPath } = require("../install.js");

const bin = binaryPath();
if (!fs.existsSync(bin)) {
  console.error(
    "stepcheck: native binary not found. Reinstall the package, or install " +
      "from source:  cargo install stepcheck"
  );
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`stepcheck: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
