'use strict';

// ASL -> BPMN 2.0 encoder for the control-flow baselines (Woflan, BPMN Analyzer, BProVe).
//
// The encoding targets a *workflow net* shape so that a Petri-net soundness
// verifier (Woflan, via pm4py) can analyse it: exactly one start event, exactly
// one end event, and every node reachable and on a path to the end. The mapping
// rules are:
//
//   A. Task/Pass/Wait   -> activity (task/intermediateCatchEvent)
//   B. Choice           -> exclusiveGateway with one guarded sequence flow per
//                          Choices[] rule plus Default (guards kept as labels)
//   C. Parallel         -> parallelGateway split ... branches ... parallelGateway join
//   D. Map              -> activity placeholder (item data flow out of scope)
//   E. Retry/Catch      -> Catch is lowered to an exclusiveGateway after the task
//                          (normal completion vs. one error branch per Catch), so
//                          error handling stays connected in the token flow rather
//                          than dangling on a boundary event that the PN converter
//                          drops. Retry is lossy metadata (no control-flow change).
//
// All terminals (End:true, Succeed, Fail, a state with no successor) are merged
// into the single end event. Data payloads, JSONPath provenance, guard
// executability and retry counts are intentionally NOT encoded -- BPMN is used
// only for the control-flow-soundness fair class; data-flow (SC1101) is
// StepCheck-unique and has no BPMN expression.

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..', '..');

function usage() {
  console.error(`Usage:
  node eval/asl2bpmn/encode.js <workflow.asl.json|dir> [--limit N] [--out DIR] [--summary PATH]

Examples:
  node eval/asl2bpmn/encode.js corpus/asl --limit 30 --out eval/asl2bpmn/out --summary eval/asl2bpmn-summary.json
  node eval/asl2bpmn/encode.js corpus/asl/example.asl.json > example.bpmn`);
}

function parseArgs(argv) {
  const opts = { input: null, limit: null, out: null, summary: null };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--limit') opts.limit = Number(argv[++i]);
    else if (a === '--out') opts.out = path.resolve(argv[++i]);
    else if (a === '--summary') opts.summary = path.resolve(argv[++i]);
    else if (!opts.input) opts.input = path.resolve(a);
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!opts.input) throw new Error('missing input path');
  if (opts.limit != null && (!Number.isInteger(opts.limit) || opts.limit <= 0)) throw new Error('--limit must be a positive integer');
  return opts;
}

function xml(s) {
  return String(s == null ? '' : s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}

function safeId(s) {
  const id = String(s).replace(/[^A-Za-z0-9_]+/g, '_').replace(/^_+|_+$/g, '');
  return id || 'x';
}

function isWorkflow(v) {
  return v && typeof v === 'object' && typeof v.StartAt === 'string' && v.States && typeof v.States === 'object';
}

function readWorkflow(file) {
  const raw = fs.readFileSync(file, 'utf8');
  const json = JSON.parse(raw);
  if (!isWorkflow(json)) throw new Error('not an ASL workflow object');
  return json;
}

function walkFiles(dir) {
  const out = [];
  function go(d) {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) go(p);
      else if (e.name.endsWith('.json')) out.push(p);
    }
  }
  go(dir);
  out.sort();
  return out;
}

function conditionName(rule, i) {
  if (!rule || typeof rule !== 'object') return `choice ${i}`;
  if (rule.Variable) {
    const op = Object.keys(rule).find(k => k !== 'Variable' && k !== 'Next' && k !== 'Comment') || 'condition';
    const val = rule[op];
    return `${rule.Variable} ${op}${val == null || typeof val === 'object' ? '' : ` ${val}`}`;
  }
  if (rule.Condition) return String(rule.Condition).slice(0, 80);
  if (rule.And) return `And[${rule.And.length}]`;
  if (rule.Or) return `Or[${rule.Or.length}]`;
  if (rule.Not) return 'Not';
  return `choice ${i}`;
}

function countStates(wf, stats) {
  for (const st of Object.values(wf.States || {})) {
    stats.states++;
    const type = st.Type || 'Unknown';
    stats.byType[type] = (stats.byType[type] || 0) + 1;
    if (type === 'Choice') stats.choices++;
    if (type === 'Parallel') stats.parallel++;
    if (type === 'Map') stats.map++;
    if (Array.isArray(st.Retry) && st.Retry.length) stats.retry++;
    if (Array.isArray(st.Catch) && st.Catch.length) stats.catch++;
    if (st.QueryLanguage === 'JSONata') stats.jsonata++;
    if (st.Iterator) countStates(st.Iterator, stats);
    if (st.ItemProcessor && st.ItemProcessor.States) countStates(st.ItemProcessor, stats);
    if (Array.isArray(st.Branches)) for (const br of st.Branches) countStates(br, stats);
  }
}

class BpmnBuilder {
  constructor(wf, name) {
    this.wf = wf;
    this.name = name;
    this.processId = `Process_${safeId(name)}`;
    this.defId = `Definitions_${safeId(name)}`;
    this.nodes = new Map();
    this.flows = [];
    this.notes = [];
    this.endId = null;
    this.stats = {
      states: 0,
      byType: {},
      choices: 0,
      parallel: 0,
      map: 0,
      retry: 0,
      catch: 0,
      jsonata: 0,
      flows: 0,
      lossy: {
        dataConditions: 0,
        retries: 0,
        mapIterators: 0,
        jsonata: 0,
        catchGateways: 0,
      },
    };
    countStates(wf, this.stats);
  }

  id(scope, name) {
    return safeId(`${scope}_${name}`);
  }

  // Entry node id for a state referenced by name in `scope`. A Parallel state is
  // entered through its split gateway; a state carrying Catch is entered through
  // its activity node (the catch gateway sits *after* it).
  stateEntry(scope, name, st) {
    if (st && st.Type === 'Parallel') return this.id(scope, `${name}_split`);
    return this.id(scope, name);
  }

  addNode(type, id, name, extra = '') {
    if (!this.nodes.has(id)) this.nodes.set(id, { type, id, name, incoming: [], outgoing: [], extra });
    return id;
  }

  addFlow(sourceRef, targetRef, name) {
    // A transition to a state that does not exist (e.g. a dangling `Next` after
    // a structural mutation) is materialised as a dead-end placeholder node so
    // the net still *builds*: Woflan then genuinely analyses it and reports the
    // improper completion / extra sink, rather than the encoder short-circuiting.
    if (!this.nodes.has(targetRef)) {
      this.addNode('bpmn:task', targetRef, `dangling ${targetRef}`);
      this.danglingTargets = (this.danglingTargets || []);
      if (!this.danglingTargets.includes(targetRef)) this.danglingTargets.push(targetRef);
    }
    const id = `Flow_${safeId(`${sourceRef}_${targetRef}_${this.flows.length}`)}`;
    this.flows.push({ id, sourceRef, targetRef, name });
    const s = this.nodes.get(sourceRef);
    const t = this.nodes.get(targetRef);
    if (s) s.outgoing.push(id);
    if (t) t.incoming.push(id);
    return id;
  }

  stateNodeType(st) {
    switch (st.Type) {
      case 'Task': return 'bpmn:task';
      case 'Pass': return 'bpmn:task';
      // Wait is a pass-through activity for control-flow soundness. (An
      // intermediateCatchEvent would be a truer BPMN type, but the PN converter
      // splits token flow through catch events and orphans the successor.)
      case 'Wait': return 'bpmn:task';
      case 'Choice': return 'bpmn:exclusiveGateway';
      case 'Map': return 'bpmn:task';           // placeholder activity (item flow out of scope)
      case 'Succeed': return 'bpmn:task';
      case 'Fail': return 'bpmn:task';
      default: return 'bpmn:task';
    }
  }

  extraForState(st) {
    return '';
  }

  lookup(scope, name) {
    return this.scopeStates.get(scope)?.[name];
  }

  // Route a terminal (End / Succeed / Fail / no-successor) into the single end
  // for its scope: `exitTarget` for a branch, the shared end event for the top.
  terminalTarget(exitTarget) {
    return exitTarget || this.endId;
  }

  lowerMachine(wf, scope, exitTarget) {
    this.scopeStates = this.scopeStates || new Map();
    this.scopeStates.set(scope, wf.States || {});

    // 1) create a node (or split/join pair) for every state.
    for (const [name, st] of Object.entries(wf.States || {})) {
      if (st.Type === 'Parallel') {
        this.addNode('bpmn:parallelGateway', this.id(scope, `${name}_split`), name);
        this.addNode('bpmn:parallelGateway', this.id(scope, `${name}_join`), `${name} join`);
      } else {
        this.addNode(this.stateNodeType(st), this.id(scope, name), name, this.extraForState(st));
        if (st.Type === 'Map') {
          this.stats.lossy.mapIterators++;
          if (st.Iterator || st.ItemProcessor) this.notes.push(`Map ${scope}/${name} encoded as activity placeholder`);
        }
        if (st.QueryLanguage === 'JSONata') this.stats.lossy.jsonata++;
        if (Array.isArray(st.Retry) && st.Retry.length) this.stats.lossy.retries++;
      }
    }

    // 2) wire control flow.
    for (const [name, st] of Object.entries(wf.States || {})) {
      if (st.Type === 'Parallel') {
        const split = this.id(scope, `${name}_split`);
        const join = this.id(scope, `${name}_join`);
        (st.Branches || []).forEach((br, i) => {
          const branchScope = `${scope}_${safeId(name)}_branch${i}`;
          this.lowerMachine(br, branchScope, join);
          this.addFlow(split, this.stateEntry(branchScope, br.StartAt, br.States?.[br.StartAt]), `branch ${i + 1}`);
        });
        if (!Array.isArray(st.Branches) || st.Branches.length === 0) {
          // keep the net connected: an empty parallel just falls through
          this.addFlow(split, join, 'empty');
          this.notes.push(`Parallel ${scope}/${name} has no encodable branches`);
        }
        this.wireExit(scope, join, st, exitTarget);
        continue;
      }

      if (st.Type === 'Choice') {
        const src = this.id(scope, name);
        (st.Choices || []).forEach((r, i) => {
          if (r.Next) {
            this.stats.lossy.dataConditions++;
            this.addFlow(src, this.stateEntry(scope, r.Next, this.lookup(scope, r.Next)), conditionName(r, i));
          }
        });
        if (st.Default) this.addFlow(src, this.stateEntry(scope, st.Default, this.lookup(scope, st.Default)), 'default');
        continue;
      }

      // Task/Pass/Wait/Map/Succeed/Fail: the activity node, then Catch as XOR.
      const activity = this.id(scope, name);
      if (Array.isArray(st.Catch) && st.Catch.length) {
        // exclusiveGateway after the activity: ok -> normal exit, err_i -> handler.
        const gw = this.id(scope, `${name}_catchgw`);
        this.addNode('bpmn:exclusiveGateway', gw, `${name} error?`);
        this.stats.lossy.catchGateways++;
        this.addFlow(activity, gw);
        this.wireExit(scope, gw, st, exitTarget, 'ok');
        st.Catch.forEach((c, i) => {
          if (c.Next) this.addFlow(gw, this.stateEntry(scope, c.Next, this.lookup(scope, c.Next)), `catch ${i + 1}`);
          else this.addFlow(gw, this.terminalTarget(exitTarget), `catch ${i + 1}`);
        });
      } else {
        this.wireExit(scope, activity, st, exitTarget);
      }
    }
  }

  // Wire a source node to the state's normal successor / terminal.
  wireExit(scope, source, st, exitTarget, label) {
    if (st.Next) {
      this.addFlow(source, this.stateEntry(scope, st.Next, this.lookup(scope, st.Next)), label);
    } else if (st.End === true || st.Type === 'Succeed' || st.Type === 'Fail') {
      this.addFlow(source, this.terminalTarget(exitTarget), label);
    } else {
      // no successor and not explicitly terminal: still route to the end so the
      // net stays connected (mirrors ASL's implicit terminal-on-missing-next).
      this.addFlow(source, this.terminalTarget(exitTarget), label);
    }
  }

  build() {
    const startId = this.id('top', 'start');
    this.endId = this.id('top', 'end');
    this.addNode('bpmn:startEvent', startId, 'Start');
    this.addNode('bpmn:endEvent', this.endId, 'End');
    this.lowerMachine(this.wf, 'top', null);
    this.addFlow(startId, this.stateEntry('top', this.wf.StartAt, this.wf.States?.[this.wf.StartAt]));
    this.stats.flows = this.flows.length;
    const validation = this.validate();
    return { xml: this.render(), stats: this.stats, notes: this.notes, validation };
  }

  validate() {
    const ids = new Set(this.nodes.keys());
    const missing = [];
    for (const f of this.flows) {
      if (!ids.has(f.sourceRef)) missing.push(`${f.id}: missing source ${f.sourceRef}`);
      if (!ids.has(f.targetRef)) missing.push(`${f.id}: missing target ${f.targetRef}`);
    }
    const starts = [...this.nodes.values()].filter(n => n.type === 'bpmn:startEvent').length;
    const ends = [...this.nodes.values()].filter(n => n.type === 'bpmn:endEvent').length;
    // orphan = a non-start node with no incoming flow (would be an extra PN source)
    const orphans = [...this.nodes.values()]
      .filter(n => n.type !== 'bpmn:startEvent' && n.incoming.length === 0)
      .map(n => n.id);
    const dangling = this.danglingTargets || [];
    return {
      ok: missing.length === 0 && starts === 1 && ends === 1 && orphans.length === 0 && dangling.length === 0,
      starts,
      ends,
      nodes: this.nodes.size,
      flows: this.flows.length,
      orphans,
      dangling,
      missing,
    };
  }

  renderNode(n) {
    const direction = n.type === 'bpmn:exclusiveGateway'
      ? (n.incoming.length > 1 ? 'Converging' : 'Diverging')
      : n.type === 'bpmn:parallelGateway'
        ? (n.incoming.length > 1 ? 'Converging' : 'Diverging')
        : null;
    const attrs = `id="${xml(n.id)}" name="${xml(n.name)}"${direction ? ` gatewayDirection="${direction}"` : ''}`;
    const body = [
      ...n.incoming.map(id => `    <bpmn:incoming>${xml(id)}</bpmn:incoming>`),
      ...n.outgoing.map(id => `    <bpmn:outgoing>${xml(id)}</bpmn:outgoing>`),
      n.extra ? `    ${n.extra}` : null,
    ].filter(Boolean).join('\n');
    if (!body) return `  <${n.type} ${attrs} />`;
    return `  <${n.type} ${attrs}>\n${body}\n  </${n.type}>`;
  }

  render() {
    const flowXml = this.flows.map(f => {
      const name = f.name ? ` name="${xml(f.name)}"` : '';
      return `  <bpmn:sequenceFlow id="${xml(f.id)}" sourceRef="${xml(f.sourceRef)}" targetRef="${xml(f.targetRef)}"${name} />`;
    });
    const notes = this.notes.length
      ? `  <bpmn:documentation>${xml(this.notes.join('; '))}</bpmn:documentation>`
      : `  <bpmn:documentation>Generated by eval/asl2bpmn/encode.js for control-flow soundness baselines. Data payload semantics are intentionally out of scope.</bpmn:documentation>`;
    return [
      '<?xml version="1.0" encoding="UTF-8"?>',
      `<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" id="${xml(this.defId)}" targetNamespace="https://stepcheck.example/asl2bpmn">`,
      `<bpmn:process id="${xml(this.processId)}" isExecutable="false">`,
      notes,
      ...[...this.nodes.values()].map(n => this.renderNode(n)),
      ...flowXml,
      '</bpmn:process>',
      '</bpmn:definitions>',
      '',
    ].join('\n');
  }
}

function encodeWorkflow(file) {
  const wf = readWorkflow(file);
  const rel = path.relative(ROOT, file).replace(/\\/g, '/');
  const name = safeId(rel.replace(/\.json$/i, ''));
  const result = new BpmnBuilder(wf, name).build();
  return { file: rel, name, ...result };
}

function main() {
  let opts;
  try { opts = parseArgs(process.argv); }
  catch (e) { console.error(e.message); usage(); process.exit(2); }

  const st = fs.statSync(opts.input);
  const files = st.isDirectory() ? walkFiles(opts.input) : [opts.input];
  const selected = opts.limit ? files.slice(0, opts.limit) : files;
  if (opts.out) fs.mkdirSync(opts.out, { recursive: true });

  const workflows = [];
  const failures = [];
  for (const file of selected) {
    try {
      const encoded = encodeWorkflow(file);
      workflows.push({
        file: encoded.file,
        output: opts.out ? `${encoded.name}.bpmn` : null,
        validation: encoded.validation,
        stats: encoded.stats,
        notes: encoded.notes,
      });
      if (opts.out) fs.writeFileSync(path.join(opts.out, `${encoded.name}.bpmn`), encoded.xml);
      else if (!st.isDirectory()) process.stdout.write(encoded.xml);
      if (!encoded.validation.ok) failures.push({ file: encoded.file, validation: encoded.validation });
    } catch (e) {
      failures.push({ file: path.relative(ROOT, file).replace(/\\/g, '/'), error: e.message });
    }
  }

  const totals = workflows.reduce((acc, w) => {
    acc.states += w.stats.states;
    acc.flows += w.stats.flows;
    acc.nodes += w.validation.nodes;
    acc.choices += w.stats.choices;
    acc.parallel += w.stats.parallel;
    acc.map += w.stats.map;
    acc.retry += w.stats.retry;
    acc.catch += w.stats.catch;
    acc.jsonata += w.stats.jsonata;
    for (const [k, v] of Object.entries(w.stats.lossy)) acc.lossy[k] = (acc.lossy[k] || 0) + v;
    return acc;
  }, { states: 0, flows: 0, nodes: 0, choices: 0, parallel: 0, map: 0, retry: 0, catch: 0, jsonata: 0, lossy: {} });

  const report = {
    generated_by: 'eval/asl2bpmn/encode.js',
    input: path.relative(ROOT, opts.input).replace(/\\/g, '/'),
    selected: selected.length,
    encoded: workflows.length,
    failed: failures.length,
    output_dir: opts.out ? path.relative(ROOT, opts.out).replace(/\\/g, '/') : null,
    totals,
    limitations: [
      'BPMN is used only for fair-class control-flow soundness baselines; JSON document provenance is not encoded.',
      'Choice condition expressions are preserved as sequence-flow labels, not executable guards.',
      'Catch is lowered to an exclusive gateway (normal vs. error branch); Retry policies and service/task semantics are lossy metadata.',
      'Map states are encoded as activity placeholders; item data flow is out of scope.',
      'All terminals are merged into a single end event to obtain a workflow-net shape.',
    ],
    workflows,
    failures,
  };
  if (opts.summary) fs.writeFileSync(opts.summary, JSON.stringify(report, null, 2) + '\n');
  else if (st.isDirectory()) process.stdout.write(JSON.stringify(report, null, 2) + '\n');
  if (failures.length) process.exit(1);
}

if (require.main === module) main();
