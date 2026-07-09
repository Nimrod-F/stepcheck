// WS-B: leakage-free evaluation of the inference (advisory) tier.
//
// The inference rules are FROZEN (documented below); the human-labelled gold set
// (23 workflows, double-labelled by annotators A and B) is split into:
//   * DESIGN  (10 original workflows -- informed rule authoring), and
//   * HOLD-OUT (13 expansion workflows in eval/gold-expansion-labels.json --
//     labelled after the rules were frozen and never used to tune them).
//
// We report the property-inference precision / recall / false-omission-rate /
// F1 and per-property inter-annotator Cohen's kappa ON THE HOLD-OUT partition
// (design shown alongside only to demonstrate no over-fitting), plus per-check
// warning precision. The inference tier is presented as an ADVISORY adoption aid,
// not a soundness claim: the sound native tier (SC1101) and the declared tier are
// label-free and carry the paper's verification weight.
//
//   node eval/score-holdout.js            -> eval/holdout-inference.json + summary
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');

// Rule-freeze provenance: the inference rules live in stepcheck/src/annot.rs.
// Recorded blob hash pins the exact ruleset scored here (regenerate with
// `git hash-object stepcheck/src/annot.rs`).
const RULE_FREEZE = { file: 'stepcheck/src/annot.rs', blob: '536271c1164a3d26267b0dcd5db9e732057b6ad4' };

function load(p) { return JSON.parse(fs.readFileSync(p, 'utf8')); }
function cat(v) { return v === true ? 'true' : v === false ? 'false' : 'null'; }

const recon = load(path.join(__dirname, 'gold-labels-human.json'));
const A = load(path.join(__dirname, 'gold-labels-human-A.json'));
const B = load(path.join(__dirname, 'gold-labels-human-B.json'));
const holdoutFiles = new Set(Object.keys(load(path.join(__dirname, 'gold-expansion-labels.json'))));
const allFiles = Object.keys(recon);
const designFiles = allFiles.filter(f => !holdoutFiles.has(f));

// property -> which gold field and which value is the "positive" (risky) class.
// idempotent: positive = NOT idempotent (false)  -> drives SC3001 (unsafe retry)
// persistent: positive = persistent (true)       -> drives SC4001/SC4010 (compensation)
const PROPS = {
  idempotent: { field: 'idempotent', positive: false },
  persistent: { field: 'persistent', positive: true },
};

function inferByFile(file) {
  try {
    const preds = JSON.parse(execFileSync(BIN, ['infer', '--json', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8', maxBuffer: 32 << 20 }));
    return Object.fromEntries(preds.map(p => [p.state, p]));
  } catch { return null; }
}

// 2x2 confusion on decided predictions for one property over a file set.
function confusion(files, prop) {
  const { field, positive } = PROPS[prop];
  const c = { tp: 0, fp: 0, fn: 0, tn: 0, abstain: 0, labelled: 0, tasks: [] };
  for (const file of files) {
    const preds = inferByFile(file);
    if (!preds) continue;
    for (const [name, g] of Object.entries(recon[file] || {})) {
      const gold = g[field];
      if (gold === null || gold === undefined) continue; // gold abstained
      c.labelled++;
      const p = preds[name];
      const pred = p ? p[field] : undefined;
      if (pred === null || pred === undefined) { c.abstain++; continue; }
      const predPos = pred === positive;
      const goldPos = gold === positive;
      if (predPos && goldPos) c.tp++;
      else if (predPos && !goldPos) c.fp++;
      else if (!predPos && goldPos) c.fn++;
      else c.tn++;
      c.tasks.push(predPos === goldPos ? 1 : 0);
    }
  }
  return c;
}

function rates(c) {
  const prec = c.tp + c.fp ? c.tp / (c.tp + c.fp) : null;
  const rec = c.tp + c.fn ? c.tp / (c.tp + c.fn) : null;
  const forate = c.fn + c.tn ? c.fn / (c.fn + c.tn) : null;   // false-omission rate
  const f1 = prec != null && rec != null && prec + rec > 0 ? (2 * prec * rec) / (prec + rec) : null;
  const acc = c.tp + c.tn + c.fp + c.fn ? (c.tp + c.tn) / (c.tp + c.tn + c.fp + c.fn) : null;
  const r = x => (x == null ? null : +x.toFixed(3));
  return { precision: r(prec), recall: r(rec), false_omission_rate: r(forate), f1: r(f1), accuracy: r(acc),
           tp: c.tp, fp: c.fp, fn: c.fn, tn: c.tn, abstain: c.abstain, labelled: c.labelled,
           coverage: c.labelled ? +((c.tp + c.tn + c.fp + c.fn) / c.labelled).toFixed(3) : null };
}

// Bootstrap 95% CI for a rate over per-task correctness (used for accuracy) or,
// for precision/recall, resample the confusion at the task level.
function bootstrapAcc(tasks, B = 3000) {
  if (!tasks.length) return [null, null];
  let seed = 23;
  const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };
  const means = [];
  for (let b = 0; b < B; b++) {
    let s = 0;
    for (let i = 0; i < tasks.length; i++) s += tasks[(rnd() * tasks.length) | 0];
    means.push(s / tasks.length);
  }
  means.sort((a, b) => a - b);
  return [+means[(0.025 * B) | 0].toFixed(3), +means[(0.975 * B) | 0].toFixed(3)];
}

// Per-property Cohen's kappa (A vs B) restricted to a file set.
function kappa(files, field) {
  const cats = ['true', 'false', 'null'], idx = { true: 0, false: 1, null: 2 };
  const m = [[0, 0, 0], [0, 0, 0], [0, 0, 0]]; let total = 0;
  for (const file of files) {
    if (!A[file] || !B[file]) continue;
    for (const task of Object.keys(A[file])) {
      if (!B[file][task]) continue;
      m[idx[cat(A[file][task][field])]][idx[cat(B[file][task][field])]]++; total++;
    }
  }
  if (!total) return { kappa: null, n: 0 };
  let po = 0; for (let i = 0; i < 3; i++) po += m[i][i]; po /= total;
  const row = m.map(r => r[0] + r[1] + r[2]);
  const col = [0, 1, 2].map(j => m[0][j] + m[1][j] + m[2][j]);
  let pe = 0; for (let i = 0; i < 3; i++) pe += (row[i] / total) * (col[i] / total);
  return { kappa: pe === 1 ? 1 : +((po - pe) / (1 - pe)).toFixed(3), n: total, observed_agreement: +(po).toFixed(3) };
}

// Per-check warning precision on a file set (SC3001 <- idempotent, SC4001/4010 <- persistent).
function warningPrecision(files) {
  const prec = { SC3001: { tp: 0, fp: 0 }, SC4001: { tp: 0, fp: 0 }, SC4010: { tp: 0, fp: 0 } };
  for (const file of files) {
    let diags;
    try { diags = JSON.parse(execFileSync(BIN, ['check', '--json', '--infer', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8', maxBuffer: 32 << 20 })).diagnostics || []; }
    catch { continue; }
    for (const d of diags) {
      if (!prec[d.code]) continue;
      const mm = d.message.match(/task '(.+?)'/); if (!mm) continue;
      const g = (recon[file] || {})[mm[1]]; if (!g) continue;
      if (d.code === 'SC3001') { if (g.idempotent == null) continue; g.idempotent === false ? prec.SC3001.tp++ : prec.SC3001.fp++; }
      else { if (g.persistent == null) continue; g.persistent === true ? prec[d.code].tp++ : prec[d.code].fp++; }
    }
  }
  const out = {};
  for (const [c, v] of Object.entries(prec)) out[c] = { tp: v.tp, fp: v.fp, precision: v.tp + v.fp ? +(v.tp / (v.tp + v.fp)).toFixed(3) : null };
  return out;
}

function partition(name, files) {
  const props = {};
  for (const p of Object.keys(PROPS)) {
    const c = confusion(files, p);
    props[p] = { ...rates(c), accuracy_ci95: bootstrapAcc(c.tasks), kappa: kappa(files, PROPS[p].field) };
  }
  return { workflows: files.length, properties: props, warning_precision: warningPrecision(files) };
}

const report = {
  generated_by: 'eval/score-holdout.js',
  rule_freeze: RULE_FREEZE,
  gold: { total_workflows: allFiles.length, design_workflows: designFiles.length, holdout_workflows: holdoutFiles.size,
          annotators: ['A', 'B'], reconciled: 'eval/gold-labels-human.json' },
  reframe: 'Inference is an advisory adoption aid; the paper\'s verification weight is on the label-free sound native tier (SC1101) and the declared tier. Hold-out numbers are the reported inference numbers.',
  holdout: partition('holdout', [...holdoutFiles]),
  design: partition('design', designFiles),
};
fs.writeFileSync(path.join(__dirname, 'holdout-inference.json'), JSON.stringify(report, null, 2));

function show(name, part) {
  console.log(`\n=== ${name} (${part.workflows} workflows) ===`);
  for (const [p, v] of Object.entries(part.properties)) {
    console.log(`  ${p}: P=${v.precision} R=${v.recall} FOR=${v.false_omission_rate} F1=${v.f1} ` +
      `acc=${v.accuracy} ${JSON.stringify(v.accuracy_ci95)} | kappa=${v.kappa.kappa} (n=${v.kappa.n}) ` +
      `[tp${v.tp} fp${v.fp} fn${v.fn} tn${v.tn} abstain${v.abstain}]`);
  }
  const w = part.warning_precision;
  console.log('  warning precision: ' + ['SC3001', 'SC4001', 'SC4010'].map(c => `${c}=${w[c].tp}/${w[c].tp + w[c].fp}`).join(' '));
}
console.log(`Rule-freeze: ${RULE_FREEZE.file} @ ${RULE_FREEZE.blob.slice(0, 10)}`);
show('HOLD-OUT (reported)', report.holdout);
show('DESIGN (reference)', report.design);
console.log('\n-> eval/holdout-inference.json');
