# Human gold-labelling protocol (idempotency / persistence)

> **STATUS: human re-labelling in progress.** All **23** gold workflows are being
> labelled from scratch by two independent human annotators (A/B). Fill
> `eval/gold-labels-human-A.json` and `-B.json` from the blank template, then run
> `node eval/score-human-gold.js --reconciled` for inter-annotator Cohen's κ, RQ2
> inference accuracy, and in-the-wild warning precision. Latest reconciled run:
> κ = 0.71 (idempotent) / 0.86 (persistent) over 116 double-labelled tasks.

**Why this exists.** RQ2 (idempotency/persistence *inference*) and the in-the-wild
*warning precision* are both measured against a gold standard. A **fully human**,
**double-labelled** gold makes those numbers credible and lets us report a real
inter-annotator agreement (rather than agreement between two LLM passes). The
template `eval/gold-labels-human.template.json` covers **every Task state in all 23
gold workflows** (172 tasks); both annotators label all of them independently, then
disagreements are reconciled. This closes the construct-validity gap (review item
**C1**).

## What to label

Only **Task** states (including tasks nested inside `Map` `Iterator`/`ItemProcessor`
and `Parallel` `Branches`). `Pass`, `Choice`, `Wait`, `Succeed`, `Fail` are not tasks.

For each task set two fields to `true`, `false`, or `null` (abstain):

### `idempotent`
`true` if **repeating** the operation causes **no additional external effect**.
- *Idempotent:* reads (`get`/`list`/`describe`/`query`/`scan`), validations,
  writes keyed by a **stable id** (DynamoDB `PutItem` by primary key, S3
  `PutObject` to a fixed key), and `cancel`/`delete`/`release` operations that
  converge to the same end state.
- *Not idempotent (`false`):* creates a new record each call, charges/pays,
  sends/publishes a message or email, appends, or increments a counter.

### `persistent`
`true` if the task **acquires or commits a durable external resource** that a
later failure would need to compensate/undo.
- *Persistent:* reserve inventory, create order/account/instance, charge a card,
  provision infrastructure, write a durable record that matters.
- *Not persistent (`false`):* reads/validations; **notifications** (SNS/SES/
  `notify`/`publish` are fire-and-forget); and **compensators themselves**
  (`refund`/`cancel`/`release`/`rollback`/cleanup-delete are the *undo* action,
  not a new acquisition).

### Abstain (`null`)
If the semantics cannot be determined with confidence (a generic `Call HTTP API`,
`Process`, `Transform`, opaque `Lambda` name), set the field to `null`. Do **not**
guess — abstention is measured separately (as coverage) and is not penalised as an
error.

## Judge only from these signals

Label each task from its **real-world operational semantics**, read off:
its **state name**, the **`Resource` ARN**, the **`FunctionName`/Action/target**, and
the **surrounding control flow**. Do **not** run any inference tool and do **not**
look at the other annotator's file — that is exactly what the agreement measures.

`eval/label-signals.md` is a generated worksheet listing every task in every gold
workflow with these signals (task, type, action, target) and blank
`idempotent?`/`persistent?` columns; read it alongside the source workflows in
`corpus/asl/`. Regenerate it with `node eval/make-label-signals.js`.

## Procedure

1. Copy the blank template to each annotator's file:
   ```bash
   cp eval/gold-labels-human.template.json eval/gold-labels-human-A.json
   cp eval/gold-labels-human.template.json eval/gold-labels-human-B.json
   ```
   The template is pre-filled with every file and task at `null`; annotators change
   `null` -> `true`/`false` only where confident.
2. Annotator **A** fills `-A.json`, annotator **B** fills `-B.json`, **independently**.
3. Check inter-annotator agreement first: `node eval/score-human-gold.js` (Cohen's κ
   for idempotent and persistent). Aim for κ ≥ 0.7 (substantial); if lower, tighten
   the rubric before trusting the gold.
4. Reconcile the A ≠ B disagreements into `eval/gold-labels-human.json` (agree a
   final value by discussion, or set `null` if genuinely ambiguous).
5. `node eval/score-human-gold.js --reconciled` merges the reconciled labels with the
   hand-labelled core into `corpus/gold-labels.human.json` and reports RQ2 inference
   accuracy/coverage and the SC3001/SC4001/SC4010 warning precision — the numbers used
   in the evaluation section.

## Format

Both `-A.json` and `-B.json` share this shape (exact state names as in the JSON):

```json
{ "<workflow-file>.asl.json": { "<state name>": { "idempotent": true, "persistent": false }, ... }, ... }
```

The 23 source workflows are under `corpus/asl/` (their filenames are the JSON keys in
the template).
