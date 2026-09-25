# Gold labels for the name-based inference (idempotency / persistence)

StepCheck's optional inference (`--infer`) guesses two effect facts per `Task` from its
name and resource: whether repeating it is harmless (`idempotent`) and whether it commits
a durable effect that a later failure must undo (`persistent`). Findings that rest on
these guesses are advisory warnings and never block a build. To measure how good the
guesses are, we labelled the tasks of 23 workflows by hand.

## What the gold set is

- **Files:** `corpus/gold-labels.json` (the labels) and
  `eval/gold-labels-human.template.json` (the same tasks with every value blank).
- **Scope:** 23 workflows from `corpus/asl/`, 172 `Task` states (tasks nested in `Map`
  and `Parallel` included).
- **Labelling:** one labelled set, produced by the authors with the rubric below. No
  inter-annotator agreement is claimed.
- **Partitions:** the 10 workflows labelled first informed how the inference rules were
  written (the *design* partition). The rules were then frozen, and the remaining 13
  workflows, listed in `eval/holdout-workflows.json`, were labelled afterwards and never
  used to write or tune a rule (the *hold-out* partition).

## Rubric

For each task, set each field to `true`, `false`, or `null` when it cannot be decided.

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

### `null`
If the semantics cannot be determined with confidence (a generic `Call HTTP API`,
`Process`, `Transform`, opaque `Lambda` name), leave the field `null`. Undecidable
tasks are excluded from the accuracy denominator.

### Signals used
Each task is labelled from its **state name**, the **`Resource` ARN**, the
**`FunctionName`/action/target**, and the **surrounding control flow**, never from the
tool's output. `eval/label-signals.md` lists these signals for every task
(regenerate with `node eval/make-label-signals.js`).

## Scoring

```bash
cd stepcheck && cargo build --release && cd ..
node eval/inference_accuracy.js   # accuracy and coverage -> eval/inference_accuracy.json
node eval/score-human-gold.js     # the same, plus warning precision on gold-labelled tasks
node eval/score-holdout.js        # design vs hold-out partitions -> eval/holdout-inference.json
```

Current results (StepCheck 0.1.5):

| | Accuracy (on predicted) | Coverage |
|---|---|---|
| Idempotency | 76.4% (107/140) | 81.4% |
| Persistence | 85.0% (119/140) | 81.4% |

Warning precision on gold-labelled tasks: SC3001 7/7, SC4001 34/37, SC4010 6/6
(47/50 overall). On the hold-out partition alone: SC3001 3/3, SC4001 27/29, SC4010 4/4.

## Rule freeze

The inference rules are `infer_effect` and its keyword tables in `stepcheck/src/annot.rs`.
They were frozen at commit `dbf3c1a`; later changes to that file only add the resolution
of linked child workflows and do not touch any inference rule. `eval/score-holdout.js`
records this provenance in its output.
