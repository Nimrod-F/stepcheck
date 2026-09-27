#!/usr/bin/env node
// Re-fetch the corpus files the artifact does not redistribute (their upstream
// repositories publish no licence; see corpus/PROVENANCE.md).
//
// Every file is pinned to an upstream commit and a SHA-256 of its content, so the
// fetched corpus is byte-identical to the one the paper measured:
//   corpus/wild-external/manifest.json   95 definitions, 16 repositories (32 not shipped)
//   corpus/realbugs/unredistributed.json 2 fix-commit pairs (4 files not shipped)
// A file whose upstream content changed or disappeared is reported, never silently
// replaced. Needs network access to raw.githubusercontent.com; no token.
//
//   node eval/fetch_unredistributed.js           # fetch the missing files
//   node eval/fetch_unredistributed.js --check   # only report which are missing
'use strict';
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const ROOT = path.resolve(__dirname, '..');
const CHECK = process.argv.includes('--check');
const sha256 = (s) => crypto.createHash('sha256').update(s).digest('hex');

// Wild-external files were stored as re-serialized JSON (eval/mine_wild.js);
// real-bug pairs as the raw upstream text.
const wild = JSON.parse(fs.readFileSync(path.join(ROOT, 'corpus/wild-external/manifest.json'), 'utf8'))
  .map((m) => ({ ...m, normalize: (t) => JSON.stringify(JSON.parse(t), null, 2) }));
const bugs = JSON.parse(fs.readFileSync(path.join(ROOT, 'corpus/realbugs/unredistributed.json'), 'utf8')).files
  .map((m) => ({ ...m, normalize: (t) => t.replace(/\r\n/g, '\n') }));

(async () => {
  let present = 0, fetched = 0;
  const failed = [];
  for (const m of [...wild, ...bugs]) {
    const dest = path.join(ROOT, m.file);
    if (fs.existsSync(dest)) { present++; continue; }
    if (CHECK) { failed.push(`${m.file}: missing`); continue; }
    const url = `https://raw.githubusercontent.com/${m.repo}/${m.commit}/${m.path}`;
    let text = null;
    try {
      const r = await fetch(url);
      if (r.ok) text = m.normalize(await r.text());
      else failed.push(`${m.file}: HTTP ${r.status} for ${url}`);
    } catch (e) {
      failed.push(`${m.file}: ${e.message}`);
    }
    if (text === null) continue;
    if (sha256(text) !== m.sha256) { failed.push(`${m.file}: upstream content differs from the measured file`); continue; }
    fs.writeFileSync(dest, text);
    fetched++;
  }
  console.log(`present ${present}, fetched ${fetched}, failed ${failed.length} (of ${wild.length + bugs.length})`);
  failed.forEach((f) => console.log('  ' + f));
  process.exit(failed.length ? 1 : 0);
})();
