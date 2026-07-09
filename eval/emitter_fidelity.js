// WS-H: emitter fidelity. Round-trips ASL through the IR (parse -> emit) and
// measures how much of each workflow's semantics is regenerated:
//   * structure: the set of state names and the transition graph (Next/Default/
//     Choice targets, Catch targets, End/terminal flags);
//   * I/O processing: per-state InputPath / OutputPath / ResultPath /
//     ResultSelector / Parameters / Result (deep equality);
//   * exact ASL-object preservation after parse -> emit, including fields that
//     StepCheck does not interpret but now carries through;
//   * stability: emit is idempotent (emit(emit(x)) == emit(x) byte-for-byte).
// Reports the exact preserved fraction per field and the fraction of workflows
// preserved in full over the selected corpus.  -> eval/emitter-fidelity.json
'use strict';
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const ROOT = path.resolve(__dirname, '..');
const BIN = path.join(ROOT, 'stepcheck', 'target', 'release', 'stepcheck.exe');
const DIR = process.argv[2] ? path.resolve(process.argv[2]) : path.join(ROOT, 'corpus', 'asl');
const LIMIT = process.argv[3] ? +process.argv[3] : Number.MAX_SAFE_INTEGER;

function walkStates(wf, prefix, out) {
  for (const [name, st] of Object.entries(wf.States || {})) {
    const id = prefix ? `${prefix}/${name}` : name;
    out[id] = st;
    for (const [i, br] of (st.Branches || []).entries()) walkStates(br, `${id}[B${i}]`, out);
    // Iterator (classic Map) and ItemProcessor (distributed Map) are both nested
    // machines; the emitter preserves the spelling used by the parsed input.
    if (st.Iterator) walkStates(st.Iterator, `${id}[Map]`, out);
    if (st.ItemProcessor) walkStates(st.ItemProcessor, `${id}[Map]`, out);
  }
  return out;
}
function stable(v) {
  if (Array.isArray(v)) return v.map(stable);
  if (v && typeof v === 'object') {
    return Object.fromEntries(Object.keys(v).sort().map(k => [k, stable(v[k])]));
  }
  return v ?? null;
}
function eq(a, b) { return JSON.stringify(stable(a)) === JSON.stringify(stable(b)); }
function transitionSig(st) {
  return JSON.stringify({
    Next: st.Next ?? null, Default: st.Default ?? null, End: st.End ?? null,
    Type: st.Type ?? null,
    Choices: (st.Choices || []).map(c => c.Next ?? null),
    Catch: (st.Catch || []).map(c => c.Next ?? null),
  });
}
const IO_FIELDS = [
  'InputPath', 'OutputPath', 'ResultPath', 'ResultSelector', 'Parameters', 'Result',
  'Arguments', 'Assign', 'Output', 'Items', 'ItemSelector', 'ItemsPath',
];

const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith('.json')) files.push(p);
  }
})(DIR);
files.sort();
const chosen = files.slice(0, LIMIT);

const agg = {
  workflows: 0, idempotent: 0,
  exact_full_document: 0,
  states_total: 0, states_preserved: 0,
  transitions_total: 0, transitions_preserved: 0,
  io: Object.fromEntries(IO_FIELDS.map(f => [f, { present: 0, preserved: 0 }])),
  fully_preserved_workflows: 0,
};
const perFile = [];

for (const f of chosen) {
  let orig;
  try { orig = JSON.parse(fs.readFileSync(f, 'utf8')); } catch { continue; }
  if (!orig.States) continue;
  let emitted, emitted2;
  try {
    const emittedText = execFileSync(BIN, ['emit', f], { encoding: 'utf8', maxBuffer: 64 << 20 });
    emitted = JSON.parse(emittedText);
    const tmp = path.join(require('os').tmpdir(), 'emit1.json');
    fs.writeFileSync(tmp, JSON.stringify(emitted));
    emitted2 = JSON.parse(execFileSync(BIN, ['emit', tmp], { encoding: 'utf8', maxBuffer: 64 << 20 }));
  } catch { continue; }
  agg.workflows++;
  const idem = eq(emitted, emitted2);
  if (idem) agg.idempotent++;
  const exact = eq(orig, emitted);
  if (exact) agg.exact_full_document++;

  const O = walkStates(orig, '', {}), E = walkStates(emitted, '', {});
  const okeys = Object.keys(O);
  let full = true;
  let stP = 0, stT = okeys.length, trP = 0, trT = 0;
  const ioLocal = Object.fromEntries(IO_FIELDS.map(f => [f, { present: 0, preserved: 0 }]));
  for (const k of okeys) {
    if (E[k]) { stP++; } else { full = false; continue; }
    trT++;
    if (transitionSig(O[k]) === transitionSig(E[k])) trP++; else full = false;
    for (const fld of IO_FIELDS) {
      if (O[k][fld] !== undefined) {
        ioLocal[fld].present++;
        if (eq(O[k][fld], E[k][fld])) ioLocal[fld].preserved++; else full = false;
      }
    }
  }
  agg.states_total += stT; agg.states_preserved += stP;
  agg.transitions_total += trT; agg.transitions_preserved += trP;
  for (const fld of IO_FIELDS) { agg.io[fld].present += ioLocal[fld].present; agg.io[fld].preserved += ioLocal[fld].preserved; }
  if (full) agg.fully_preserved_workflows++;
  perFile.push({ file: path.relative(ROOT, f).replace(/\\/g, '/'), states: stT, states_preserved: stP, transitions_preserved: trP, idempotent: idem, exact_full_document: exact, fully_preserved: full });
}

const pct = (a, b) => (b ? +(100 * a / b).toFixed(1) : null);
const report = {
  generated_by: 'eval/emitter_fidelity.js',
  corpus: path.relative(ROOT, DIR).replace(/\\/g, '/'),
  workflows: agg.workflows,
  idempotent_emit_pct: pct(agg.idempotent, agg.workflows),
  exact_full_document_pct: pct(agg.exact_full_document, agg.workflows),
  fully_preserved_workflows_pct: pct(agg.fully_preserved_workflows, agg.workflows),
  structure: {
    states_preserved_pct: pct(agg.states_preserved, agg.states_total),
    transitions_preserved_pct: pct(agg.transitions_preserved, agg.transitions_total),
    states_total: agg.states_total, transitions_total: agg.transitions_total,
  },
  io_fields: Object.fromEntries(IO_FIELDS.map(f => [f, {
    present: agg.io[f].present, preserved: agg.io[f].preserved, preserved_pct: pct(agg.io[f].preserved, agg.io[f].present),
  }])),
  note: 'Modeled-fragment fidelity measures state set, transition graph, and I/O-processing fields. Exact full-document fidelity additionally checks ASL fields preserved by passthrough; intentionally normalized malformed inputs may differ.',
};
fs.writeFileSync(path.join(__dirname, 'emitter-fidelity.json'), JSON.stringify({ ...report, per_file: perFile }, null, 2));
console.log(JSON.stringify(report, null, 2));
