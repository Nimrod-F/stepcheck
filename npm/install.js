#!/usr/bin/env node
"use strict";

// Postinstall step for the `stepcheck` npm wrapper. It downloads the prebuilt
// platform binary that the release workflow (.github/workflows/release.yml)
// attaches to the matching GitHub release, and stores it under `binary/`. The
// `bin/stepcheck.js` launcher then execs it.
//
// The release repository that hosts the binaries is baked in below. Override it
// at install time with STEPCHECK_REPO=owner/repo only if you host them elsewhere.

const fs = require("fs");
const path = require("path");
const https = require("https");

const REPO = process.env.STEPCHECK_REPO || "Nimrod-F/stepcheck";
const VERSION = require("./package.json").version;

// process.platform-process.arch -> release asset name.
const ASSETS = {
  "linux-x64": "stepcheck-linux-x64",
  "linux-arm64": "stepcheck-linux-arm64",
  "darwin-x64": "stepcheck-darwin-x64",
  "darwin-arm64": "stepcheck-darwin-arm64",
  "win32-x64": "stepcheck-win32-x64.exe",
};

function assetName() {
  return ASSETS[`${process.platform}-${process.arch}`];
}

function binDir() {
  return path.join(__dirname, "binary");
}

function binaryPath() {
  const exe = process.platform === "win32" ? "stepcheck.exe" : "stepcheck";
  return path.join(binDir(), exe);
}

function download(url, dest, cb) {
  https
    .get(url, { headers: { "User-Agent": "stepcheck-npm-installer" } }, (res) => {
      if ([301, 302, 307, 308].includes(res.statusCode)) {
        res.resume();
        return download(res.headers.location, dest, cb);
      }
      if (res.statusCode !== 200) {
        res.resume();
        return cb(new Error(`HTTP ${res.statusCode} for ${url}`));
      }
      const file = fs.createWriteStream(dest);
      res.pipe(file);
      file.on("finish", () => file.close(() => cb(null)));
      file.on("error", cb);
    })
    .on("error", cb);
}

function main() {
  const name = assetName();
  if (!name) {
    console.error(
      `stepcheck: no prebuilt binary for ${process.platform}/${process.arch}. ` +
        `Install from source instead:  cargo install stepcheck`
    );
    process.exit(1);
  }
  fs.mkdirSync(binDir(), { recursive: true });
  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${name}`;
  const dest = binaryPath();
  download(url, dest, (err) => {
    if (err) {
      console.error(`stepcheck: could not download the binary (${err.message}).`);
      console.error(
        `Set STEPCHECK_REPO to the release repo, or install from source:  cargo install stepcheck`
      );
      process.exit(1);
    }
    if (process.platform !== "win32") fs.chmodSync(dest, 0o755);
    console.log(`stepcheck ${VERSION} installed (${name}).`);
  });
}

module.exports = { assetName, binaryPath };

if (require.main === module) main();
