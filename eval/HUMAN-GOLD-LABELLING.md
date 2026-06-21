# Human gold-labelling protocol (idempotency / persistence)

**Why this exists.** The paper's gold set (`corpus/gold-labels.json`, 23 workflows)
is a **mix**: an original **hand-labelled** core (10 workflows) plus a **13-workflow
LLM-assisted expansion** (`eval/gold-expansion-labels.json`, produced by two
independent rubric-guided LLM passes + adjudication; see `eval/wf-eval.js`). The
paper now discloses this split honestly, and the reported Cohen's κ (1.00 / 0.745)
is the agreement between the two LLM passes of the **expansion**.

To upgrade the evaluation to a **fully human** gold set for camera-ready, hand-label
the **13 expanded workflows** below (the template covers exactly those) — ideally two
people independently, then reconcile — and recompute the numbers. The original 10 are
already human-labelled, so only the expansion needs redoing.

This restores the strongest framing of RQ2 (real inter-annotator agreement) and
fully closes the construct-validity gap (review item **C1**).

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
guess — abstention is measured separately and is not penalised as an error.

## Procedure

1. Copy `eval/gold-labels-human.template.json` (pre-filled with every file and
   task, values `null`) to `eval/gold-labels-human-A.json`. Labeller A fills it in
   **without** looking at `gold-expansion-labels.json` or the tool's output.
2. Labeller B independently fills a second copy `eval/gold-labels-human-B.json`.
3. Reconcile disagreements into `eval/gold-labels-human.json`, recording any
   genuinely ambiguous cases.
4. Recompute accuracy / coverage / warning precision and **human** Cohen's κ with
   the existing helpers (`kappa()`, `prec()` in `eval/wf-eval.js`) fed the human
   labels; update Table~\ref{tab:infer} and Section~\ref{sec:eval:infer}.
5. Optionally keep the LLM labels as an auxiliary **"LLM-as-annotator vs human"**
   agreement table — that turns the disclosure into a methodological contribution.

The 23 source workflows are under `corpus/asl/` (filenames are the JSON keys in
the template).
