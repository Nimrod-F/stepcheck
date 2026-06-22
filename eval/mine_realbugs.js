// Mine real pre-fix / post-fix ASL pairs from public Step Functions repos for the
// real-bug benchmark (`stepcheck eval-pairs`). For each commit whose message looks
// like a fix and whose diff edits an ASL definition, it extracts the file *before*
// and *after* the commit and writes corpus/realbugs/<id>-pre.json / <id>-post.json,
// plus mined-manifest.json for adjudication.
//
// MODES
//   node eval/mine_realbugs.js                       # clone the built-in repo list, mine locally
//   node eval/mine_realbugs.js --repos owner/a,owner/b
//   node eval/mine_realbugs.js --discover            # ALSO use GitHub code search to find more repos
//   flags: --max N (default 200) | --keep (keep clones) | --since YYYY-MM-DD
//
// TOKEN
//   Cloning public repos needs NO token. Only --discover calls the GitHub API; set
//   GITHUB_TOKEN to a read-only public PAT (classic: no scopes; fine-grained: Public
//   repositories -> Contents: read-only). The token only raises the rate limit.
//
// Output is for ADJUDICATION: each pair is a candidate; confirm the fixed_codes match
// the real bug before reporting numbers. Re-run `stepcheck eval-pairs corpus/realbugs`.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const OUT = path.join(ROOT, 'corpus', 'realbugs');
const WORK = path.join(__dirname, '.realbugs-work');

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const opt = (name, def) => { const i = argv.indexOf(name); return i >= 0 && argv[i + 1] ? argv[i + 1] : def; };
const MAX = parseInt(opt('--max', '200'), 10);
const SINCE = opt('--since', null);

const DEFAULT_REPOS = [
  'aws-samples/aws-stepfunctions-examples',
  'aws-samples/step-functions-workflows-collection',
  'aws-samples/serverless-patterns',
];

// Commit-message signals that a change is a *fix* (vs a feature/refactor).
const FIX_RE = /\b(fix(e[ds])?|bug|typo|wrong|incorrect|missing|broken|invalid|revert|nochoicematched|states\.runtime|resultpath|inputpath|outputpath|heartbeat|timeout|dangling|unreachable|default branch)\b/i;
// ASL definition file paths (state machine JSON / ASL).
const ASL_PATH_RE = /\.(asl\.json|json)$/i;

function sh(args, cwd) {
  return execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'ignore'] });
}
function trySh(args, cwd) { try { return sh(args, cwd); } catch { return null; } }

function isAsl(text) {
  if (!text) return false;
  let v; try { v = JSON.parse(text); } catch { return false; }
  return v && typeof v === 'object' && typeof v.StartAt === 'string' && v.States && typeof v.States === 'object';
}

function slug(repo) { return repo.replace(/[^a-z0-9]+/gi, '-'); }

async function discoverRepos() {
  const token = process.env.GITHUB_TOKEN;
  if (!token) { console.error('--discover needs GITHUB_TOKEN (read-only public PAT); skipping discovery.'); return []; }
  const repos = new Set();
  const headers = { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'User-Agent': 'stepcheck-miner' };
  // Code search for ASL definitions; collect their repos.
  for (const q of ['"StartAt" "States" extension:asl.json', '"StartAt" "States" "Type": "Task" language:JSON']) {
    for (let page = 1; page <= 3; page++) {
      const url = `https://api.github.com/search/code?q=${encodeURIComponent(q)}&per_page=50&page=${page}`;
      let r; try { r = await fetch(url, { headers }); } catch (e) { console.error('search failed:', e.message); break; }
      if (!r.ok) { console.error('search HTTP', r.status, await r.text().catch(() => '')); break; }
      const j = await r.json();
      for (const item of j.items || []) if (item.repository?.full_name) repos.add(item.repository.full_name);
      if (!j.items || j.items.length < 50) break;
      await new Promise((res) => setTimeout(res, 2500)); // be gentle on the search rate limit
    }
  }
  return [...repos];
}

function mineRepo(repo, pairs, manifest, seen) {
  const dir = path.join(WORK, slug(repo));
  // Reuse a *valid* existing clone; otherwise remove any stale/partial dir and clone.
  if (!fs.existsSync(path.join(dir, '.git'))) {
    if (fs.existsSync(dir)) { try { fs.rmSync(dir, { recursive: true, force: true }); } catch {} }
    console.error(`cloning ${repo} ...`);
    try {
      // --no-checkout: we read history + blobs via `git log`/`git show`, never the
      // working tree, so we skip the checkout step (which fails on repos containing
      // filenames that are illegal on Windows).
      execFileSync('git', ['clone', '--no-checkout', '--quiet', `https://github.com/${repo}.git`, dir],
        { cwd: WORK, stdio: ['ignore', 'ignore', 'pipe'] });
    } catch (e) {
      console.error(`  clone failed: ${repo} -- ${String(e.stderr || e.message).split('\n').filter(Boolean).pop()}`);
      return;
    }
  }
  const since = SINCE ? ['--since', SINCE] : [];
  const log = trySh(['log', '--no-merges', '--pretty=format:%H%x1f%s', ...since], dir);
  if (!log) return;
  for (const line of log.split('\n')) {
    if (pairs.length >= MAX) return;
    const [hash, subject] = line.split('\x1f');
    if (!hash || !FIX_RE.test(subject || '')) continue;
    const changed = trySh(['show', '--name-only', '--pretty=format:', hash], dir);
    if (!changed) continue;
    for (const file of changed.split('\n').map((s) => s.trim()).filter(Boolean)) {
      if (pairs.length >= MAX) return;
      if (!ASL_PATH_RE.test(file)) continue;
      const post = trySh(['show', `${hash}:${file}`], dir);
      const pre = trySh(['show', `${hash}^:${file}`], dir);
      if (!isAsl(post) || !isAsl(pre) || post === pre) continue; // need a real edit to an existing ASL file
      const id = `${slug(repo)}-${hash.slice(0, 7)}-${path.basename(file).replace(/\.(asl\.)?json$/i, '')}`.slice(0, 80);
      if (seen.has(id)) continue;
      seen.add(id);
      fs.writeFileSync(path.join(OUT, `${id}-pre.json`), pre);
      fs.writeFileSync(path.join(OUT, `${id}-post.json`), post);
      manifest.push({ id, repo, commit: hash, file, subject });
      pairs.push(id);
      console.error(`  pair: ${id}  (${subject.slice(0, 60)})`);
    }
  }
}

(async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  fs.mkdirSync(WORK, { recursive: true });
  let repos = opt('--repos', null) ? opt('--repos', '').split(',').map((s) => s.trim()).filter(Boolean) : [...DEFAULT_REPOS];
  if (flag('--discover')) repos = [...new Set([...repos, ...(await discoverRepos())])];
  console.error(`mining ${repos.length} repo(s), cap ${MAX} pairs`);

  const pairs = [], manifest = [], seen = new Set();
  for (const repo of repos) {
    if (pairs.length >= MAX) break;
    try { mineRepo(repo, pairs, manifest, seen); } catch (e) { console.error(`  error on ${repo}: ${e.message}`); }
  }
  fs.writeFileSync(path.join(OUT, 'mined-manifest.json'), JSON.stringify(manifest, null, 2));
  if (!flag('--keep')) { try { fs.rmSync(WORK, { recursive: true, force: true }); } catch {} }

  console.error(`\nWrote ${pairs.length} candidate pair(s) to corpus/realbugs/ + mined-manifest.json`);
  console.error('Next: adjudicate each (does the fix really match the removed codes?), then run:');
  console.error('  cargo run --release -- eval-pairs corpus/realbugs --infer');
})();
