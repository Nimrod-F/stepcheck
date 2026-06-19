// Baseline comparison: statelint (awslabs, Ruby gem 0.8.0 + j2119 0.4.0) vs StepCheck.
//   (a) in-the-wild: run statelint on all 193 corpus workflows, categorize what it
//       flags, and contrast with StepCheck's findings;
//   (b) detection power: for each corpus workflow x each of StepCheck's 4 mutation
//       classes, emit the mutant ASL via `stepcheck mutate` and check whether
//       statelint detects the *injected* defect (new problem attributable to it).
// Output: eval/baseline-statelint.json
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const ASL = path.join(ROOT, 'corpus', 'asl');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');
// Invoke statelint via ruby.exe + the gem's shebang script, with forward-slash
// paths (Windows accepts these and they survive shell quoting cleanly).
const RUBY = 'C:/Ruby33-x64/bin/ruby.exe';
const SLSCRIPT = 'C:/Ruby33-x64/bin/statelint';
const TMP = path.join(os.tmpdir(), 'sl-mut');
fs.mkdirSync(TMP, { recursive: true });

const LIMIT = process.argv[2] ? parseInt(process.argv[2], 10) : Infinity;
const files = fs.readdirSync(ASL).filter((f) => f.endsWith('.json')).sort().slice(0, LIMIT);

// Run statelint; return {count, problems[]}. statelint exits 0 (clean) / 1 (problems).
function statelint(file) {
  let out = '';
  try {
    out = execFileSync(RUBY, [SLSCRIPT, file.replace(/\\/g, '/')], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  } catch (e) {
    out = (e.stdout || '') + (e.stderr || '');
  }
  const lines = out.split(/\r?\n/);
  const problems = lines.filter((l) => l.trim().length && !/^\d+ error/.test(l) && !/^\s*$/.test(l)).map((l) => l.trim());
  const m = out.match(/^(\d+)\s+error/m);
  const count = m ? parseInt(m[1], 10) : problems.length;
  return { count, problems };
}

// Categorize a statelint problem message into a coarse class for reporting.
function classify(msg) {
  if (/should be a |is not a |should be one of|not allowed|unexpected|required|missing field|has no |Extra field|wrong type/i.test(msg)
      && !/No state found|No transition|not found in|terminal/i.test(msg)) {
    if (/JSONPath|Path|Reference/i.test(msg)) return 'jsonpath/path';
    return 'schema-type/shape';
  }
  if (/No state found|not found in States|No transition found to/i.test(msg)) return 'transition/reachability';
  if (/terminal/i.test(msg)) return 'no-terminal';
  if (/States\.ALL/i.test(msg)) return 'retry-States.ALL-placement';
  if (/also defined at|dupe|duplicate/i.test(msg)) return 'duplicate-state';
  if (/JSONPath|Path/i.test(msg)) return 'jsonpath/path';
  return 'other';
}

console.error(`[1/2] statelint baseline over ${files.length} workflows ...`);
const perFile = [];
const catTotals = {};
let flagged = 0, totalProblems = 0;
for (const f of files) {
  const r = statelint(path.join(ASL, f));
  if (r.count > 0) flagged++;
  totalProblems += r.count;
  const cats = {};
  for (const p of r.problems) { const c = classify(p); cats[c] = (cats[c] || 0) + 1; catTotals[c] = (catTotals[c] || 0) + 1; }
  perFile.push({ file: f, count: r.count, cats, problems: r.problems });
}

// Detection power on StepCheck's 4 mutation classes.
const KINDS = ['contract', 'retry', 'compensation', 'structural'];
// What an injected defect looks like in statelint's eyes (attribution patterns):
// A statelint problem counts as DETECTING the injected defect only if it names the
// actual fault, not an incidental schema nit. (Note: every injected unsafe retry also
// trips statelint's "IntervalSeconds should be an Integer" pedantry on the float
// interval — that is schema noise, NOT detection of the retry-safety defect, so we do
// not credit it.)
const ATTRIB = {
  structural: /__StepCheckMissingState__/,                       // dangling Next target
  contract: /BROKEN|is not a JSONPath|not a JSONPath or intrinsic/i, // broken .$ payload
  retry: /States\.ALL can only appear|States\.ALL.{0,40}last element/i, // only misplacement
  compensation: /__never__/,                                     // no compensation concept
};

console.error(`[2/2] mutation detection: ${files.length} x ${KINDS.length} ...`);
const mut = {};
for (const k of KINDS) mut[k] = { applicable: 0, sl_detected: 0, sl_any_new: 0, examples: [] };
for (const f of files) {
  const src = path.join(ASL, f);
  const baseProblems = perFile.find((x) => x.file === f).problems;
  for (const k of KINDS) {
    const outp = path.join(TMP, `${k}.json`);
    try { fs.existsSync(outp) && fs.unlinkSync(outp); } catch {}
    let applicable = true;
    try {
      execFileSync(BIN, ['mutate', src, '--kind', k, '--out', outp], { stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      applicable = false; // exit 3 = no applicable site
    }
    if (!applicable || !fs.existsSync(outp)) continue;
    mut[k].applicable++;
    const r = statelint(outp);
    // detected iff statelint emits a NEW problem attributable to the injected defect
    const newProblems = r.problems.filter((p) => !baseProblems.includes(p));
    if (newProblems.length > 0) mut[k].sl_any_new++;
    const hit = newProblems.some((p) => ATTRIB[k].test(p));
    if (hit) {
      mut[k].sl_detected++;
      if (mut[k].examples.length < 2) mut[k].examples.push({ file: f, problem: newProblems.find((p) => ATTRIB[k].test(p)) });
    }
  }
}

const report = {
  tool: 'statelint 0.8.0 (j2119 0.4.0)',
  corpus: files.length,
  in_the_wild: {
    statelint_flagged_files: flagged,
    statelint_total_problems: totalProblems,
    category_totals: catTotals,
    note: 'statelint checks JSON-schema conformance + structural/semantic rules expressible without task semantics; it has no notion of idempotency, persistence, business typestate, or data contracts beyond JSONPath syntax.',
  },
  mutation_detection: Object.fromEntries(
    KINDS.map((k) => [k, {
      applicable: mut[k].applicable,
      statelint_detected: mut[k].sl_detected,
      statelint_recall: mut[k].applicable ? +(mut[k].sl_detected / mut[k].applicable).toFixed(3) : null,
      statelint_any_new_problem: mut[k].sl_any_new,
      examples: mut[k].examples,
    }]),
  ),
};
fs.writeFileSync(path.join(__dirname, 'baseline-statelint.json'), JSON.stringify(report, null, 2));
console.error('done -> eval/baseline-statelint.json');
console.log(JSON.stringify(report.mutation_detection, null, 2));
console.log('in-the-wild:', JSON.stringify(report.in_the_wild, null, 2));
