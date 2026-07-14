#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const root = path.resolve(__dirname, "..");
const exe = process.platform === "win32" ? "stepcheck.exe" : "stepcheck";
const bin = process.env.STEPCHECK_BIN || path.join(root, "stepcheck", "target", "release", exe);

function fail(message) {
  console.error(`repro check failed: ${message}`);
  process.exit(1);
}

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    fail(`${label}: expected ${expected}, got ${actual}`);
  }
}

if (!fs.existsSync(bin)) {
  fail(`missing stepcheck binary at ${bin}; run cargo build --release --locked first`);
}

const dataflow = fs.readFileSync(path.join(root, "stepcheck", "src", "passes", "dataflow.rs"), "utf8");
if (!/const\s+WIDEN_DEPTH:\s*usize\s*=\s*12\s*;/.test(dataflow)) {
  fail("expected data-flow widening depth WIDEN_DEPTH = 12");
}

const dirs = ["asl", "cncf", "dsl"].map((name) => path.join(root, "corpus", name));
const run = spawnSync(bin, ["fixpoint-stats", ...dirs], {
  cwd: root,
  encoding: "utf8",
  windowsHide: true,
});

if (run.status !== 0) {
  fail(`fixpoint-stats exited ${run.status}: ${run.stderr || run.stdout}`);
}

let report;
try {
  report = JSON.parse(run.stdout);
} catch (err) {
  fail(`fixpoint-stats did not emit JSON: ${err.message}`);
}

const combined = report.combined || {};
assertEqual(combined.files, 262, "combined workflow-file count");
assertEqual(combined.machines, 792, "combined data-flow fixpoint count");
assertEqual(combined.non_converged, 0, "non-converged fixpoints");
assertEqual(combined.max_rounds, 7, "maximum observed fixpoint rounds");

console.log(
  `repro ok: WIDEN_DEPTH=12, files=${combined.files}, fixpoints=${combined.machines}, ` +
    `non_converged=${combined.non_converged}, max_rounds=${combined.max_rounds}`
);