// Hold-out evaluation of the name-based inference (the advisory tier).
//
// The inference rules were frozen before the last 13 gold workflows were labelled.
// The hand-labelled gold set (corpus/gold-labels.json, 23 workflows) is split into:
//   * DESIGN   (10 original workflows -- these informed rule authoring), and
//   * HOLD-OUT (13 workflows listed in eval/holdout-workflows.json -- labelled after
//     the freeze and never used to write or tune a rule).
//
// We report property-inference precision / recall / false-omission rate / F1 and
// accuracy (bootstrap 95% CI) on the hold-out partition, the design partition
// alongside for comparison, and per-check warning precision.
//
//   node eval/score-holdout.js            -> eval/holdout-inference.json + summary
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release',
  process.platform === 'win32' ? 'stepcheck.exe' : 'stepcheck');

// Rule-freeze provenance: the inference rules live in stepcheck/src/annot.rs
// (`infer_effect` and its keyword tables). They were frozen at commit dbf3c1a
// (blob 536271c1...). Later commits touched annot.rs only to resolve linked child
// workflows (resolve_linked_children); no inference rule has changed since.
const RULE_FREEZE = { file: 'stepcheck/src/annot.rs', commit: 'dbf3c1a',
  blob: '536271c1164a3d26267b0dcd5db9e732057b6ad4',
  note: 'later changes to annot.rs add linked-child resolution only; infer_effect is unchanged' };

function load(p) { return JSON.parse(fs.readFileSync(p, 'utf8')); }

const recon = load(path.join(ROOT, 'corpus', 'gold-labels.json')).labels;
const holdoutFiles = new Set(load(path.join(__dirname, 'holdout-workflows.json')).workflows);
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
    props[p] = { ...rates(c), accuracy_ci95: bootstrapAcc(c.tasks) };
  }
  return { workflows: files.length, properties: props, warning_precision: warningPrecision(files) };
}

const report = {
  generated_by: 'eval/score-holdout.js',
  rule_freeze: RULE_FREEZE,
    gold: { source: 'corpus/gold-labels.json', total_workflows: allFiles.length, design_workflows: designFiles.length,
          holdout_workflows: holdoutFiles.size, holdout_list: 'eval/holdout-workflows.json' },
  holdout: partition('holdout', [...holdoutFiles]),
  design: partition('design', designFiles),
};
fs.writeFileSync(path.join(__dirname, 'holdout-inference.json'), JSON.stringify(report, null, 2));

function show(name, part) {
  console.log(`\n=== ${name} (${part.workflows} workflows) ===`);
  for (const [p, v] of Object.entries(part.properties)) {
    console.log(`  ${p}: P=${v.precision} R=${v.recall} FOR=${v.false_omission_rate} F1=${v.f1} ` +
      `acc=${v.accuracy} ${JSON.stringify(v.accuracy_ci95)} ` +
      `[tp${v.tp} fp${v.fp} fn${v.fn} tn${v.tn} abstain${v.abstain}]`);
  }
  const w = part.warning_precision;
  console.log('  warning precision: ' + ['SC3001', 'SC4001', 'SC4010'].map(c => `${c}=${w[c].tp}/${w[c].tp + w[c].fp}`).join(' '));
}
console.log(`Rule-freeze: ${RULE_FREEZE.file} @ ${RULE_FREEZE.commit} (${RULE_FREEZE.note})`);
show('HOLD-OUT (reported)', report.holdout);
show('DESIGN (reference)', report.design);
console.log('\n-> eval/holdout-inference.json');
