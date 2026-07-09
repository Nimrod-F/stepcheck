'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..', '..');

function usage() {
  console.error(`Usage:
  node eval/asl2bpmn/run_baselines.js [--summary eval/asl2bpmn-summary.json] [--out eval/bpmn-baseline.json]

Optional tool configuration:
  --bprove-cmd PATH           or BPROVE_CMD=PATH
  --bprove-args TEMPLATE      or BPROVE_ARGS="{file}"
  --bpmn-analyze-cmd PATH     or BPMN_ANALYZE_CMD=PATH
  --bpmn-analyze-args TEMPLATE or BPMN_ANALYZE_ARGS="{file}"

The TEMPLATE is split on whitespace after replacing {file} and {name}. Quote paths by configuring
the command as PATH and keeping {file} as its own argument. This runner records availability and
raw command outcomes; scoring is added only after a concrete verifier is installed.`);
}

function parseArgs(argv) {
  const opts = {
    summary: path.join(ROOT, 'eval', 'asl2bpmn-summary.json'),
    out: path.join(ROOT, 'eval', 'bpmn-baseline.json'),
    timeoutMs: 30000,
    bproveCmd: process.env.BPROVE_CMD || null,
    bproveArgs: process.env.BPROVE_ARGS || '{file}',
    bpmnAnalyzeCmd: process.env.BPMN_ANALYZE_CMD || null,
    bpmnAnalyzeArgs: process.env.BPMN_ANALYZE_ARGS || '{file}',
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--summary') opts.summary = path.resolve(argv[++i]);
    else if (a === '--out') opts.out = path.resolve(argv[++i]);
    else if (a === '--timeout-ms') opts.timeoutMs = Number(argv[++i]);
    else if (a === '--bprove-cmd') opts.bproveCmd = argv[++i];
    else if (a === '--bprove-args') opts.bproveArgs = argv[++i];
    else if (a === '--bpmn-analyze-cmd') opts.bpmnAnalyzeCmd = argv[++i];
    else if (a === '--bpmn-analyze-args') opts.bpmnAnalyzeArgs = argv[++i];
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!Number.isInteger(opts.timeoutMs) || opts.timeoutMs <= 0) throw new Error('--timeout-ms must be a positive integer');
  return opts;
}

function rel(p) {
  return path.relative(ROOT, p).replace(/\\/g, '/');
}

function existsFile(p) {
  try { return !!p && fs.existsSync(p) && fs.statSync(p).isFile(); }
  catch { return false; }
}

function splitTemplate(template, values) {
  const replaced = template.replace(/\{(file|name)\}/g, (_, k) => values[k]);
  const out = [];
  let cur = '';
  let quote = null;
  let escaped = false;
  for (const ch of replaced) {
    if (escaped) { cur += ch; escaped = false; continue; }
    if (ch === '\\') { escaped = true; continue; }
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
      continue;
    }
    if (ch === '"' || ch === "'") { quote = ch; continue; }
    if (/\s/.test(ch)) {
      if (cur) { out.push(cur); cur = ''; }
      continue;
    }
    cur += ch;
  }
  if (cur) out.push(cur);
  return out;
}

function truncate(s, n = 4000) {
  s = String(s || '');
  return s.length > n ? s.slice(0, n) + `\n<truncated ${s.length - n} chars>` : s;
}

function runTool(tool, cmd, argsTemplate, workflows, outputDir, timeoutMs) {
  const configured = !!cmd;
  const available = configured && existsFile(cmd);
  const base = {
    tool,
    configured,
    available,
    command: cmd || null,
    args_template: argsTemplate,
    status: 'not_configured',
    workflows: workflows.length,
    runs: [],
  };
  if (!configured) {
    base.how_to_run = tool === 'bprove'
      ? 'Set BPROVE_CMD and optionally BPROVE_ARGS, then rerun this script.'
      : 'Set BPMN_ANALYZE_CMD and optionally BPMN_ANALYZE_ARGS, then rerun this script.';
    return base;
  }
  if (!available) {
    base.status = 'missing_command';
    base.how_to_run = `Configured command does not exist: ${cmd}`;
    return base;
  }
  base.status = 'ran';
  for (const wf of workflows) {
    const bpmn = path.resolve(ROOT, outputDir || '', wf.output || '');
    if (!existsFile(bpmn)) {
      base.runs.push({ file: wf.file, bpmn: rel(bpmn), status: 'missing_bpmn' });
      continue;
    }
    const args = splitTemplate(argsTemplate, { file: bpmn, name: wf.output || wf.file });
    const started = Date.now();
    try {
      const out = execFileSync(cmd, args, { encoding: 'utf8', timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024 });
      base.runs.push({ file: wf.file, bpmn: rel(bpmn), status: 'ok', ms: Date.now() - started, stdout: truncate(out) });
    } catch (e) {
      base.runs.push({
        file: wf.file,
        bpmn: rel(bpmn),
        status: e.killed || e.signal === 'SIGTERM' ? 'timeout_or_killed' : 'nonzero',
        code: e.status == null ? null : e.status,
        ms: Date.now() - started,
        stdout: truncate(e.stdout),
        stderr: truncate(e.stderr),
      });
    }
  }
  base.ok = base.runs.filter(r => r.status === 'ok').length;
  base.nonzero = base.runs.filter(r => r.status === 'nonzero').length;
  base.missing_bpmn = base.runs.filter(r => r.status === 'missing_bpmn').length;
  return base;
}

function main() {
  let opts;
  try { opts = parseArgs(process.argv); }
  catch (e) { console.error(e.message); usage(); process.exit(2); }

  const summary = JSON.parse(fs.readFileSync(opts.summary, 'utf8'));
  const workflows = summary.workflows || [];
  const results = [
    runTool('bprove', opts.bproveCmd, opts.bproveArgs, workflows, summary.output_dir, opts.timeoutMs),
    runTool('bpmn_analyze', opts.bpmnAnalyzeCmd, opts.bpmnAnalyzeArgs, workflows, summary.output_dir, opts.timeoutMs),
  ];
  const report = {
    generated_by: 'eval/asl2bpmn/run_baselines.js',
    summary: rel(opts.summary),
    bpmn_inputs: workflows.length,
    note: 'This file records external verifier availability and raw runs only. It is not an AP/F1 result table.',
    results,
  };
  fs.writeFileSync(opts.out, JSON.stringify(report, null, 2) + '\n');
  console.log(`${results.map(r => `${r.tool}:${r.status}`).join(' ')} -> ${rel(opts.out)}`);
}

if (require.main === module) main();
