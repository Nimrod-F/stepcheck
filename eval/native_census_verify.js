'use strict';

// Code-disjoint census verifier for the 45 native-tier StepCheck findings on the
// 193-workflow AWS corpus. For every native finding in
// eval/diagnostic-precision-population.json this re-checks the flagged structural
// predicate directly against the referenced ASL source, independently of the tool's
// own analysis passes. It answers one question per finding: does the ASL actually
// exhibit the flagged construct at the referenced state?
//
//   node eval/native_census_verify.js
//
// Output: eval/diagnostic-precision-census.native.json + a console summary.

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const POPULATION = path.join(__dirname, 'diagnostic-precision-population.json');
const OUT = path.join(__dirname, 'diagnostic-precision-census.native.json');

function loadJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

// Resolve a StepCheck state path ("Parallel[Branch0]/Task", "Map[Map]/Task", "Name")
// to the concrete state object in the ASL. Returns undefined if unresolvable.
function resolveState(machine, statePath) {
  let states = machine.States || {};
  const segments = String(statePath).split('/');
  let node;
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    const m = seg.match(/^(.*?)(?:\[Branch(\d+)\]|\[Map\])?$/);
    const name = m[1];
    const branch = m[2];
    node = states ? states[name] : undefined;
    if (node === undefined) return undefined;
    const isLast = i === segments.length - 1;
    if (!isLast) {
      if (branch !== undefined) {
        const b = (node.Branches || [])[Number(branch)];
        states = b ? b.States : undefined;
      } else {
        // Map iterator: ItemProcessor (current) or Iterator (legacy)
        const proc = node.ItemProcessor || node.Iterator;
        states = proc ? proc.States : undefined;
      }
    }
  }
  return node;
}

function isCallbackTask(state) {
  if (!state || typeof state !== 'object') return false;
  const res = state.Resource;
  if (typeof res === 'string' && res.includes('.waitForTaskToken')) return true;
  // Fallback: an explicit task-token hand-off in Parameters/Arguments.
  const blob = JSON.stringify(state.Parameters || state.Arguments || {});
  return /\$\$\.Task\.Token|"TaskToken/.test(blob);
}

function hasBound(state) {
  return ['TimeoutSeconds', 'TimeoutSecondsPath', 'HeartbeatSeconds', 'HeartbeatSecondsPath']
    .some(k => Object.prototype.hasOwnProperty.call(state, k));
}

// Collect the startExecution child ARN(s) reachable inside one parallel branch.
function branchChildArns(branchStates) {
  const arns = [];
  for (const s of Object.values(branchStates || {})) {
    if (s && typeof s === 'object' && typeof s.Resource === 'string' &&
        s.Resource.includes('states:startExecution')) {
      const arn = (s.Parameters && s.Parameters.StateMachineArn) ||
                  (s.Arguments && s.Arguments.StateMachineArn);
      if (arn) arns.push(arn);
    }
  }
  return arns;
}

const CHECKS = {
  // Choice with no Default: an unmatched input fails with States.NoChoiceMatched.
  SC0007(machine, finding) {
    const st = resolveState(machine, finding.state);
    if (!st || typeof st !== 'object') return { tp: false, why: 'state not found' };
    if (st.Type !== 'Choice') return { tp: false, why: `state Type is ${st.Type}, not Choice` };
    const hasDefault = Object.prototype.hasOwnProperty.call(st, 'Default');
    return { tp: !hasDefault, why: hasDefault ? 'has a Default branch' : 'Choice without Default' };
  },
  // Machine-level directive misplaced inside States (not a valid state object).
  SC0010(machine, finding) {
    const val = (machine.States || {})[finding.state];
    if (val === undefined) return { tp: false, why: 'key not present in States' };
    const isStateObject = val && typeof val === 'object' && typeof val.Type === 'string';
    return { tp: !isStateObject, why: isStateObject ? 'is a valid state object' : 'not a valid state object' };
  },
  // Two parallel branches concurrently start the same child state machine.
  SC5003(machine, finding) {
    const st = resolveState(machine, finding.state);
    if (!st || st.Type !== 'Parallel') return { tp: false, why: 'not a Parallel state' };
    const perBranch = (st.Branches || []).map(b => branchChildArns(b.States));
    const seen = new Map();
    for (let i = 0; i < perBranch.length; i++) {
      for (const arn of perBranch[i]) {
        if (seen.has(arn) && seen.get(arn) !== i) {
          return { tp: true, why: `branches ${seen.get(arn)} and ${i} start ${arn}` };
        }
        seen.set(arn, i);
      }
    }
    return { tp: false, why: 'no shared child ARN across branches' };
  },
  // Callback/activity wait with no TimeoutSeconds/HeartbeatSeconds bound.
  SC6001(machine, finding) {
    const st = resolveState(machine, finding.state);
    if (!st || typeof st !== 'object') return { tp: false, why: 'state not found' };
    if (!isCallbackTask(st)) return { tp: false, why: 'not a callback (.waitForTaskToken) task' };
    const bounded = hasBound(st);
    return { tp: !bounded, why: bounded ? 'has a Timeout/Heartbeat bound' : 'callback with no Timeout/Heartbeat' };
  },
};

function main() {
  const population = loadJson(POPULATION);
  const native = (population.findings || []).filter(f => f.tier === 'native');
  const machineCache = new Map();

  const results = [];
  for (const f of native) {
    const abs = path.join(ROOT, f.file);
    if (!machineCache.has(abs)) machineCache.set(abs, loadJson(abs));
    const machine = machineCache.get(abs);
    const check = CHECKS[f.code];
    const verdict = check ? check(machine, f)
                          : { tp: null, why: `no verifier for ${f.code}` };
    results.push({
      audit_id: f.audit_id,
      workflow_id: f.workflow_id,
      source_path: f.source_path,
      code: f.code,
      state: f.state,
      structural_tp: verdict.tp,
      basis: verdict.why,
    });
  }

  const byCode = {};
  for (const r of results) {
    const c = (byCode[r.code] ||= { n: 0, tp: 0, fp: 0, unknown: 0 });
    c.n++;
    if (r.structural_tp === true) c.tp++;
    else if (r.structural_tp === false) c.fp++;
    else c.unknown++;
  }
  const total = { n: results.length, tp: 0, fp: 0, unknown: 0 };
  for (const c of Object.values(byCode)) { total.tp += c.tp; total.fp += c.fp; total.unknown += c.unknown; }

  const report = {
    generated_at: new Date().toISOString(),
    generated_by: 'eval/native_census_verify.js',
    description: 'Code-disjoint structural re-verification of all native-tier findings; ' +
                 'structural_tp=true iff the ASL exhibits the flagged construct at the referenced state.',
    population_file: 'eval/diagnostic-precision-population.json',
    scope: 'native (census of all eligible findings)',
    totals: { by_code: byCode, overall: total },
    findings: results,
  };
  fs.writeFileSync(OUT, JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report.totals, null, 2));
  const mismatches = results.filter(r => r.structural_tp !== true);
  if (mismatches.length) {
    console.log('\nNon-confirmed findings:');
    for (const r of mismatches) console.log(`  ${r.audit_id} ${r.code} ${r.workflow_id} ${r.state} :: ${r.basis}`);
  } else {
    console.log('\nAll native findings structurally confirmed.');
  }
  console.log(`\n-> ${path.relative(ROOT, OUT).replace(/\\/g, '/')}`);
}

main();
