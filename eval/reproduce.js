#!/usr/bin/env node
// One-command reproduction of the numbers in the ICSOC 2026 paper
// "StepCheck: Sound Static Verification of AWS Step Functions Workflows".
//
//   node eval/reproduce.js [--tier 0|1|2|3] [--no-cargo-test]
//
// Tiers are cumulative (--tier 2 also runs tiers 0 and 1); the default is 1.
//   0  smoke test: version, unit tests, running example (Fig. 1), fixpoint claims   ~2 min
//   1  every StepCheck-side number of Sections 4.1-4.4, offline                   ~5 min
//   2  the baseline columns of Table 1: statelint, asl-validator, Woflan and
//      BPMN Analyzer 2.0 on all 193 workflows, BProVe on a sample (Docker)     ~25 min
//      (--bprove-full runs BProVe on all 193 workflows: about six hours)
//   3  network: re-fetches the 36 unredistributed corpus files by pinned commit,
//      then re-runs the independent-repository and fix-commit studies           ~1 min
//      (AWS ValidateStateMachineDefinition additionally needs STEPCHECK_AWS=1
//       and credentials; see README "Reproduce the AWS round-trip")
//
// Each check prints PASS or FAIL against the value in the paper. Timing values
// depend on the machine and print as INFO. Values that cannot be re-run in the chosen
// tier are checked against the recorded result file and print as RECORDED.
// Fresh outputs go to eval/repro/ (git-ignored), so recorded results are never
// overwritten. Exit status is 1 if any check fails.
'use strict';
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const OUT = path.join(__dirname, 'repro');
fs.mkdirSync(OUT, { recursive: true });
const argv = process.argv.slice(2);
const TIER = argv.includes('--tier') ? parseInt(argv[argv.indexOf('--tier') + 1], 10) : 1;
const WIN = process.platform === 'win32';
const BIN = process.env.STEPCHECK_BIN ||
  path.join(ROOT, 'stepcheck', 'target', 'release', WIN ? 'stepcheck.exe' : 'stepcheck');
const PY = process.env.PYTHON || (WIN ? 'python' : 'python3');

// ---------------------------------------------------------------- helpers
function sh(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, windowsHide: true, ...opts });
  return { code: r.status, out: r.stdout || '', err: (r.stderr || '') + (r.error ? String(r.error) : '') };
}
function sc(args) {
  const r = sh(BIN, args);
  if (r.code === null || r.code > 2) throw new Error(`stepcheck ${args.join(' ')} failed: ${r.err.slice(0, 300)}`);
  return r.out;
}
const scJson = (args, save) => {
  const out = sc(args);
  if (save) fs.writeFileSync(path.join(OUT, save), out);
  return JSON.parse(out);
};
const readJson = (rel) => JSON.parse(fs.readFileSync(path.join(ROOT, rel), 'utf8'));
const r2 = (x) => Math.round(x * 100) / 100;
const have = (cmd, args = ['--version']) => { const r = sh(cmd, args); return r.code === 0; };

const results = [];
let section = '';
function head(title) { section = title; console.log(`\n== ${title}`); }
function record(status, claim, actual, expected, where) {
  results.push({ section, status, claim, actual, expected, where });
  const pad = (s, n) => (s + ' '.repeat(n)).slice(0, n);
  const exp = ['FAIL', 'INFO'].includes(status) && expected !== '' ? ` (paper: ${expected})` : '';
  console.log(`  ${pad(status, 9)}${pad(claim, 58)} ${actual}${exp}`);
}
function check(claim, actual, expected, where) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  record(ok ? 'PASS' : 'FAIL', claim, JSON.stringify(actual), JSON.stringify(expected), where);
}
function info(claim, actual, paper, where) { record('INFO', claim, actual, paper, where); }
function recorded(claim, actual, expected, where) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  record(ok ? 'RECORDED' : 'FAIL', claim, JSON.stringify(actual), JSON.stringify(expected), where);
}
function skip(claim, why) { record('SKIP', claim, why, '', ''); }
function guard(claim, fn) {
  try { fn(); } catch (e) { record('FAIL', claim, 'error: ' + e.message.split('\n')[0], '', ''); }
}
// Exact (Clopper-Pearson) lower bound when every one of n trials succeeds.
const cpLower = (n) => Math.pow(0.025, 1 / n);

// Table 1, as printed in the paper. Keys are StepCheck's mutation classes.
const TABLE1 = {
  //             n    AWS   sl    av    BPV   BPMN  Wof   hard n, hard SC
  Structural:   [166, 1.00, 1.00, 0.90, 0.69, 0.87, 0.95, 42, 1.00],
  Contract:     [141, 0.96, 0.99, 0.94, 0, 0, 0, 166, 1.00],
  Retry:        [103, 0, 0, 0, 0, 0, 0, 111, 0.68],
  Compensation: [19, 0, 0, 0, 0.11, 0.37, 0.37, 56, 0.79],
  Concurrency:  [21, 0, 0, 0, 0.05, 0, 0, 21, 0.00],
  Temporal:     [166, 0, 0, 0, 0, 0, 0, 166, 0.00],
};
const PANEL_KEY = { Structural: 'structural', Contract: 'contract', Retry: 'retry', Compensation: 'compensation', Concurrency: 'concurrency', Temporal: 'temporal' };

if (!fs.existsSync(BIN)) {
  console.error(`stepcheck binary not found at ${BIN}.\nBuild it first: cargo build --release --locked --manifest-path stepcheck/Cargo.toml\n(or set STEPCHECK_BIN).`);
  process.exit(2);
}
console.log(`StepCheck artifact reproduction, tier ${TIER}; binary: ${path.relative(ROOT, BIN) || BIN}`);

// ================================================================ tier 0
head('Tier 0: smoke test');
guard('tool version', () => check('stepcheck --version', sc(['--version']).trim(), 'stepcheck 0.1.6', 'artifact'));
if (argv.includes('--no-cargo-test') || !have('cargo')) {
  skip('unit and integration tests', argv.includes('--no-cargo-test') ? '--no-cargo-test' : 'cargo not found');
} else {
  guard('unit and integration tests', () => {
    const r = sh('cargo', ['test', '--release', '--locked', '--manifest-path', 'stepcheck/Cargo.toml']);
    const all = r.out + r.err;
    const passed = [...all.matchAll(/test result: \w+\. (\d+) passed; (\d+) failed/g)];
    const p = passed.reduce((a, m) => a + +m[1], 0), f = passed.reduce((a, m) => a + +m[2], 0);
    check('cargo test: failed tests', f, 0, 'stepcheck/src/tests.rs');
    info('cargo test: passed tests', String(p), '', '');
  });
}
guard('running example', () => {
  const ex = 'corpus/examples/running-example.asl.json';
  const nat = scJson(['check', '--json', ex]).diagnostics.map((d) => `${d.code}:${d.severity}`).sort();
  check('Fig. 1 native: defects (1) and (5)', nat, ['SC1101:error', 'SC6001:warning'], 'Sec. 2, Fig. 1');
  const inf = scJson(['check', '--json', '--infer', ex]).diagnostics;
  const codes = [...new Set(inf.map((d) => d.code))].sort();
  check('Fig. 1 --infer: adds (3) SC3001, (4) SC5001, (6) SC4001', codes, ['SC1101', 'SC3001', 'SC4001', 'SC5001', 'SC6001'], 'Sec. 2, Fig. 1');
});
guard('fixpoint termination', () => {
  const r = sh('node', ['eval/check_repro.js'], { env: { ...process.env, STEPCHECK_BIN: BIN } });
  check('widened fixpoint claims (eval/check_repro.js)', r.code, 0, 'TR; eval/fixpoint-stats.json');
});

// ================================================================ tier 1
if (TIER >= 1) {
  head('Tier 1 / RQ1: designed mutants (Table 1, SC column)');
  let designed;
  guard('designed mutants', () => {
    designed = scJson(['eval', 'corpus/asl', '--infer'], 'eval-infer.json');
    check('corpus: workflows, states', [designed.corpus.files, designed.corpus.total_states], [193, 1355], 'Sec. 4');
    let total = 0;
    for (const [cls, row] of Object.entries(TABLE1)) {
      const m = designed.mutation_study[cls];
      total += m.detected;
      check(`${cls} (${m.expected_code}): n, recall`, [m.applicable, r2(m.recall)], [row[0], 1.0], 'Table 1');
    }
    check('all designed mutants detected', total, 616, 'Sec. 4.1');
    check('exact 95% CI lower bounds at n=19 and n=166', [r2(cpLower(19)), r2(cpLower(166))], [0.82, 0.98], 'Sec. 4.1');
  });

  head('Tier 1 / RQ1: hard mutants (Table 1, hard columns)');
  guard('hard mutants', () => {
    const h = scJson(['eval', 'corpus/asl', '--infer', '--result-shapes', '--hard'], 'eval-hard.json').mutation_study_hard;
    let det = 0, n = 0;
    for (const [cls, row] of Object.entries(TABLE1)) {
      const m = h[cls];
      check(`hard ${cls}: n, recall`, [m.applicable, r2(m.recall_family)], [row[7], row[8]], 'Table 1');
    }
    for (const m of Object.values(h)) { det += m.detected_family; n += m.applicable; }
    check('hard data-flow: n, detected', [h.Dataflow.applicable, h.Dataflow.detected_family], [126, 0], 'Sec. 4.2');
    check('hard aggregate: detected / total', [det, n], [328, 688], 'Sec. 4.1');
    check('hard aggregate recall', r2(det / n), 0.48, 'Sec. 4.1');
  });

  head('Tier 1 / RQ2: data-flow soundness and coverage');
  guard('data-flow', () => {
    const clean = designed || scJson(['eval', 'corpus/asl', '--infer']);
    check('SC1101 on the clean corpus', clean.baseline.code_totals.SC1101 || 0, 0, 'Sec. 4.2');
    const df = scJson(['eval', 'corpus/asl', '--infer', '--strict-input', '--result-shapes'], 'eval-dataflow.json').mutation_study.Dataflow;
    check('missing-field mutants: n, detected', [df.applicable, df.detected], [126, 88], 'Sec. 4.2');
    const cert = scJson(['dataflow-cert', 'corpus/asl', '--infer', '--result-shapes'], 'dataflow-cert.json');
    check('certificate: certified reports, obligations, failed', [cert.certificate.certified_sc1101_reports, cert.certificate.postcondition_obligations, cert.certificate.failed_postcondition_obligations], [88, 1146, 0], 'Sec. 4.2');
    const or = scJson(['oracle', 'corpus/asl', '--infer', '--result-shapes'], 'oracle.json').oracle;
    check('bounded oracle: confirmed absent, counterexamples', [or.confirmed_absent, or.counterexamples_present], [88, 0], 'Sec. 4.2');
    const pc = scJson(['path-coverage', 'corpus/asl'], 'path-coverage.json').combined;
    const docReads = pc.reads_total - pc.context - pc.intrinsic;   // excludes $$ and States.*
    check('modeled fragment: precise / document-field reads', [pc.precise, docReads], [742, 803], 'Sec. 4.2');
    check('modeled fragment coverage (%)', Math.round(100 * pc.precise / docReads), 92, 'Sec. 4.2');
  });

  head('Tier 1 / RQ3: real-world evidence');
  guard('in-the-wild findings', () => {
    const b = (designed || scJson(['eval', 'corpus/asl', '--infer'])).baseline;
    const ct = b.code_totals;
    const inferred = (ct.SC3001 || 0) + (ct.SC4001 || 0) + (ct.SC4010 || 0);
    check('diagnostics: total, inferred-tier, native-tier', [b.errors + b.warnings, inferred, b.errors + b.warnings - inferred], [226, 181, 45], 'Sec. 4.3');
    check('unbounded callbacks (SC6001)', ct.SC6001, 33, 'Sec. 4.3');
    // The census script rewrites its recorded report; keep a copy in eval/repro/ and restore it.
    const censusFile = path.join(ROOT, 'eval/diagnostic-precision-census.native.json');
    const kept = fs.readFileSync(censusFile);
    const r = sh('node', ['eval/native_census_verify.js'], { env: { ...process.env, STEPCHECK_BIN: BIN } });
    const census = readJson('eval/diagnostic-precision-census.native.json');
    fs.copyFileSync(censusFile, path.join(OUT, 'diagnostic-precision-census.native.json'));
    fs.writeFileSync(censusFile, kept);
    const s = census.totals.overall;
    check('code-disjoint census: confirmed / native findings', [s.tp, s.n], [45, 45], 'Sec. 4.3');
    if (r.code !== 0) throw new Error('native_census_verify.js exited ' + r.code);
  });
  guard('issues', () => {
    const I = 'corpus/realbugs/issues/';
    const runs = [
      ['campus-compute #32', ['check', '--json', '--infer', '--result-shapes', I + 'campus-compute-32-REAL-cdk-synth.asl.json']],
      ['turbofan #1', ['check', '--json', '--annot', I + 'turbofan-1-missing-input.sidecar.toml', I + 'turbofan-1-missing-input.asl.json']],
      ['serverless-coffee-workshop #56', ['check', '--json', '--annot', I + 'coffee-workshop-56.sidecar.toml', I + 'coffee-workshop-56-detail-orderid.asl.json']],
    ];
    const hits = runs.filter(([, a]) => scJson(a).diagnostics.some((d) => d.code === 'SC1101' && d.severity === 'error')).length;
    check('user-filed issues flagged with SC1101', hits, 3, 'Sec. 4.3');
  });
  guard('fix-commit pairs', () => {
    const pairs = scJson(['eval-pairs', 'corpus/realbugs', '--infer'], 'realbugs-pairs.json');
    const local = fs.readdirSync(path.join(ROOT, 'corpus/realbugs')).filter((f) => f.endsWith('-pre.json')).length;
    check('shipped pairs: all caught', [pairs.caught, pairs.pairs], [local, local], 'corpus/realbugs');
    const rb = readJson('corpus/realbugs/realbugs.json');
    recorded('fix-commit pairs: mined, flagged, genuine', [rb.mined_pairs, rb.flagged_by_stepcheck, rb.confirmed_genuine], [39, 7, 6], 'Sec. 4.3');
  });
  guard('deployment artifacts', () => {
    // check --json on a CloudFormation/SAM template returns one entry per state machine.
    const perMachine = (dir) => fs.readdirSync(path.join(ROOT, dir)).filter((f) => !/\.md$/i.test(f)).sort().flatMap((f) => {
      const j = JSON.parse(sc(['check', '--json', '--infer', path.join(dir, f)]) || 'null');
      if (j === null) return [];
      return (Array.isArray(j) ? j.map((m) => ({ file: f, codes: m.result.diagnostics.map((d) => d.code) }))
        : [{ file: f, codes: j.diagnostics.map((d) => d.code) }]);
    });
    const sam = perMachine('corpus/aws-templates');
    const patterns = new Map();   // serverless-patterns: one workflow per pattern directory
    for (const m of sam) {
      const p = m.file.split('__')[0];
      patterns.set(p, (patterns.get(p) || 0) + m.codes.length);
    }
    check('SAM (serverless-patterns): workflows, flagged', [patterns.size, [...patterns.values()].filter((c) => c > 0).length], [55, 13], 'Sec. 4.3');
    const sol = scJson(['scan', 'corpus/aws-solutions', '--infer'], 'scan-aws-solutions.json');
    check('AWS Solutions Library: machines, flagged', [sol.machines, sol.flagged_machines], [26, 14], 'Sec. 4.3');
  });
  guard('CNCF', () => {
    const c = scJson(['eval', 'corpus/cncf', '--infer'], 'eval-cncf.json');
    check('CNCF examples lowered and checked', c.corpus.files, 66, 'Sec. 4.3');
    recorded('CNCF examples with declared records', readJson('eval/cncf-jq-coverage.json').workflows_with_modelable_pure_field_ref, 12, 'Sec. 4.3');
  });
  guard('independent repositories', () => {
    const w = readJson('eval/results-wild-external.json');
    const repos = new Set(readJson('corpus/wild-external/manifest.json').map((m) => m.repo)).size;
    recorded('independent ASL: definitions, repositories', [w.files, repos], [95, 16], 'Sec. 4.3 (re-run in tier 3)');
  });
  record('N/A', 'automotive case study (Sec. 4.3)', 'private module, not distributable', '', 'Sec. 4.3');

  head('Tier 1 / RQ4: build-time cost');
  guard('scale', () => {
    const dir = path.join(OUT, 'scale-10000');
    fs.mkdirSync(dir, { recursive: true });
    const g = sh(PY, ['eval/gen_scale.py', '10000', path.join(dir, 'chain-10000.asl.json')]);
    if (g.code !== 0) throw new Error('gen_scale.py: ' + g.err.slice(0, 200));
    const t = scJson(['eval', dir]).timing_us;
    info('10,000-state workflow, pipeline time', `${(t.mean / 1000).toFixed(1)} ms`, 'about 31 ms', 'Sec. 4.4');
  });
  const pm4py = sh(PY, ['-c', 'import pm4py']).code === 0;
  if (!pm4py) {
    skip('industrial build gate (Sec. 4.4)', `needs Python with pm4py 2.7.23.1 (use the Docker image)`);
  } else {
    guard('industrial build gate', () => {
      const out = path.join(OUT, 'industrial-case.json');
      const r = sh(PY, ['eval/industrial/wse.py', '--out', out], { env: { ...process.env, STEPCHECK_BIN: BIN } });
      if (r.code !== 0) throw new Error('wse.py: ' + r.err.slice(-300));
      const j = JSON.parse(fs.readFileSync(out, 'utf8'));
      const states = +(/total states\s*:\s*(\d+)/.exec(sc(['stats', 'corpus/industrial'])) || [])[1];   // recursive count
      check('industrial-topology workflows, states', [j.workflows_detail.length, states], [6, 65], 'Sec. 4.4');
      const g = j.build_time_gate.by_injected_defects;
      check('gate catch rate at 1 and 5 injected defects', [g['1'].catch_rate, g['5'].catch_rate], [1, 1], 'Sec. 4.4');
      const p95 = Math.max(...Object.values(g).map((x) => x.gate_latency_ms.p95));
      info('gate latency p95 (process start included)', `${p95} ms`, 'below 21 ms', 'Sec. 4.4');
    });
  }
}

// ================================================================ tier 2
if (TIER >= 2) {
  head('Tier 2: schema validators (Table 1, AWS / sl / av columns)');
  guard('validator panel', () => {
    const out = path.join(OUT, 'validator-panel.json');
    const r = sh('node', ['eval/validator_panel.js', 'corpus/asl', '--out', out], { env: { ...process.env, STEPCHECK_BIN: BIN } });
    if (r.code !== 0) throw new Error('validator_panel.js: ' + r.err.slice(-300));
    const v = JSON.parse(fs.readFileSync(out, 'utf8'));
    const rec = readJson('eval/validator-panel.json');
    for (const [cls, row] of Object.entries(TABLE1)) {
      const m = v.mutation_detection[PANEL_KEY[cls]];
      for (const [tool, idx] of [['statelint', 2], ['asl-validator', 3]]) {
        if (!v.tools[tool]) { skip(`${tool} ${cls}`, 'tool not installed'); continue; }
        check(`${tool} ${cls}: recall`, r2(m[tool].detected / m.applicable), row[idx], 'Table 1');
      }
      const aws = v.tools.aws ? m.aws : rec.mutation_detection[PANEL_KEY[cls]].aws;
      (v.tools.aws ? check : recorded)(`AWS Validate ${cls}: recall`, r2(aws.detected / m.applicable), row[1], 'Table 1');
    }
  });

  head('Tier 2: workflow-soundness verifiers (Table 1, BPV / BPMN / Wof columns)');
  // BProVe runs a Java parser and a Maude LTL model check per net: about two minutes per
  // workflow, so the full corpus takes about six hours. By default it runs on a sample whose
  // per-workflow verdicts are compared with the recorded run; --bprove-full runs all 193.
  const BPROVE_FULL = argv.includes('--bprove-full');
  const bproveOn = !!(process.env.BPROVE_PARSER && process.env.BPROVE_MAUDE_MODEL);
  const compare = (extra, out, withBprove) => {
    const env = { ...process.env };
    if (!withBprove) { env.BPROVE_PARSER = ''; env.BPROVE_MAUDE_MODEL = ''; }
    const r = sh(PY, ['eval/asl2bpmn/compare.py', '--limit', '193', '--out', out, '--bin', BIN, ...extra], { env });
    if (r.code !== 0) throw new Error('compare.py: ' + r.err.slice(-300));
    return JSON.parse(fs.readFileSync(out, 'utf8'));
  };
  guard('Woflan and BPMN Analyzer', () => {
    const c = compare([], path.join(OUT, 'asl2bpmn-comparison-full.json'), BPROVE_FULL && bproveOn);
    const tools = [['bpmn_analyzer', 5], ['woflan', 6]];
    if (BPROVE_FULL && bproveOn) tools.unshift(['bprove', 4]);
    for (const [cls, row] of Object.entries(TABLE1)) {
      const k = c.classes[PANEL_KEY[cls]];
      for (const [tool, idx] of tools) {
        if (tool === 'bpmn_analyzer' && !process.env.BPMN_ANALYZER) { skip(`${tool} ${cls}`, 'tool not configured'); continue; }
        check(`${tool} ${cls}: fresh violations / n`, r2(k[`${tool}_detected`] / k.applicable), row[idx], 'Table 1');
      }
    }
    const cb = c.clean_baseline;
    check('valid workflows reported unsound: BPMN, Wof', [cb.bpmn_analyzer_unsound, cb.woflan_unsound], [16, 16], 'Table 1, last row');
    if (BPROVE_FULL && bproveOn) check('valid workflows reported unsound: BPV', cb.bprove_unsound, 56, 'Table 1, last row');
  });
  if (!bproveOn) {
    skip('BProVe', 'BPROVE_PARSER / BPROVE_MAUDE_MODEL not set (use the Docker image)');
  } else if (!BPROVE_FULL) {
    guard('BProVe sample', () => {
      // Deterministic sample: every k-th workflow among those BProVe called sound and
      // among those it called unsound in the recorded run, so both verdicts are exercised.
      const n = parseInt(process.env.BPROVE_SAMPLE || '6', 10);
      const rec = readJson('eval/asl2bpmn-comparison-full.json').rows;
      const pick = (rows, m) => rows.filter((_, i) => i % Math.max(1, Math.floor(rows.length / m)) === 0).slice(0, m);
      const unsound = rec.filter((r) => r.control_bprove_sound !== true), sound = rec.filter((r) => r.control_bprove_sound === true);
      const sample = [...pick(unsound, Math.floor(n / 2)), ...pick(sound, n - Math.floor(n / 2))];
      const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const include = '^(' + sample.map((r) => esc(r.file)).join('|') + ')$';
      const c = compare(['--include', include], path.join(OUT, 'asl2bpmn-bprove-sample.json'), true);
      const verdicts = (row) => [String(row.control_bprove_sound), ...Object.keys(row.classes).sort().map((k) => `${k}:${row.classes[k].bprove}`)];
      const recBy = new Map(rec.map((r) => [r.file, verdicts(r)]));
      const differing = c.rows.filter((r) => JSON.stringify(verdicts(r)) !== JSON.stringify(recBy.get(r.file))).map((r) => path.basename(r.file));
      check(`BProVe, ${c.rows.length}-workflow sample: verdicts unlike recorded`, differing, [], 'eval/asl2bpmn-comparison-full.json');
      const cb = readJson('eval/asl2bpmn-comparison-full.json').clean_baseline;
      recorded('BProVe: valid workflows reported unsound', cb.bprove_unsound, 56, 'Table 1, last row');
    });
  }
}

// ================================================================ tier 3
if (TIER >= 3) {
  head('Tier 3: unredistributed corpus files (network)');
  guard('fetch', () => {
    const r = sh('node', ['eval/fetch_unredistributed.js']);
    check('36 files fetched at pinned commits, hashes verified', r.code, 0, 'corpus/PROVENANCE.md');
    if (r.code !== 0) console.log(r.out);
  });
  guard('independent repositories', () => {
    const w = scJson(['scan', 'corpus/wild-external', '--infer'], 'scan-wild-external.json');
    const rec = readJson('eval/results-wild-external.json');
    check('independent ASL: definitions', w.files, 95, 'Sec. 4.3');
    check('independent ASL: findings by code match the recorded run', w.code_totals, rec.code_totals, 'eval/results-wild-external.json');
  });
  guard('fix-commit pairs', () => {
    const p = scJson(['eval-pairs', 'corpus/realbugs', '--infer'], 'realbugs-pairs-full.json');
    check('kept pairs (6 genuine + 1 demonstrator): caught', [p.caught, p.pairs], [7, 7], 'Sec. 4.3');
  });
  guard('fix-commit mining', () => {
    // Re-mine the 30 reproducible pairs (git history up to the study date) and replay them.
    const man = readJson('corpus/realbugs/mined-pairs.json');
    const repos = [...new Set(man.pairs.map((x) => x.repo))].sort().join(',');
    const dir = path.join(OUT, 'realbugs-remined');
    fs.rmSync(dir, { recursive: true, force: true });
    const r = sh('node', ['eval/mine_realbugs.js', '--repos', repos, '--until', '2026-06-22', '--out', dir]);
    if (r.code !== 0) throw new Error('mine_realbugs.js: ' + r.err.slice(-300));
    const ids = fs.readdirSync(dir).filter((f) => f.endsWith('-pre.json')).map((f) => f.slice(0, -'-pre.json'.length)).sort();
    check('re-mined pairs identical to corpus/realbugs/mined-pairs.json', ids, man.pairs.map((x) => x.id).sort(), 'Sec. 4.3');
    const rep = scJson(['eval-pairs', dir, '--infer'], 'realbugs-remined-pairs.json');
    const flagged = rep.results.filter((x) => x.fixed_codes.length).map((x) => x.id).sort();
    check('re-mined pairs: flagged ones match the study', flagged, man.pairs.filter((x) => x.fixed_codes.length).map((x) => x.id).sort(), 'Sec. 4.3');
    recorded('fix-commit pairs from the unrecorded discovery search (none flagged)', man.unrecorded_discovery_pairs, 9, 'corpus/realbugs/mined-pairs.json');
  });
}

// ---------------------------------------------------------------- summary
const count = (s) => results.filter((r) => r.status === s).length;
fs.writeFileSync(path.join(OUT, 'report.json'), JSON.stringify({ tier: TIER, binary: BIN, results }, null, 2));
console.log(`\n${count('PASS')} PASS, ${count('FAIL')} FAIL, ${count('RECORDED')} RECORDED, ${count('INFO')} INFO, ${count('SKIP')} SKIP` +
  `  -> ${path.relative(ROOT, path.join(OUT, 'report.json'))}`);
process.exit(count('FAIL') ? 1 : 0);
