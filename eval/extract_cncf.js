// Extract CNCF Serverless Workflow example definitions from the cached
// serverlessworkflow/specification dump into corpus/cncf/*.yaml.
const fs = require('fs');
const path = require('path');
const ROOT = path.resolve(__dirname, '..');
const CACHE = path.join(ROOT, '.github-explorer-cache');
const OUT = path.join(ROOT, 'corpus', 'cncf');
fs.mkdirSync(OUT, { recursive: true });

function splitFiles(content) {
  const out = [];
  const re = /={10,}\s*\nFile:\s*(.+?)\s*\n={10,}\s*\n/g;
  let m, prev = null, prevPath = null;
  while ((m = re.exec(content)) !== null) {
    if (prev !== null) out.push({ path: prevPath, body: content.slice(prev, m.index) });
    prevPath = m[1].trim(); prev = re.lastIndex;
  }
  if (prev !== null) out.push({ path: prevPath, body: content.slice(prev) });
  return out;
}

let kept = 0;
for (const fname of fs.readdirSync(CACHE)) {
  if (!fname.endsWith('.json')) continue;
  let dump;
  try { dump = JSON.parse(fs.readFileSync(path.join(CACHE, fname), 'utf8')); } catch { continue; }
  const repo = dump.summary && dump.summary.repository;
  if (repo !== 'serverlessworkflow/specification') continue;
  const files = splitFiles(dump.content || '');
  for (const f of files) {
    const p = f.path.replace(/\\/g, '/');
    if (!/(^|\/)examples\/[^/]*\.ya?ml$/i.test(p)) continue;
    if (/\/invalid\//i.test(p)) continue;
    const body = f.body.trim();
    // keep only complete 1.0 DSL workflow definitions
    if (!/\bdocument\s*:/.test(body) || !/\n\s*do\s*:/.test(body)) continue;
    const flat = p.replace(/^.*examples\//, '').replace(/[\\/ ]/g, '__').replace(/[^A-Za-z0-9_.-]/g, '_');
    fs.writeFileSync(path.join(OUT, flat), body + '\n');
    kept++;
  }
  console.error(`${repo}: extracted ${kept} CNCF example workflows`);
}
console.error(`\nTOTAL CNCF corpus: ${kept} -> corpus/cncf/`);
