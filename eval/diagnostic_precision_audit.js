'use strict';

// Diagnostic-level precision audit for StepCheck findings on the 193-workflow AWS corpus.
//
// Sample native/sound-tier findings for human true/false-positive labelling:
//   node eval/diagnostic_precision_audit.js sample --scope native --k 30 --seed 20260709
//
// Optional: sample all emitted findings, including advisory inference warnings:
//   node eval/diagnostic_precision_audit.js sample --scope all --k 30 --seed 20260709
//
// After filling labels.final.verdict (or matching labels.author/labels.independent verdicts):
//   node eval/diagnostic_precision_audit.js score eval/diagnostic-precision-sample.native.json

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');
const MANIFEST = path.join(ROOT, 'corpus', 'manifest.json');
const POPULATION = path.join(__dirname, 'diagnostic-precision-population.json');

const ADVISORY_CODES = new Set(['SC3001', 'SC4001', 'SC4010']);

function usage() {
  console.error(`Usage:
  node eval/diagnostic_precision_audit.js sample [--scope native|all|advisory] [--k 30] [--seed N]
  node eval/diagnostic_precision_audit.js score <sample.json>

Labels use verdict values: true_positive, false_positive, unclear.`);
  process.exit(2);
}

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) out._.push(a);
    else {
      const key = a.slice(2);
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

function tierFor(code) {
  return ADVISORY_CODES.has(code) ? 'advisory' : 'native';
}

function runStepCheck(file) {
  const args = ['check', '--json', '--infer', file];
  try {
    return JSON.parse(execFileSync(BIN, args, { encoding: 'utf8', maxBuffer: 64 << 20 }));
  } catch (e) {
    const stdout = e.stdout || '';
    if (stdout.trim()) return JSON.parse(stdout);
    throw e;
  }
}

function summarize(items, keyFn) {
  const out = {};
  for (const item of items) {
    const key = keyFn(item);
    out[key] = (out[key] || 0) + 1;
  }
  return Object.fromEntries(Object.entries(out).sort(([a], [b]) => a.localeCompare(b)));
}

function buildPopulation() {
  if (!fs.existsSync(BIN)) throw new Error(`Missing StepCheck release binary: ${BIN}`);
  const manifest = loadJson(MANIFEST);
  const findings = [];
  const perFile = [];

  for (const workflow of manifest) {
    const file = path.join(ROOT, workflow.file);
    const result = runStepCheck(file);
    const diagnostics = result.diagnostics || [];
    perFile.push({
      id: workflow.id,
      file: workflow.file.replace(/\\/g, '/'),
      repo: workflow.repo,
      path: workflow.path.replace(/\\/g, '/'),
      diagnostics: diagnostics.length,
      codes: summarize(diagnostics, d => d.code || 'UNKNOWN'),
    });
    for (let i = 0; i < diagnostics.length; i++) {
      const d = diagnostics[i];
      const code = d.code || 'UNKNOWN';
      findings.push({
        audit_id: `DP-${String(findings.length + 1).padStart(4, '0')}`,
        corpus: 'aws-193',
        workflow_id: workflow.id,
        file: workflow.file.replace(/\\/g, '/'),
        repo: workflow.repo,
        source_path: workflow.path.replace(/\\/g, '/'),
        diagnostic_index: i,
        code,
        tier: tierFor(code),
        severity: d.severity || null,
        state: d.state || null,
        message: d.message || '',
        note: d.note || '',
      });
    }
  }

  const report = {
    generated_at: new Date().toISOString(),
    generated_by: 'eval/diagnostic_precision_audit.js sample',
    command: 'stepcheck check --json --infer <workflow>',
    corpus: { manifest: rel(MANIFEST), workflows: manifest.length },
    diagnostics: {
      total: findings.length,
      by_code: summarize(findings, f => f.code),
      by_tier: summarize(findings, f => f.tier),
    },
    per_file: perFile,
    findings,
  };
  fs.writeFileSync(POPULATION, JSON.stringify(report, null, 2));
  return report;
}

function mulberry32(seed) {
  let a = seed >>> 0;
  return function random() {
    a += 0x6D2B79F5;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function shuffled(xs, random) {
  const ys = xs.slice();
  for (let i = ys.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [ys[i], ys[j]] = [ys[j], ys[i]];
  }
  return ys;
}

function allocate(groups, k) {
  const entries = Object.entries(groups).sort(([a], [b]) => a.localeCompare(b));
  const total = entries.reduce((sum, [, xs]) => sum + xs.length, 0);
  if (total <= k) return Object.fromEntries(entries.map(([code, xs]) => [code, xs.length]));

  const rows = entries.map(([code, xs]) => {
    const ideal = k * xs.length / total;
    return { code, size: xs.length, ideal, target: Math.min(xs.length, Math.max(1, Math.floor(ideal))) };
  });
  while (rows.reduce((s, r) => s + r.target, 0) > k) {
    const candidates = rows.filter(r => r.target > 1).sort((a, b) => (a.ideal - Math.floor(a.ideal)) - (b.ideal - Math.floor(b.ideal)) || a.size - b.size);
    candidates[0].target--;
  }
  while (rows.reduce((s, r) => s + r.target, 0) < k) {
    const candidates = rows.filter(r => r.target < r.size).sort((a, b) => (b.ideal - b.target) - (a.ideal - a.target) || b.size - a.size);
    if (!candidates.length) break;
    candidates[0].target++;
  }
  return Object.fromEntries(rows.map(r => [r.code, r.target]));
}

function scopeFilter(scope) {
  if (scope === 'all') return () => true;
  if (scope === 'native' || scope === 'sound') return f => f.tier === 'native';
  if (scope === 'advisory') return f => f.tier === 'advisory';
  throw new Error(`Unknown scope: ${scope}`);
}

function sample(args) {
  const scope = args.scope || 'native';
  const k = Number(args.k || 30);
  const seed = Number(args.seed || 20260709);
  if (!Number.isFinite(k) || k <= 0) throw new Error(`Bad --k: ${args.k}`);
  if (!Number.isFinite(seed)) throw new Error(`Bad --seed: ${args.seed}`);

  const population = buildPopulation();
  const eligible = population.findings.filter(scopeFilter(scope));
  const byCode = {};
  for (const finding of eligible) (byCode[finding.code] ||= []).push(finding);
  const targets = allocate(byCode, k);
  const random = mulberry32(seed);
  const selected = [];
  for (const code of Object.keys(byCode).sort()) {
    selected.push(...shuffled(byCode[code], random).slice(0, targets[code] || 0));
  }
  selected.sort((a, b) => a.code.localeCompare(b.code) || a.audit_id.localeCompare(b.audit_id));

  const sampleReport = {
    generated_at: new Date().toISOString(),
    generated_by: 'eval/diagnostic_precision_audit.js sample',
    population_file: rel(POPULATION),
    scope,
    k_requested: k,
    seed,
    sampling_unit: 'one emitted diagnostic on one unmodified AWS workflow',
    sampling: 'stratified by diagnostic code; proportional allocation with at least one item per non-empty code stratum when possible',
    population: {
      eligible_findings: eligible.length,
      by_code: summarize(eligible, f => f.code),
      by_tier: summarize(eligible, f => f.tier),
    },
    sample_counts: summarize(selected, f => f.code),
    label_values: ['true_positive', 'false_positive', 'unclear'],
    rubric: {
      general: 'Judge whether the emitted diagnostic is correct for the referenced unmodified workflow and state/path. Label severity separately only in rationale; the verdict is about factual correctness of the finding.',
      SC0007: 'true_positive iff the Choice state has no Default and can therefore fail at runtime when no branch matches.',
      SC0010: 'true_positive iff a machine-level field such as QueryLanguage is incorrectly nested inside States as though it were a state.',
      SC5003: 'true_positive iff two reachable branches can start the same child execution/effectful child workflow concurrently, so the child effects may interfere.',
      SC6001: 'true_positive iff a callback/activity-style wait can remain open without TimeoutSeconds or HeartbeatSeconds bounding it.',
      SC1101: 'true_positive iff the reported JSONPath/document field is definitely absent on all modeled incoming paths.',
      advisory: 'For SC3001/SC4001/SC4010, judge as advisory risk, not a proven soundness error; keep these separate from the native/sound-tier precision number.',
    },
    sample: selected.map(f => ({
      ...f,
      labels: {
        author: { verdict: null, rationale: '' },
        independent: { verdict: null, rationale: '' },
        final: { verdict: null, rationale: '' },
      },
    })),
  };
  const outFile = path.join(__dirname, `diagnostic-precision-sample.${scope}.json`);
  fs.writeFileSync(outFile, JSON.stringify(sampleReport, null, 2));
  console.log(`Population: ${rel(POPULATION)} (${population.findings.length} findings)`);
  console.log(`Sample: ${rel(outFile)} (${selected.length}/${eligible.length} eligible ${scope} findings)`);
  console.log(`Sample counts: ${JSON.stringify(sampleReport.sample_counts)}`);
}

function verdictOf(item) {
  const final = item.labels && item.labels.final && item.labels.final.verdict;
  if (final) return final;
  const a = item.labels && item.labels.author && item.labels.author.verdict;
  const b = item.labels && item.labels.independent && item.labels.independent.verdict;
  if (a && b && a === b) return a;
  return null;
}

function wilson(tp, n, z = 1.959963984540054) {
  if (!n) return [null, null];
  const p = tp / n;
  const denom = 1 + z * z / n;
  const center = (p + z * z / (2 * n)) / denom;
  const half = z * Math.sqrt((p * (1 - p) + z * z / (4 * n)) / n) / denom;
  return [+(Math.max(0, center - half)).toFixed(3), +(Math.min(1, center + half)).toFixed(3)];
}

function score(args) {
  const input = args._[1];
  if (!input) usage();
  const sampleFile = path.resolve(input);
  const sampleReport = loadJson(sampleFile);
  const buckets = { overall: { tp: 0, fp: 0, unclear: 0, unlabelled: 0 } };
  for (const item of sampleReport.sample || []) {
    const verdict = verdictOf(item);
    const keys = ['overall', `code:${item.code}`, `tier:${item.tier}`];
    for (const key of keys) buckets[key] ||= { tp: 0, fp: 0, unclear: 0, unlabelled: 0 };
    for (const key of keys) {
      if (verdict === 'true_positive') buckets[key].tp++;
      else if (verdict === 'false_positive') buckets[key].fp++;
      else if (verdict === 'unclear') buckets[key].unclear++;
      else buckets[key].unlabelled++;
    }
  }
  const scored = {};
  for (const [key, b] of Object.entries(buckets).sort(([a], [b]) => a.localeCompare(b))) {
    const n = b.tp + b.fp;
    scored[key] = {
      true_positive: b.tp,
      false_positive: b.fp,
      unclear: b.unclear,
      unlabelled: b.unlabelled,
      labelled_for_precision: n,
      precision: n ? +(b.tp / n).toFixed(3) : null,
      wilson_ci95: wilson(b.tp, n),
    };
  }
  const out = {
    generated_at: new Date().toISOString(),
    generated_by: 'eval/diagnostic_precision_audit.js score',
    sample_file: rel(sampleFile),
    scope: sampleReport.scope,
    score: scored,
  };
  const outFile = sampleFile.replace(/\.json$/i, '.score.json');
  fs.writeFileSync(outFile, JSON.stringify(out, null, 2));
  console.log(JSON.stringify(scored, null, 2));
  console.log(`-> ${rel(outFile)}`);
}

const args = parseArgs(process.argv.slice(2));
const mode = args._[0];
try {
  if (mode === 'sample') sample(args);
  else if (mode === 'score') score(args);
  else usage();
} catch (e) {
  console.error(e && e.stack ? e.stack : String(e));
  process.exit(1);
}