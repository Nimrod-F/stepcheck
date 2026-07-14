'use strict';

// Head-to-head latency benchmark for local StepCheck versus AWS's deployed
// ValidateStateMachineDefinition API over a deterministic sample of ASL files.
//
// Usage:
//   node eval/aws_validate_latency.js --k 50 --seed 20260709
//
// Optional arguments:
//   --manifest corpus/manifest.json
//   --out eval/aws-validate-latency.json
//   --region eu-central-1
//   --profile research-profile
//   --sleep-ms 100

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { performance } = require('perf_hooks');

const ROOT = path.resolve(__dirname, '..');
const DEFAULT_MANIFEST = path.join(ROOT, 'corpus', 'manifest.json');
const DEFAULT_OUT = path.join(__dirname, 'aws-validate-latency.json');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');

function usage() {
  console.error(`Usage:
  node eval/aws_validate_latency.js [--k 50] [--seed N] [--manifest FILE] [--out FILE]
                               [--region REGION] [--profile PROFILE] [--sleep-ms N]
                               [--retries N] [--skip-stepcheck]`);
  process.exit(2);
}

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) out._.push(arg);
    else {
      const key = arg.slice(2);
      const next = argv[i + 1];
      if (!next || next.startsWith('--')) out[key] = true;
      else { out[key] = next; i++; }
    }
  }
  return out;
}

function loadJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function rel(file) {
  return path.relative(ROOT, file).replace(/\\/g, '/');
}

function round1(value) {
  return Math.round(value * 10) / 10;
}

function sleepMs(ms) {
  if (ms <= 0) return;
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function mulberry32(seed) {
  let state = seed >>> 0;
  return function random() {
    state += 0x6D2B79F5;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function sample(items, k, seed) {
  const random = mulberry32(seed);
  const shuffled = items.slice();
  for (let i = shuffled.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
  }
  return shuffled.slice(0, Math.min(k, shuffled.length));
}

function timedExec(command, args, options) {
  const start = performance.now();
  try {
    const out = execFileSync(command, args, {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 64 << 20,
      env: { ...process.env, AWS_PAGER: '' },
      ...options,
    });
    return { ok: true, code: 0, stdout: out, stderr: '', ms: performance.now() - start };
  } catch (error) {
    return {
      ok: false,
      code: error.status == null ? -1 : error.status,
      stdout: error.stdout || '',
      stderr: error.stderr || '',
      ms: performance.now() - start,
    };
  }
}

function awsDefinitionArg(workflow) {
  return `file://${workflow.file.replace(/\\/g, '/')}`;
}

function awsArgs(workflow, args) {
  const out = [
    'stepfunctions',
    'validate-state-machine-definition',
    '--definition', awsDefinitionArg(workflow),
    '--output', 'json',
    '--no-cli-pager',
  ];
  if (args.region) out.push('--region', args.region);
  if (args.profile) out.push('--profile', args.profile);
  return out;
}

function awsValidate(workflow, args, retries) {
  let elapsed = 0;
  let last;
  for (let attempt = 1; attempt <= retries + 1; attempt++) {
    const result = timedExec('aws', awsArgs(workflow, args));
    elapsed += result.ms;
    last = result;
    const text = `${result.stdout}\n${result.stderr}`;
    if (result.ok) {
      let parsed = null;
      try { parsed = JSON.parse(result.stdout); } catch {}
      return {
        ok: true,
        attempts: attempt,
        ms: round1(elapsed),
        result: parsed && parsed.result || null,
        diagnostics: parsed && Array.isArray(parsed.diagnostics) ? parsed.diagnostics.length : null,
        truncated: parsed && Object.prototype.hasOwnProperty.call(parsed, 'truncated') ? parsed.truncated : null,
      };
    }
    if (!/Throttl|Rate exceeded|TooManyRequests|RequestLimitExceeded/i.test(text)) break;
    sleepMs(400 * attempt);
    elapsed += 400 * attempt;
  }
  return {
    ok: false,
    attempts: retries + 1,
    ms: round1(elapsed),
    code: last.code,
    error: `${last.stderr || last.stdout}`.trim().slice(0, 500),
  };
}

function stepcheck(workflow) {
  const file = path.join(ROOT, workflow.file);
  const result = timedExec(BIN, ['check', '--json', '--infer', file]);
  let diagnostics = null;
  const json = result.stdout.trim();
  if (json) {
    try {
      const parsed = JSON.parse(json);
      diagnostics = Array.isArray(parsed.diagnostics) ? parsed.diagnostics.length : null;
    } catch {}
  }
  return {
    ok: result.ok || diagnostics != null,
    code: result.code,
    ms: round1(result.ms),
    diagnostics,
    error: result.ok ? null : `${result.stderr || result.stdout}`.trim().slice(0, 300) || null,
  };
}

function percentile(sortedValues, p) {
  if (!sortedValues.length) return null;
  const index = Math.min(sortedValues.length - 1, Math.max(0, Math.ceil(p * sortedValues.length) - 1));
  return sortedValues[index];
}

function timingSummary(calls) {
  const values = calls.filter(call => call.ok).map(call => call.ms).sort((a, b) => a - b);
  if (!values.length) return { n: 0, errors: calls.length };
  const sum = values.reduce((acc, value) => acc + value, 0);
  return {
    n: values.length,
    errors: calls.length - values.length,
    mean_ms: round1(sum / values.length),
    p50_ms: percentile(values, 0.50),
    p95_ms: percentile(values, 0.95),
    max_ms: values[values.length - 1],
    min_ms: values[0],
  };
}

function commandVersion(command, args) {
  const result = timedExec(command, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  return `${result.stdout || result.stderr}`.trim() || null;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || args._.length) usage();

  const manifestFile = args.manifest ? path.resolve(args.manifest) : DEFAULT_MANIFEST;
  const outFile = args.out ? path.resolve(args.out) : DEFAULT_OUT;
  const k = Number(args.k || 50);
  const seed = Number(args.seed || 20260709);
  const retries = Number(args.retries || 3);
  const sleep = Number(args['sleep-ms'] || 0);

  if (!Number.isFinite(k) || k <= 0) throw new Error(`invalid --k: ${args.k}`);
  if (!Number.isFinite(seed)) throw new Error(`invalid --seed: ${args.seed}`);
  if (!Number.isFinite(retries) || retries < 0) throw new Error(`invalid --retries: ${args.retries}`);
  if (!Number.isFinite(sleep) || sleep < 0) throw new Error(`invalid --sleep-ms: ${args['sleep-ms']}`);
  if (!fs.existsSync(manifestFile)) throw new Error(`missing manifest: ${manifestFile}`);
  if (!args['skip-stepcheck'] && !fs.existsSync(BIN)) throw new Error(`missing StepCheck binary: ${BIN}`);

  const manifest = loadJson(manifestFile);
  const workflows = sample(manifest, k, seed);
  const awsCalls = [];
  const stepcheckCalls = [];
  const details = [];

  for (const workflow of workflows) {
    const aws = awsValidate(workflow, args, retries);
    const local = args['skip-stepcheck'] ? null : stepcheck(workflow);
    awsCalls.push(aws);
    if (local) stepcheckCalls.push(local);
    details.push({
      id: workflow.id,
      file: workflow.file.replace(/\\/g, '/'),
      topLevelStates: workflow.topLevelStates,
      totalStates: workflow.totalStates,
      aws,
      stepcheck: local,
    });
    sleepMs(sleep);
  }

  const report = {
    generated_at: new Date().toISOString(),
    generated_by: 'eval/aws_validate_latency.js',
    corpus: { manifest: rel(manifestFile), workflows: manifest.length },
    sampling: {
      requested_calls: k,
      completed_workflows: workflows.length,
      seed,
      unit: 'one AWS ValidateStateMachineDefinition API call on one ASL definition',
    },
    command: {
      aws: 'aws stepfunctions validate-state-machine-definition --definition file://<workflow> --output json',
      stepcheck: 'stepcheck check --json --infer <workflow>',
    },
    environment: {
      aws_cli: commandVersion('aws', ['--version']),
      node: process.version,
      region: args.region || process.env.AWS_REGION || process.env.AWS_DEFAULT_REGION || null,
      profile: args.profile || process.env.AWS_PROFILE || null,
      stepcheck_binary: rel(BIN),
    },
    summary: {
      aws_validate_state_machine_definition: timingSummary(awsCalls),
      stepcheck_local_process: timingSummary(stepcheckCalls),
    },
    details,
  };

  fs.writeFileSync(outFile, JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report.summary, null, 2));
  console.error(`-> ${rel(outFile)}`);
}

main();