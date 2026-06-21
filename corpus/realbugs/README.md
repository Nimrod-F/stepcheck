# Real-bug benchmark (pre-fix / post-fix pairs)

`stepcheck eval-pairs <dir>` replays StepCheck on pairs of ASL definitions:

- `<id>-pre.json`  — the **buggy** (pre-fix) version
- `<id>-post.json` — the **fixed** (post-fix) version

A bug is reported **caught** when some diagnostic code fires on the pre-fix
version and is gone (or reduced in count) on the post-fix version. This measures
detection of *independently introduced and fixed* defects, which avoids the
construct-validity bias of the synthetic mutation study (where each mutant is
built to match a check).

## How to populate it with REAL bugs

Use the miner: `node eval/mine_realbugs.js` (clones a built-in list of public
Step Functions repos with `--no-checkout` and extracts pre/post ASL for every
fix-looking commit that edits an ASL file). No token needed for the default mode;
`--discover` (GitHub code search for more repos) needs a read-only public
`GITHUB_TOKEN`. Flags: `--repos owner/a,owner/b`, `--max N`, `--since YYYY-MM-DD`.
Then `cargo run --release -- eval-pairs corpus/realbugs --infer` and **adjudicate**
each candidate (does the fix really match the removed codes?).

**Finding so far (2026-06-21):** mining the curated `aws-samples` repos yields fix
commits that are overwhelmingly *value/config* corrections (update a bucket /
API endpoint / parameter, set `ConsistentRead`, fix an ARN link) --- i.e. outside
StepCheck's targeted defect classes, so it catches ~0 of them. This corroborates
that curated samples are largely free of the deeper defects; a positive real-bug
result needs mining *production* repositories (or targeting commit messages that
name `NoChoiceMatched` / `States.Runtime` / a missing field). The miner + harness
are released so this can be done at scale; we report it as tooling + future work,
not a results table.

## Note on the included pair

`demo-missingfield-*.json` is a **synthetic demonstrator** (not a mined bug) that
shows the harness end-to-end: the pre-fix version reads `$.order.total` after a
`Pass` that only produced `$.order.amount` (caught by `SC1101`); the post-fix
version reads the produced field and is clean. Replace/augment with mined pairs
before reporting any real-bug numbers in the paper.
