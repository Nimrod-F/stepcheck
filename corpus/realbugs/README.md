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

Mine fix commits that touch a Step Functions / ASL definition from public repos
(GitHub/CDK/SAM/serverless-framework). Heuristics for candidate commits: a diff
that edits an `*.asl.json` / state-machine definition and a message containing
`fix`, `typo`, `wrong field`, `missing`, `NoChoiceMatched`, `States.Runtime`,
`Default`, `heartbeat`, `timeout`. For each, save the file *before* the fix as
`<id>-pre.json` and *after* as `<id>-post.json`, then run the harness and
adjudicate the `fixed_codes`.

## Note on the included pair

`demo-missingfield-*.json` is a **synthetic demonstrator** (not a mined bug) that
shows the harness end-to-end: the pre-fix version reads `$.order.total` after a
`Pass` that only produced `$.order.amount` (caught by `SC1101`); the post-fix
version reads the produced field and is clean. Replace/augment with mined pairs
before reporting any real-bug numbers in the paper.
