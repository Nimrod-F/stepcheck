// Scores the name-based inference against the hand-labelled gold set.
//   node eval/score-human-gold.js
//
// Gold: corpus/gold-labels.json -- 23 workflows, 172 Task states, each labelled by hand
// for `idempotent` and `persistent` (true / false / null = undecidable) following the
// rubric in eval/GOLD-LABELLING.md. It is a single labelled set: no inter-annotator
// agreement is claimed or computed.
//
// Reports:
//  - inference accuracy (on predicted tasks) and coverage vs `stepcheck infer`;
//  - warning precision: for each SC3001 / SC4001 / SC4010 warning that lands on a gold-labelled
//    task, a true positive iff the gold label confirms the inferred risk.
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release',
  process.platform === 'win32' ? 'stepcheck.exe' : 'stepcheck');

const gold = JSON.parse(fs.readFileSync(path.join(ROOT, 'corpus', 'gold-labels.json'), 'utf8')).labels;

// Accuracy/coverage vs the tool (mirrors inference_accuracy.js).
const acc = { idempotent: { c: 0, w: 0, a: 0, t: 0 }, persistent: { c: 0, w: 0, a: 0, t: 0 } };
for (const [file, tasks] of Object.entries(gold)) {
  let preds;
  try { preds = JSON.parse(execFileSync(BIN, ['infer', '--json', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8' })); }
  catch { console.error(`WARN: infer failed on ${file}`); continue; }
  const byName = Object.fromEntries(preds.map(p => [p.state, p]));
  for (const [name, g] of Object.entries(tasks)) {
    const p = byName[name]; if (!p) continue;
    for (const f of ['idempotent', 'persistent']) {
      if (g[f] === null || g[f] === undefined) continue;   // undecidable in the gold -> skip
      acc[f].t++;
      const pred = p[f];
      if (pred === null || pred === undefined) acc[f].a++;
      else if (pred === g[f]) acc[f].c++;
      else acc[f].w++;
    }
  }
}
console.log('Inference accuracy vs gold:');
for (const f of ['idempotent', 'persistent']) {
  const s = acc[f], decided = s.c + s.w;
  console.log(`  ${f}: accuracy=${decided ? (100 * s.c / decided).toFixed(1) : 0}% on predicted, ` +
    `coverage=${s.t ? (100 * decided / s.t).toFixed(1) : 0}% (labelled=${s.t}, correct=${s.c}, wrong=${s.w}, abstain=${s.a})`);
}

// Warning precision on gold-labelled tasks.
const prec = { SC3001: { tp: 0, fp: 0 }, SC4001: { tp: 0, fp: 0 }, SC4010: { tp: 0, fp: 0 } };
for (const [file, tasks] of Object.entries(gold)) {
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
console.log('\nWarning precision (gold-covered findings):');
let TP = 0, N = 0;
for (const c of ['SC3001', 'SC4001', 'SC4010']) {
  const { tp, fp } = prec[c]; const n = tp + fp;
  console.log(`  ${c}: ${tp}/${n}${n ? ` = ${(100 * tp / n).toFixed(0)}%` : ''}`);
  TP += tp; N += n;
}
const t2 = prec.SC3001.tp + prec.SC4001.tp, n2 = t2 + prec.SC3001.fp + prec.SC4001.fp;
console.log(`  SC3001+SC4001: ${t2}/${n2} = ${(100 * t2 / n2).toFixed(0)}%`);
console.log(`  all three: ${TP}/${N} = ${(100 * TP / N).toFixed(0)}%`);
