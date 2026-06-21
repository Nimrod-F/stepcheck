// Scores the human-labelled gold set.
//   node eval/score-human-gold.js                 -> Cohen's kappa between A and B
//   node eval/score-human-gold.js --reconciled    -> also recompute RQ2 accuracy
//
// Reads:  eval/gold-labels-human-A.json, eval/gold-labels-human-B.json
//         (and eval/gold-labels-human.json once you reconcile disagreements)
// Format (same as the template): { "<file>": { "<task>": {idempotent, persistent} } }
//
// Computes:
//  - human inter-annotator Cohen's kappa for idempotent & persistent (over A and B)
//  - if --reconciled: merges the reconciled 13 with the original hand-labelled core
//    (the 10 workflows already in corpus/gold-labels.json) and recomputes inference
//    accuracy/coverage vs `stepcheck infer`, writing corpus/gold-labels.human.json.
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');

function load(p) { return JSON.parse(fs.readFileSync(p, 'utf8')); }
function maybe(p) { try { return load(p); } catch { return null; } }
function cat(v) { return v === true ? 'true' : v === false ? 'false' : 'null'; }

// Cohen's kappa over categories {true,false,null} for one field, across shared tasks.
function kappa(A, B, field) {
  const cats = ['true', 'false', 'null'];
  const idx = Object.fromEntries(cats.map((c, i) => [c, i]));
  const n = cats.length;
  const m = Array.from({ length: n }, () => Array(n).fill(0));
  let total = 0;
  for (const file of Object.keys(A)) {
    if (!B[file]) continue;
    for (const task of Object.keys(A[file])) {
      if (!B[file][task]) continue;
      m[idx[cat(A[file][task][field])]][idx[cat(B[file][task][field])]]++;
      total++;
    }
  }
  if (!total) return { kappa: NaN, n: 0 };
  let po = 0; for (let i = 0; i < n; i++) po += m[i][i];
  po /= total;
  const row = m.map(r => r.reduce((a, b) => a + b, 0));
  const col = cats.map((_, j) => m.reduce((a, r) => a + r[j], 0));
  let pe = 0; for (let i = 0; i < n; i++) pe += (row[i] / total) * (col[i] / total);
  return { kappa: pe === 1 ? 1 : (po - pe) / (1 - pe), n: total, po };
}

const A = maybe(path.join(__dirname, 'gold-labels-human-A.json'));
const B = maybe(path.join(__dirname, 'gold-labels-human-B.json'));
if (A && B) {
  for (const f of ['idempotent', 'persistent']) {
    const k = kappa(A, B, f);
    console.log(`Cohen's kappa (${f}): ${k.kappa.toFixed(3)}  (n=${k.n}, observed agreement=${(k.po * 100).toFixed(1)}%)`);
  }
} else {
  console.log('(Two labeller files A/B not both present -> skipping inter-annotator kappa.\n' +
    ' For a genuine human kappa, have a second person fill gold-labels-human-B.json.)');
}

if (!process.argv.includes('--reconciled')) {
  console.log('\nNext: reconcile (or, single-labeller, just rename your file) into ' +
    'eval/gold-labels-human.json, then re-run with --reconciled to recompute accuracy.');
  process.exit(0);
}

const recon = maybe(path.join(__dirname, 'gold-labels-human.json'));
if (!recon) { console.error('Missing eval/gold-labels-human.json (the reconciled 13).'); process.exit(1); }

// Original hand-labelled core = corpus/gold-labels.json minus the 13 expansion files.
const full = load(path.join(ROOT, 'corpus', 'gold-labels.json')).labels;
const expansionFiles = new Set(Object.keys(recon));
const merged = {};
for (const f of Object.keys(full)) if (!expansionFiles.has(f)) merged[f] = full[f];   // 10 human core
for (const f of Object.keys(recon)) merged[f] = recon[f];                              // 13 reconciled human
fs.writeFileSync(path.join(ROOT, 'corpus', 'gold-labels.human.json'), JSON.stringify({ labels: merged }, null, 2));

// Recompute accuracy/coverage vs the tool (mirrors inference_accuracy.js).
const acc = { idempotent: { c: 0, w: 0, a: 0, t: 0 }, persistent: { c: 0, w: 0, a: 0, t: 0 } };
for (const [file, tasks] of Object.entries(merged)) {
  let preds;
  try { preds = JSON.parse(execFileSync(BIN, ['infer', '--json', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8' })); }
  catch { console.error(`WARN: infer failed on ${file}`); continue; }
  const byName = Object.fromEntries(preds.map(p => [p.state, p]));
  for (const [name, g] of Object.entries(tasks)) {
    const p = byName[name]; if (!p) continue;
    for (const f of ['idempotent', 'persistent']) {
      if (g[f] === null || g[f] === undefined) continue;   // unlabelled gold -> skip
      acc[f].t++;
      const pred = p[f];
      if (pred === null || pred === undefined) acc[f].a++;
      else if (pred === g[f]) acc[f].c++;
      else acc[f].w++;
    }
  }
}
console.log('\nHuman-gold inference accuracy (RQ2):');
for (const f of ['idempotent', 'persistent']) {
  const s = acc[f], decided = s.c + s.w;
  console.log(`  ${f}: accuracy=${decided ? (100 * s.c / decided).toFixed(1) : 0}% on predicted, ` +
    `coverage=${s.t ? (100 * decided / s.t).toFixed(1) : 0}% (labelled=${s.t}, correct=${s.c}, wrong=${s.w}, abstain=${s.a})`);
}

// Warning precision from the human gold: for each SC3001/SC4001/SC4010 warning on a
// gold-labelled task, a true positive iff the gold label confirms the inferred risk.
const prec = { SC3001: { tp: 0, fp: 0 }, SC4001: { tp: 0, fp: 0 }, SC4010: { tp: 0, fp: 0 } };
for (const [file, tasks] of Object.entries(merged)) {
  let diags;
  try { diags = (JSON.parse(execFileSync(BIN, ['check', '--json', '--infer', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8' })).diagnostics) || []; }
  catch { continue; }
  for (const d of diags) {
    if (!prec[d.code]) continue;
    const m = d.message.match(/task '(.+?)'/);
    if (!m) continue;
    const g = tasks[m[1]];
    if (!g) continue; // not gold-covered
    if (d.code === 'SC3001') { if (g.idempotent === null) continue; g.idempotent === false ? prec.SC3001.tp++ : prec.SC3001.fp++; }
    else { if (g.persistent === null) continue; g.persistent === true ? prec[d.code].tp++ : prec[d.code].fp++; }
  }
}
console.log('\nHuman-gold warning precision (gold-covered findings):');
let TP = 0, FP = 0;
for (const c of ['SC3001', 'SC4001', 'SC4010']) {
  const { tp, fp } = prec[c]; const n = tp + fp;
  console.log(`  ${c}: ${tp}/${n}${n ? ` = ${(100 * tp / n).toFixed(0)}%` : ''}`);
  TP += tp; FP += fp;
}
console.log(`  SC3001+SC4001 overall: ${prec.SC3001.tp + prec.SC4001.tp}/${prec.SC3001.tp + prec.SC4001.fp + prec.SC4001.tp + prec.SC3001.fp}` +
  ` = ${(100 * (prec.SC3001.tp + prec.SC4001.tp) / (prec.SC3001.tp + prec.SC3001.fp + prec.SC4001.tp + prec.SC4001.fp)).toFixed(0)}%`);

console.log('\nWrote corpus/gold-labels.human.json — point inference_accuracy.js at it (or keep for the camera-ready table).');
