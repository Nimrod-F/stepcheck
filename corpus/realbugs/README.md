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

**Result (2026-06-21):** across 58 mined pairs, StepCheck flags a fix-removed defect
on 4, and all 4 are confirmed genuine defects in its targeted classes (see
`realbugs.json` for the adjudication):

| code | repo | bug |
|------|------|-----|
| SC1101 | aws-samples/aws-batch-runtime-monitoring | `$.Execution.Input` (single `$`) reads a doc field that never exists (meant `$$` context) |
| SC1101 | allenheltondev/serverless-ai-fitness | reads `$.profile.Item` / `$.userId`, never produced |
| SC1110 | allenheltondev/serverless-ai-fitness | Choice guards on `$.profile.subscription.level` (never produced) → dead subscriber branch |
| SC6001 | manikanta5827/leave-management | callback with no timeout → can hang forever |

The other 54 pairs are value/config fixes outside StepCheck's remit (it correctly
stays silent on them). Curated pairs kept here; re-run the miner to grow the set.
Curated AWS-sample-only mining yields ~0 (samples are clean) — the catches above
come from production repos surfaced via `--discover`.

## Note on the included pair

`demo-missingfield-*.json` is a **synthetic demonstrator** (not a mined bug) that
shows the harness end-to-end: the pre-fix version reads `$.order.total` after a
`Pass` that only produced `$.order.amount` (caught by `SC1101`); the post-fix
version reads the produced field and is clean. Replace/augment with mined pairs
before reporting any real-bug numbers in the paper.
