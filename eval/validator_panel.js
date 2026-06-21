// Multi-tool baseline panel: run the validators a Step Functions developer could
// reach (statelint, asl-validator, and AWS's authoritative server-side
// ValidateStateMachineDefinition) over the corpus and tabulate, per workflow,
// which tools flag it. Produces a per-class Venn against StepCheck's `scan`:
// findings UNIQUE to StepCheck (the semantic classes), OVERLAP (structural), and
// a baseline-only column (JSONata/intrinsic syntax StepCheck ignores).
//
// Usage:  node eval/validator_panel.js [corpus/asl]
// Requires whichever tools are installed; missing tools are reported as skipped,
// so the panel degrades gracefully. AWS validation needs credentials (no
// resource creation, ~free): set STEPCHECK_AWS=1 to enable it.
const fs = require('fs');
const path = require('path');
const { execFileSync, execSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const DIR = process.argv[2] ? path.resolve(process.argv[2]) : path.join(ROOT, 'corpus', 'asl');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');

function have(cmd) {
  try { execSync(process.platform === 'win32' ? `where ${cmd}` : `command -v ${cmd}`, { stdio: 'ignore' }); return true; }
  catch { return false; }
}
const tools = {
  statelint: have('statelint'),
  'asl-validator': have('asl-validator'),
  aws: process.env.STEPCHECK_AWS === '1' && have('aws'),
};
console.error('tools available:', tools);

function run(cmd, args) {
  try { execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); return { ok: true, out: '' }; }
  catch (e) { return { ok: false, out: `${e.stdout || ''}${e.stderr || ''}` }; }
}

function statelintFlags(f) { return tools.statelint && !run('statelint', [f]).ok; }
function aslValidatorFlags(f) { return tools['asl-validator'] && !run('asl-validator', [f]).ok; }
function awsFlags(f) {
  if (!tools.aws) return null;
  const def = fs.readFileSync(f, 'utf8');
  const r = run('aws', ['stepfunctions', 'validate-state-machine-definition', '--definition', def]);
  return r.ok ? /"result":\s*"FAIL"/.test(r.out) : true;
}
function stepcheckCodes(f) {
  try {
    const out = execFileSync(BIN, ['check', '--json', '--infer', f], { encoding: 'utf8' });
    const diags = JSON.parse(out).diagnostics || [];
    return [...new Set(diags.map(d => d.code))];
  } catch { return []; }
}

// Map a StepCheck code to a defect class for the Venn.
const CLASS = c =>
  c.startsWith('SC0') ? 'structural' :
  c === 'SC1101' || c === 'SC1110' || c === 'SC1010' ? 'data-flow' :
  c.startsWith('SC1') ? 'binding' :
  c.startsWith('SC2') ? 'typestate' :
  c.startsWith('SC3') ? 'retry' :
  c.startsWith('SC4') ? 'compensation' :
  c.startsWith('SC5') ? 'concurrency' :
  c.startsWith('SC6') ? 'temporal' : 'other';

const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith('.json') || e.name.endsWith('.asl.json')) files.push(p);
  }
})(DIR);

let scOnly = 0, baselineOnly = 0, both = 0, neither = 0;
const byClass = {};
for (const f of files) {
  const codes = stepcheckCodes(f);
  const scClasses = new Set(codes.map(CLASS));
  for (const cl of scClasses) byClass[cl] = (byClass[cl] || 0) + 1;
  const baseFlag = statelintFlags(f) || aslValidatorFlags(f) || awsFlags(f) === true;
  const scFlag = codes.length > 0;
  if (scFlag && baseFlag) both++;
  else if (scFlag) scOnly++;
  else if (baseFlag) baselineOnly++;
  else neither++;
}
console.log(JSON.stringify({
  dir: DIR, files: files.length, tools,
  venn: { stepcheck_only: scOnly, baseline_only: baselineOnly, both, neither },
  stepcheck_by_class: byClass,
  note: 'baseline = union(statelint, asl-validator, AWS ValidateStateMachineDefinition). ' +
        'stepcheck_only is dominated by the semantic classes no validator expresses.',
}, null, 2));
