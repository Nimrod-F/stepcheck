// Extract real Amazon States Language (ASL) workflows from the github-explorer
// cache dumps into corpus/asl/*.json and build corpus/manifest.json.
// Reliable, offline: parses the cached gitingest-style JSON (no network/auth).
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const CACHE = path.join(ROOT, '.github-explorer-cache');
const OUT = path.join(ROOT, 'corpus', 'asl');
fs.mkdirSync(OUT, { recursive: true });

// Split a gitingest "content" blob into {path, body} sections.
function splitFiles(content) {
  const out = [];
  // Sections look like:  ====...\nFile: <path>\n====...\n<body>
  const re = /={10,}\s*\nFile:\s*(.+?)\s*\n={10,}\s*\n/g;
  let m, prev = null, prevPath = null;
  while ((m = re.exec(content)) !== null) {
    if (prev !== null) out.push({ path: prevPath, body: content.slice(prev, m.index) });
    prevPath = m[1].trim();
    prev = re.lastIndex;
  }
  if (prev !== null) out.push({ path: prevPath, body: content.slice(prev) });
  return out;
}

function tryParse(s) { try { return JSON.parse(s); } catch { return null; } }

// Is this a parsed ASL state machine?
function isAsl(o) {
  return o && typeof o === 'object' && o.States && typeof o.States === 'object' &&
         (typeof o.StartAt === 'string');
}

// Recursively collect state-type / feature info.
function analyze(asl) {
  const types = {}; let retry = 0, catch_ = 0, total = 0;
  function walk(states) {
    if (!states || typeof states !== 'object') return;
    for (const name of Object.keys(states)) {
      const st = states[name];
      if (!st || typeof st !== 'object') continue;
      total++;
      const t = st.Type || 'Unknown';
      types[t] = (types[t] || 0) + 1;
      if (Array.isArray(st.Retry) && st.Retry.length) retry++;
      if (Array.isArray(st.Catch) && st.Catch.length) catch_++;
      if (st.Type === 'Map') {
        if (st.Iterator && st.Iterator.States) walk(st.Iterator.States);
        if (st.ItemProcessor && st.ItemProcessor.States) walk(st.ItemProcessor.States);
      }
      if (st.Type === 'Parallel' && Array.isArray(st.Branches)) {
        for (const b of st.Branches) if (b && b.States) walk(b.States);
      }
    }
  }
  walk(asl.States);
  return { total, topLevel: Object.keys(asl.States).length, types, retry, catch_ };
}

const manifest = [];
const seen = new Set();
let idCounter = 0;

const REPOS = {
  'aws-samples/aws-stepfunctions-examples': 'sfn-examples',
  'aws-samples/step-functions-workflows-collection': 'sfn-collection',
  'aws-samples/serverless-patterns': 'serverless-patterns',
};

for (const fname of fs.readdirSync(CACHE)) {
  if (!fname.endsWith('.json')) continue;
  const full = path.join(CACHE, fname);
  const sz = fs.statSync(full).size;
  if (sz > 150 * 1024 * 1024) { console.error(`skip ${fname} (${(sz/1e6)|0}MB too big)`); continue; }
  let dump;
  try { dump = JSON.parse(fs.readFileSync(full, 'utf8')); }
  catch (e) { console.error(`parse fail ${fname}: ${e.message}`); continue; }
  const repo = dump.summary && dump.summary.repository;
  const prefix = REPOS[repo];
  if (!prefix) { console.error(`skip repo ${repo}`); continue; }
  const content = dump.content || '';
  if (!content) { console.error(`no content for ${repo}`); continue; }
  const files = splitFiles(content);
  let kept = 0;
  for (const f of files) {
    if (!/\.json$/i.test(f.path)) continue;
    if (/package(-lock)?\.json$|tsconfig|cdk\.json$|\.eslintrc|composer\.json$/i.test(f.path)) continue;
    const obj = tryParse(f.body.trim());
    if (!isAsl(obj)) continue;
    const info = analyze(obj);
    if (info.total < 1) continue;
    const flat = f.path.replace(/[\\/]/g, '__').replace(/[^A-Za-z0-9_.-]/g, '_');
    const outName = `${prefix}__${flat}`.replace(/\.json$/i, '') + '.json';
    if (seen.has(outName)) continue;
    seen.add(outName);
    fs.writeFileSync(path.join(OUT, outName), JSON.stringify(obj, null, 2));
    manifest.push({
      id: `wf${String(++idCounter).padStart(3, '0')}`,
      repo, path: f.path, file: `corpus/asl/${outName}`,
      topLevelStates: info.topLevel, totalStates: info.total,
      types: info.types, retryStates: info.retry, catchStates: info.catch_,
    });
    kept++;
  }
  console.error(`${repo}: kept ${kept} ASL workflows (of ${files.length} files scanned)`);
}

manifest.sort((a, b) => a.totalStates - b.totalStates);
fs.writeFileSync(path.join(ROOT, 'corpus', 'manifest.json'), JSON.stringify(manifest, null, 2));
console.error(`\nTOTAL corpus: ${manifest.length} workflows -> corpus/manifest.json`);
