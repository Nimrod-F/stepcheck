// Mine ASL (Amazon States Language) workflows from INDEPENDENT public GitHub repos
// (NOT the three aws-samples collections that make up the main 193-workflow corpus),
// then run stepcheck over them. Purpose: external-validity evidence that the analysis
// generalises beyond AWS's own curated samples.
//
// Discovery is unauthenticated: repo-search + git-trees API (both work without a token),
// raw file download via raw.githubusercontent.com (not rate-limited). The default branch
// is auto-resolved (many repos use `master`, not `main`).
//
//   node eval/mine_wild.js            # seeds + repo-search -> corpus/wild-external/
//   node eval/mine_wild.js --seeds    # only the curated seed repo list (no search)
//
// Output: corpus/wild-external/*.json  +  corpus/wild-external/manifest.json

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const OUT = path.join(ROOT, 'corpus', 'wild-external');
fs.mkdirSync(OUT, { recursive: true });

const argv = process.argv.slice(2);
const flag = (n) => argv.includes(n);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Repos to EXCLUDE (already in the main corpus, or AWS-official).
const EXCLUDE_OWNER = new Set(['aws-samples', 'aws', 'awslabs', 'aws-solutions']);

// Curated seed repos: independent projects that commit raw ASL / state-machine JSON.
const SEED_REPOS = [
  'vdaron/StatesLanguage',
  'skyflow-workflow/skyflow_backend',
  'yskszk63/sam-local-asl',
  'nationalarchives/tdr-service-unavailable',
  'wmfs/tymly',
  'wmfs/statebox',
  'Cimpress-MCP/StateMachineTest',
  'coinbase/step',
  'nib-health-funds/asl-validator',
  'moia-oss/asl-validator',
  'ChristopheBougere/asl-validator',
  'localstack/localstack',
];

const H = { Accept: 'application/vnd.github+json', 'User-Agent': 'stepcheck-wild-miner' };

async function api(url) {
  let r;
  try { r = await fetch(url, { headers: H }); } catch (e) { console.error('  net err', e.message); return null; }
  if (r.status === 403 || r.status === 429) { console.error('  rate-limited'); return null; }
  if (!r.ok) { console.error('  HTTP', r.status, url.slice(28, 90)); return null; }
  return r.json();
}

async function raw(owner, repo, branch, p) {
  const url = 'https://raw.githubusercontent.com/' + owner + '/' + repo + '/' + branch + '/' + p;
  try { const r = await fetch(url, { headers: { 'User-Agent': 'stepcheck-wild-miner' } }); return r.ok ? r.text() : null; }
  catch { return null; }
}

function isAsl(text) {
  let v; try { v = JSON.parse(text); } catch { return false; }
  return v && typeof v === 'object' && typeof v.StartAt === 'string' && v.States && typeof v.States === 'object';
}

const ASL_EXT = /\.asl\.json$/i;
const SM_JSON = /(state[-_ ]?machine|statemachine|workflow|sfn|step[-_ ]?function)/i;
const SKIP = /package(-lock)?\.json$|tsconfig|cdk\.json$|\.eslintrc|composer\.json$|node_modules\//i;

async function searchRepos(query) {
  const url = 'https://api.github.com/search/repositories?q=' + encodeURIComponent(query) + '&sort=stars&per_page=30';
  const j = await api(url);
  if (!j || !j.items) return [];
  return j.items.map((it) => it.full_name + ' ' + (it.default_branch || 'main'));
}

async function mineRepo(full, branch, manifest, seen) {
  const parts = full.split('/');
  const owner = parts[0], repo = parts[1];
  if (EXCLUDE_OWNER.has(owner)) return 0;
  if (branch === '?') {
    const meta = await api('https://api.github.com/repos/' + owner + '/' + repo);
    if (!meta || !meta.default_branch) return 0;
    branch = meta.default_branch;
    await sleep(500);
  }
  let tree = await api('https://api.github.com/repos/' + owner + '/' + repo + '/git/trees/' + branch + '?recursive=1');
  if (!tree) {
    const alt = branch === 'main' ? 'master' : 'main';
    tree = await api('https://api.github.com/repos/' + owner + '/' + repo + '/git/trees/' + alt + '?recursive=1');
    if (tree) branch = alt;
  }
  if (!tree || !tree.tree) return 0;
  const cands = tree.tree
    .filter((n) => n.type === 'blob' && /\.json$/i.test(n.path) && !SKIP.test(n.path))
    .filter((n) => ASL_EXT.test(n.path) || SM_JSON.test(n.path))
    .slice(0, 40);
  let kept = 0;
  for (const n of cands) {
    const body = await raw(owner, repo, branch, n.path);
    if (!body || !isAsl(body)) continue;
    const obj = JSON.parse(body);
    const flat = (owner + '__' + repo + '__' + n.path).replace(/[\\/]/g, '__').replace(/[^A-Za-z0-9_.-]/g, '_').replace(/\.json$/i, '') + '.json';
    if (seen.has(flat)) continue;
    seen.add(flat);
    fs.writeFileSync(path.join(OUT, flat), JSON.stringify(obj, null, 2));
    manifest.push({ repo: full, branch, path: n.path, file: 'corpus/wild-external/' + flat, states: Object.keys(obj.States).length });
    kept++;
    if (kept >= 15) break;
  }
  if (kept) console.error('  ' + full + ': kept ' + kept + ' ASL workflow(s)');
  return kept;
}

(async function main() {
  let repos = SEED_REPOS.map((r) => r + ' ?');
  if (!flag('--seeds')) {
    console.error('searching for independent Step Functions repos');
    for (const q of ['amazon states language stepfunctions', 'aws step functions example workflow', 'stepfunctions saga orchestration']) {
      const hits = await searchRepos(q);
      repos = repos.concat(hits);
      console.error('  "' + q + '" -> ' + hits.length + ' repos');
      await sleep(2500);
    }
  }
  const uniq = new Map();
  for (const r of repos) { const sp = r.split(' '); if (!uniq.has(sp[0])) uniq.set(sp[0], sp[1]); }
  console.error('\nmining ' + uniq.size + ' candidate repo(s)');

  const manifest = [], seen = new Set();
  let total = 0;
  for (const [full, br] of uniq) {
    if (EXCLUDE_OWNER.has(full.split('/')[0])) continue;
    try { total += await mineRepo(full, br, manifest, seen); }
    catch (e) { console.error('  err ' + full + ': ' + e.message); }
    await sleep(800);
    if (total >= 120) break;
  }
  fs.writeFileSync(path.join(OUT, 'manifest.json'), JSON.stringify(manifest, null, 2));
  const repoSet = new Set(manifest.map((m) => m.repo));
  console.error('\nWrote ' + manifest.length + ' ASL workflow(s) from ' + repoSet.size + ' independent repo(s) -> corpus/wild-external/');
})();
