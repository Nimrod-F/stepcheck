// Generates eval/label-signals.md: a human-readable reference of every Task state
// in the 13 LLM-expanded gold workflows, with the signals you need to label it by
// hand (Resource/action, FunctionName/target, nesting). Read this while filling
// gold-labels-human-A.json / -B.json. Run: node eval/make-label-signals.js
const fs = require('fs');
const path = require('path');
const ROOT = path.resolve(__dirname, '..');

const template = JSON.parse(fs.readFileSync(path.join(__dirname, 'gold-labels-human.template.json'), 'utf8'));

// Recursively find a state object by name within a machine and its sub-machines.
function findState(machine, name) {
  if (!machine || typeof machine !== 'object') return null;
  const states = machine.States || {};
  if (states[name]) return states[name];
  for (const s of Object.values(states)) {
    if (!s || typeof s !== 'object') continue;
    const subs = [];
    if (s.Iterator) subs.push(s.Iterator);
    if (s.ItemProcessor) subs.push(s.ItemProcessor);
    if (Array.isArray(s.Branches)) subs.push(...s.Branches);
    for (const sub of subs) {
      const hit = findState(sub, name);
      if (hit) return hit;
    }
  }
  return null;
}

function signal(st) {
  if (!st) return { type: '?', resource: '', action: '', target: '' };
  const res = st.Resource || '';
  const action = res ? res.toLowerCase().split(/[.:]/).filter(Boolean).pop() : '';
  const p = st.Parameters || st.Arguments || {};
  let target = '';
  for (const k of ['FunctionName', 'StateMachineArn', 'TableName', 'QueueUrl', 'TopicArn', 'Bucket', 'Action', 'ApiEndpoint']) {
    if (typeof p[k] === 'string') { target = `${k}=${p[k]}`; break; }
  }
  return { type: st.Type || '?', resource: res, action, target };
}

let out = `# Label signals for the 13 expansion workflows\n\n`;
out += `Fill \`idempotent\` and \`persistent\` (\`true\` / \`false\` / \`null\`) for each task in\n`;
out += `\`gold-labels-human-A.json\` and \`gold-labels-human-B.json\`, using these signals and\n`;
out += `the rubric in HUMAN-GOLD-LABELLING.md. \`null\` = abstain (cannot decide).\n\n`;

let files = 0, tasks = 0;
for (const file of Object.keys(template)) {
  files++;
  // the corpus stores files under corpus/asl/<key>
  let asl = null;
  const fp = path.join(ROOT, 'corpus', 'asl', file);
  try { asl = JSON.parse(fs.readFileSync(fp, 'utf8')); } catch (e) { /* keep going */ }
  out += `\n## ${file}\n\n`;
  out += `| task | type | action | target | idempotent? | persistent? |\n`;
  out += `|------|------|--------|--------|-------------|-------------|\n`;
  for (const task of Object.keys(template[file])) {
    tasks++;
    const sg = signal(findState(asl, task));
    const tgt = (sg.target || sg.resource || '').replace(/\|/g, '\\|').slice(0, 60);
    out += `| ${task.replace(/\|/g, '\\|')} | ${sg.type} | ${sg.action} | ${tgt} |  |  |\n`;
  }
}
out += `\n_Total: ${files} files, ${tasks} tasks._\n`;
fs.writeFileSync(path.join(__dirname, 'label-signals.md'), out);
console.log(`wrote eval/label-signals.md (${files} files, ${tasks} tasks)`);
