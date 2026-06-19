// E4: inference accuracy vs the hand-labeled gold set.
// Runs `stepcheck infer --json` on each gold workflow and compares the inferred
// idempotency/persistence to the gold labels. Abstentions (no keyword matched,
// prediction null) are reported separately from wrong predictions.
const fs = require('fs');
const { execFileSync } = require('child_process');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');
const gold = JSON.parse(fs.readFileSync(path.join(ROOT, 'corpus', 'gold-labels.json'))).labels;

function score(field) {
  return { correct: 0, wrong: 0, abstain: 0, total: 0 };
}
const idem = score(), pers = score();
const perTask = [];

for (const [file, tasks] of Object.entries(gold)) {
  const out = execFileSync(BIN, ['infer', '--json', path.join(ROOT, 'corpus', 'asl', file)], { encoding: 'utf8' });
  const preds = JSON.parse(out);
  const byName = Object.fromEntries(preds.map(p => [p.state, p]));
  for (const [name, g] of Object.entries(tasks)) {
    const p = byName[name];
    if (!p) { console.error(`WARN: no prediction for ${file} :: ${name}`); continue; }
    for (const [field, acc] of [['idempotent', idem], ['persistent', pers]]) {
      acc.total++;
      const pred = p[field];
      if (pred === null || pred === undefined) acc.abstain++;
      else if (pred === g[field]) acc.correct++;
      else acc.wrong++;
    }
    perTask.push({ file: file.split('__').slice(-1)[0], task: name,
      gold_idem: g.idempotent, pred_idem: p.idempotent,
      gold_pers: g.persistent, pred_pers: p.persistent });
  }
}

function summary(name, s) {
  const decided = s.correct + s.wrong;
  const acc = decided ? (s.correct / decided) : 0;
  const cov = s.total ? (decided / s.total) : 0;
  return { property: name, labeled: s.total, predicted: decided, abstained: s.abstain,
    correct: s.correct, wrong: s.wrong,
    accuracy_on_predicted: +(100 * acc).toFixed(1), coverage: +(100 * cov).toFixed(1) };
}

const report = {
  workflows: Object.keys(gold).length,
  tasks_labeled: idem.total,
  idempotency: summary('idempotency', idem),
  persistence: summary('persistence', pers),
};
console.log(JSON.stringify(report, null, 2));
fs.writeFileSync(path.join(ROOT, 'eval', 'inference_accuracy.json'),
  JSON.stringify({ ...report, per_task: perTask }, null, 2));
