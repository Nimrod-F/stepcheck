// Multi-tool baseline panel. Runs the three validators a Step Functions developer
// can actually reach --- statelint (AWS Labs / J2119), asl-validator (npm), and
// AWS's authoritative server-side ValidateStateMachineDefinition --- and contrasts
// them with StepCheck on two axes:
//
//   (A) in the wild: over the raw corpus, how many workflows each tool flags and
//       what *kind* of problem (schema/structural nits vs. the semantic classes);
//   (B) detection power: for each StepCheck mutation class, inject one defect with
//       `stepcheck mutate` and check whether each baseline validator reports a NEW
//       problem *naming the injected fault*.
//
// (B) is confound-free: the mutant is StepCheck's emitter output, so we compare each
// validator's verdict on the emitted MUTANT against its verdict on the emitted, but
// otherwise UNCHANGED, control (`stepcheck emit`). The single difference between the
// two files is the injected defect, so a new problem on the mutant is attributable
// to it (we still require the message to name the fault, never crediting incidental
// schema nits such as a float IntervalSeconds).
//
// Usage:  node eval/validator_panel.js [corpus/asl]
//   STEPCHECK_AWS=1  also run AWS ValidateStateMachineDefinition (needs credentials;
//                    the call creates no resources and is free).
// Output: eval/validator-panel.json
'use strict';
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync, execSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const DIR = process.argv[2] ? path.resolve(process.argv[2]) : path.join(ROOT, 'corpus', 'asl');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');

// --- tool locations (robust on Windows: invoke the real interpreters, not shims) --
const RUBY = 'C:/Ruby33-x64/bin/ruby.exe';
const SLSCRIPT = 'C:/Ruby33-x64/bin/statelint';
const ASLV = path.join(
  process.env.APPDATA || os.homedir(),
  'npm', 'node_modules', 'asl-validator', 'dist', 'bin', 'asl-validator.js'
);
function have(p) { try { return fs.existsSync(p); } catch { return false; } }
function haveCmd(c) { try { execSync(`where ${c}`, { stdio: 'ignore' }); return true; } catch { return false; } }
const tools = {
  statelint: have(RUBY) && have(SLSCRIPT),
  'asl-validator': have(ASLV),
  aws: process.env.STEPCHECK_AWS === '1' && haveCmd('aws'),
};
console.error('tools available:', tools);

const TMP = path.join(os.tmpdir(), 'sc-panel');
fs.mkdirSync(TMP, { recursive: true });

function run(cmd, args) {
  try { const out = execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 * 1024 * 1024 }); return { code: 0, out }; }
  catch (e) { return { code: e.status == null ? -1 : e.status, out: (e.stdout || '') + (e.stderr || '') }; }
}

// --- normalized problem sets per tool (so control vs mutant diffs cleanly) ---------
function statelintProblems(f) {
  if (!tools.statelint) return null;
  const r = run(RUBY, [SLSCRIPT, f.replace(/\\/g, '/')]);
  return r.out.split(/\r?\n/).map(l => l.trim())
    .filter(l => l.length && !/^\d+\s+error/.test(l));
}
function aslvProblems(f) {
  if (!tools['asl-validator']) return null;
  const r = run('node', [ASLV, '--json-path', f]);
  if (r.code === 0) return [];
  return r.out.split(/\r?\n/).map(l => l.trim())
    .filter(l => l.length && /[A-Z_]{3,}|invalid|missing|must/i.test(l) && !/is invalid:?$/.test(l));
}
let awsThrottleSleeps = 0;
function awsProblems(f) {
  if (!tools.aws) return null;
  const def = fs.readFileSync(f, 'utf8');
  for (let attempt = 0; attempt < 4; attempt++) {
    const r = run('aws', ['stepfunctions', 'validate-state-machine-definition', '--definition', def, '--output', 'json']);
    let j;
    try { j = JSON.parse(r.out); } catch { j = null; }
    if (j && Array.isArray(j.diagnostics)) {
      return j.diagnostics.map(d => `${d.code}@${d.location || ''}::${d.message || ''}`);
    }
    if (/Throttl|Rate exceeded|TooManyRequests/i.test(r.out)) { awsThrottleSleeps++; sleepMs(400 * (attempt + 1)); continue; }
    return ['<aws-error>::' + r.out.slice(0, 120)];
  }
  return ['<aws-throttled>'];
}
function sleepMs(ms) { try { execSync(process.platform === 'win32' ? `ping -n ${Math.ceil(ms / 1000) + 1} 127.0.0.1 > NUL` : `sleep ${ms / 1000}`, { stdio: 'ignore' }); } catch {} }

// --- StepCheck defect class -> coarse class, for the in-the-wild Venn --------------
const SC_CLASS = c =>
  c.startsWith('SC0') ? 'structural' :
  (c === 'SC1101' || c === 'SC1110' || c === 'SC1010') ? 'data-flow' :
  c.startsWith('SC1') ? 'binding' :
  c.startsWith('SC2') ? 'typestate' :
  c.startsWith('SC3') ? 'retry' :
  c.startsWith('SC4') ? 'compensation' :
  c.startsWith('SC5') ? 'concurrency' :
  c.startsWith('SC6') ? 'temporal' : 'other';
function stepcheckCodes(f) {
  const parse = s => { try { return [...new Set((JSON.parse(s).diagnostics || []).map(d => d.code))]; } catch { return null; } };
  try {
    return parse(execFileSync(BIN, ['check', '--json', '--infer', f], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })) || [];
  } catch (e) {
    // `check` exits non-zero when a file has an error-level finding; its JSON is still on stdout.
    return parse(e.stdout || '') || [];
  }
}

// --- attribution: does a NEW problem string name the injected fault? ----------------
// Applied only to the matching mutant; semantic-class patterns are intentionally
// unsatisfiable for schema validators (they have no such concept), so those score 0.
const ATTRIB = {
  structural:   /MISSING_TRANSITION_TARGET|MISSING_TERMINAL|not reachable|No state found|not found in States|no transition found|__StepCheckMissingState__/i,
  contract:     /SCHEMA_VALIDATION_FAILED|asl_payload_template|is not a JSONPath|not a valid JSONPath|JSONPath|intrinsic/i,
  // The four semantic classes are intentionally hard to satisfy: a schema validator
  // detects "unsafe retry" only if it complains about retrying a *non-idempotent*
  // operation, etc. Crucially we do NOT match a bare "idempoten" (it occurs in state
  // names like "Create idempotency settings"), and a numeric/type nit (a float
  // IntervalSeconds, a BackoffRate type --- the same complaint the paper declines to
  // credit) is stripped by NIT below before attribution.
  retry:        /not safe to retr|unsafe to retr|retry\w*\s+\w*\s*(a\s+)?non-?idempotent|non-?idempotent\s+\w*\s*(task|operation|action).{0,24}retr/i,
  compensation: /missing compensation|uncompensated|no compensating|rollback handler|\bsaga\b.{0,24}(incomplete|missing)/i,
  concurrency:  /race condition|interfer|contend|shared resource|both branches write|concurrent.{0,16}write/i,
  temporal:     /heartbeat.{0,30}timeout|timeout.{0,30}heartbeat|HeartbeatSeconds.{0,30}Timeout/i,
};
// A purely numeric/type schema nit is not detection of the injected semantic defect.
const NIT = /IntervalSeconds|BackoffRate|MaxAttempts|of type (Integer|Number)|should be an? (Integer|Number)|Expected value of type/i;
const KINDS = ['structural', 'contract', 'retry', 'compensation', 'concurrency', 'temporal'];
const KIND_LABEL = {
  structural: 'Dangling transition (SC0002)',
  contract: 'Broken data binding (SC1003)',
  retry: 'Unsafe retry (SC3001)',
  compensation: 'Missing compensation (SC4001)',
  concurrency: 'Concurrency interference (SC5001)',
  temporal: 'Temporal heartbeat/timeout (SC6003)',
};

// --- enumerate corpus --------------------------------------------------------------
const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith('.json')) files.push(p);
  }
})(DIR);
files.sort();
console.error(`corpus: ${files.length} workflows`);

// ============ (A) in the wild ============
console.error('[A] in-the-wild over raw corpus ...');
const inWild = {
  statelint: { flagged: 0, problems: 0 },
  'asl-validator': { flagged: 0, problems: 0 },
  aws: { flagged: 0, problems: 0 },
};
const byClass = {};                 // StepCheck class -> #workflows
let scOnly = 0, baseOnly = 0, both = 0, neither = 0;
let done = 0;
for (const f of files) {
  const codes = stepcheckCodes(f);
  for (const cl of new Set(codes.map(SC_CLASS))) byClass[cl] = (byClass[cl] || 0) + 1;
  const scFlag = codes.length > 0;

  let baseFlag = false;
  for (const [tool, fn] of [['statelint', statelintProblems], ['asl-validator', aslvProblems], ['aws', awsProblems]]) {
    const probs = fn(f);
    if (probs == null) continue;
    const realProbs = probs.filter(p => !/^<aws-(error|throttled)>/.test(p));
    if (realProbs.length > 0) { inWild[tool].flagged++; inWild[tool].problems += realProbs.length; baseFlag = true; }
  }
  if (scFlag && baseFlag) both++; else if (scFlag) scOnly++; else if (baseFlag) baseOnly++; else neither++;
  if (++done % 25 === 0) console.error(`  ... ${done}/${files.length}`);
}

// ============ (B) detection power on injected defects ============
console.error('[B] mutation detection (emit-control vs emit-mutant) ...');
const detect = {};                  // kind -> tool -> {applicable, detected, anyNew}
for (const k of KINDS) detect[k] = {
  applicable: 0,
  statelint: { detected: 0, anyNew: 0 },
  'asl-validator': { detected: 0, anyNew: 0 },
  aws: { detected: 0, anyNew: 0 },
};
const ctl = path.join(TMP, 'control.json');
const mut = path.join(TMP, 'mutant.json');
done = 0;
for (const f of files) {
  // emitted control (once per workflow)
  const er = run(BIN, ['emit', f, '--out', ctl]);
  if (er.code !== 0 || !fs.existsSync(ctl)) { console.error(`  emit failed: ${path.basename(f)}`); continue; }
  const ctlProb = {
    statelint: statelintProblems(ctl),
    'asl-validator': aslvProblems(ctl),
    aws: awsProblems(ctl),
  };
  for (const k of KINDS) {
    try { fs.existsSync(mut) && fs.unlinkSync(mut); } catch {}
    const mr = run(BIN, ['mutate', f, '--kind', k, '--out', mut]);
    if (mr.code === 3 || !fs.existsSync(mut)) continue;   // no applicable site
    detect[k].applicable++;
    for (const tool of ['statelint', 'asl-validator', 'aws']) {
      const base = ctlProb[tool]; if (base == null) continue;
      const m = (tool === 'statelint' ? statelintProblems : tool === 'asl-validator' ? aslvProblems : awsProblems)(mut);
      if (m == null) continue;
      const baseSet = new Set(base);
      const fresh = m.filter(p => !baseSet.has(p) && !/^<aws-(error|throttled)>/.test(p));
      if (fresh.length) detect[k][tool].anyNew++;
      // credit only a NEW problem that names the fault and is not a numeric/type nit
      const creditable = fresh.filter(p => !NIT.test(p));
      if (creditable.some(p => ATTRIB[k].test(p))) detect[k][tool].detected++;
    }
  }
  if (++done % 25 === 0) console.error(`  ... ${done}/${files.length}`);
}

// ============ report ============
const report = {
  generated_by: 'eval/validator_panel.js',
  corpus_dir: DIR,
  workflows: files.length,
  tools,
  aws_throttle_sleeps: awsThrottleSleeps,
  in_the_wild: {
    per_tool: inWild,
    stepcheck_by_class: byClass,
    venn_vs_stepcheck: { stepcheck_only: scOnly, baseline_only: baseOnly, both, neither },
    note: 'baseline = union of the available validators; stepcheck_only is dominated by the semantic classes no validator expresses.',
  },
  mutation_detection: Object.fromEntries(KINDS.map(k => {
    const d = detect[k];
    const pct = t => d.applicable ? +(d[t].detected / d.applicable * 100).toFixed(1) : null;
    return [k, {
      label: KIND_LABEL[k],
      applicable: d.applicable,
      statelint: { detected: d.statelint.detected, recall_pct: pct('statelint'), any_new_problem: d.statelint.anyNew },
      'asl-validator': { detected: d['asl-validator'].detected, recall_pct: pct('asl-validator'), any_new_problem: d['asl-validator'].anyNew },
      aws: { detected: d.aws.detected, recall_pct: pct('aws'), any_new_problem: d.aws.anyNew },
    }];
  })),
  note: 'detection credited only when a validator emits a NEW problem (vs the emitted control) that NAMES the injected fault; incidental schema nits (e.g. a float IntervalSeconds) are not credited.',
};
fs.writeFileSync(path.join(__dirname, 'validator-panel.json'), JSON.stringify(report, null, 2));
console.error('done -> eval/validator-panel.json');
console.log(JSON.stringify({ in_the_wild: report.in_the_wild, mutation_detection: report.mutation_detection }, null, 2));
