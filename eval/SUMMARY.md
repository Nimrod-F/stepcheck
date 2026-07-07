# StepCheck — Evaluation Results (real numbers, reproducible)

All numbers produced by the committed tool over the committed corpus. Reproduce:
`stepcheck stats corpus/asl`, `stepcheck eval corpus/asl --infer`,
`node eval/inference_accuracy.js`.

## E1 — Corpus characterization
- **193** real workflows mined from 2 AWS repos
  (`aws-samples/aws-stepfunctions-examples`, `aws-samples/step-functions-workflows-collection`).
- **1355** states total; per workflow min 1 / median 6 / mean 7.0 / max 33.
- State types: Task 747, Choice 166, Pass 159, Wait 71, Fail 65, Map 64, Succeed 49, Parallel 33.
- 84 workflows use retry policies; 51 use catch handlers.

## E2 — Findings in the wild (unmodified corpus, with inference)
- **95 / 193** workflows flagged.
- **Sound native findings (high confidence):**
  - `SC0007` × 9  — Choice state with no Default (non-exhaustive).
  - `SC0010` × 1  — malformed state (a top-level `QueryLanguage` directive nested inside `States`).
  - `SC1101` × 0  — data-flow provenance: **zero false positives** (soundness; see E10).
- **New native analyses:**
  - `SC6001` × 33 — unbounded `waitForTaskToken`/Activity callback (no Timeout/Heartbeat).
  - `SC5001`/`SC5002` × 0 — concurrency interference fires **0** in the wild: the
    conservative overwriting-write rule (only PutItem/PutObject, never UpdateItem-merge or
    item-keyed writes) finds no genuine last-writer-wins race among these correct samples
    (detection power shown by the mutation study, E3/E9).
- **Inference-driven warnings (need triage):**
  - `SC3001` × 36 — broad retry on an inferred non-idempotent task.
  - `SC4001` × 117 — inferred persistent task with no compensation/error handling.

## E3 — Mutation-based detection (delta over baseline; 0 = not detected)
| Defect class | Check | Applicable | Detected | Recall |
|---|---|---|---|---|
| Broken data binding | SC1003 (data-flow) | 141 | 141 | 100% |
| Unsafe retry        | SC3001 (retry)    | 103 | 103 | 100% |
| Missing compensation| SC4001 (comp.)    | 19  | 19  | 100% |
| Dangling transition | SC0002 (struct.)  | 166 | 166 | 100% |
| Concurrency interference | SC5001 (concurrency) | 21 | 21 | 100% |
| Temporal (heartbeat≥timeout) | SC6003 (temporal) | 166 | 166 | 100% |
| **Total**           |                    | **616** | **616** | **100%** |

Each mutant injects exactly one defect; "detected" means the expected diagnostic
count strictly increased vs the unmutated baseline. The data-flow class (SC1101) is
studied separately (E10) because its detection depends on a known document shape.

## E4 — Inference accuracy vs agreement-checked gold (23 workflows, 147 tasks)
Gold set expanded from 10→23 workflows; new workflows labelled in **two independent
passes** (Cohen's kappa **1.00** idempotency, **0.745** persistence) reconciled by
adjudication. `node eval/inference_accuracy.js` (gold in `corpus/gold-labels.json`).
| Property | Labeled | Predicted | Abstained | Correct | Accuracy (of predicted) | Coverage |
|---|---|---|---|---|---|---|
| Idempotency | 147 | 118 | 29 | 73  | **61.9%** | 80.3% |
| Persistence | 147 | 118 | 29 | 101 | **85.6%** | 80.3% |

The larger, more diverse set reveals idempotency inference is genuinely weak (62%): the
write-verb heuristic misreads stable-key DynamoDB writes and cancel/release/refund
compensators (idempotent in effect) as non-idempotent. Persistence (which drives the
compensation check) holds up at 86%. High annotator agreement + low heuristic accuracy =
the truth is clear but names don't carry it → findings are warnings.

## E4b — In-the-wild warning precision (vs human gold)
`node eval/score-human-gold.js --reconciled` (human gold = reconciled `gold-labels-human-{A,B}.json`
+ the 10-workflow hand-labelled core). Of the SC3001/SC4001 warnings landing on a gold-labelled
task, **40/44 (91%)** are true positives — **33/37 (89%)** for SC4001 (compensation), **7/7 (100%)**
for SC3001 (retry), and **6/6 (100%)** for SC4010; all three inferred codes together are
**46/50 (92%)**. The residual SC4001 false positives are compensators (RefundPayment/Cancel*) or
idempotent keyed/metadata writes. (Human inter-annotator Cohen's kappa **0.89** idempotent /
**0.99** persistent.)

## E9 — Baseline: statelint (AWS Labs reference linter, v0.8.0)
`node eval/statelint_baseline.js` → `eval/baseline-statelint.json`. On the 616 mutants:
| Defect class | Applicable | StepCheck | statelint |
|---|---|---|---|
| Dangling transition (SC0002) | 166 | 166 (100%) | 166 (100%) |
| Broken binding (SC1003) | 141 | 141 (100%) | 140 (99%) |
| Unsafe retry (SC3001) | 103 | 103 (100%) | **0 (0%)** |
| Missing compensation (SC4001) | 19 | 19 (100%) | **0 (0%)** |
| Concurrency interference (SC5001) | 21 | 21 (100%) | **0 (0%)** |
| Temporal heartbeat≥timeout (SC6003) | 166 | 166 (100%) | **0 (0%)** |
| **Total** | **616** | **616 (100%)** | **306 (50%)** |

statelint catches schema/structural defects but is blind to retry-safety, compensation,
concurrency interference, and temporal-budget faults (all schema-valid). In the wild it
flags 113/193 files with 499 problems, **410** of them schema-shape nits, none semantic.

## E10 — Data-flow provenance analysis (SC1101), `corpus/dataflow/`
- **Soundness:** SC1101 fires on **0 / 193** unmodified workflows — zero false positives,
  corroborating the no-false-positive guarantee of the abstract interpretation.
- **Power (demonstrators):** `corpus/dataflow/native-bad.asl.json` → SC1101 with **no
  annotations** (a `Pass` builds `{order:{orderId,amount}}`, a task reads `$.order.total`);
  `typed-bad.asl.json` (+`typed.sidecar.toml`) → **2 SC1101**: a schema miss (`$.total`)
  and a flow-sensitive miss across a `ResultPath` merge (`$.reservation.reservationCode`).
  Good variants are silent. `statelint`/`asl-validator` accept all (paths are valid).
- **Reach (typed tier, `eval ... --strict-input`):** seeding each workflow's start
  document as a closed record of its referenced fields, injected missing-field defects are
  detected **84 / 126 (67%)**; the undetected third sit downstream of opaque task results
  (shape soundly = ⊤) — a measure of ASL's intrinsic opacity, not a reliability gap. The
  closed-world baseline produces **0** spurious SC1101.
- **Result-shape ablation:** the implementation now resolves quoted-bracket member paths;
  `--result-shapes` additionally adds closed declared task-output schemas and known AWS
  service-result envelopes. SC1101 recall improves from **84 / 126** to **88 / 126 (70%)**;
  the ablation oracle confirms **92 / 92** SC1101 reports with **0** counterexamples.

## E5 — Verification cost
- Mean **38.9 µs**/workflow (eight passes incl. the data-flow fixpoint), median 23.7 µs,
  max 1.16 ms; **7.5 ms** to verify the entire 193-workflow corpus.
- Runtime overhead on AWS: **0** (verification is entirely ahead-of-deployment).

## E6 — DSL conciseness
- The order-processing workflow: **20** declarative DSL statements →
  **121-line / 2830-char** ASL (10 states). The DSL additionally encodes 5 record
  schemas, a 4-edge protocol, and per-task idempotency/persistence/compensation —
  semantics ASL cannot express.

## E7 — AWS round-trip (infra/)
- The DSL-verified `order.asl.json` was deployed **unmodified** (only `FunctionName`
  bound to a Lambda ARN) to real AWS Step Functions (`eu-central-1`, EXPRESS).
- Synchronous execution reached **`SUCCEEDED`**; the input
  `{customerId, items, orderId, amount}` propagated through all 4 Lambda steps to
  `OrderCompleted`. Billed **500 ms / 64 MB** (≈ $0, free tier).
- Reused two pre-existing IAM roles; both created resources (state machine + Lambda)
  torn down afterwards. Evidence: `infra/execution-evidence.json`.
- Runtime overhead introduced by StepCheck: **none** (verification is ahead-of-time).
- Note: the round-trip surfaced a real emitter bug (AWS rejects `End` on
  `Succeed`/`Fail`), which was fixed — demonstrating the value of round-trip validation.

## E8 — Cross-format generalization (CNCF Serverless Workflow), `corpus/cncf/`
- A third frontend for the **CNCF Serverless Workflow DSL** was added with **no change to
  the IR or any analysis pass** (`stepcheck eval corpus/cncf --infer`).
- Parses **66/66** real spec examples; the structural/typestate/retry/compensation
  analyses apply unchanged; the ASL-specific data-binding checks (SC1001–SC1003) correctly
  stay silent (CNCF uses jq expressions, not JSONPath).
- Mutation study where a class has an injection site: **retry 4/4, structural 9/9,
  concurrency 1/1, temporal 50/50 (100%)**; contract/compensation have no sites in these
  single-task feature demos. SC1101 stays silent (CNCF uses jq, not `.$` JSONPath).
- **Native typed contract:** the CNCF DSL declares JSON Schemas, so the typed contract
  check (SC1010) runs with **no inference**. `corpus/cncf-typed/order-bad.yaml` (ship
  before charge) → **3 SC1010 errors** derived directly from the declared schemas.
- Verification: mean **3.9 µs**/workflow.

## Implementation size — 3,692 lines of Rust
| Component | Module(s) | LOC |
|---|---|---|
| Workflow IR | ir.rs | 298 |
| Frontends (ASL parse+emit, typed DSL, CNCF Serverless Workflow) | asl.rs, dsl.rs, cncf.rs | 729 |
| Annotations + inference | annot.rs | 258 |
| Analysis passes (8: structural, dataflow, contract, typestate, retry, compensation, concurrency, temporal + manager) | passes/* | 1426 |
| Diagnostics | diag.rs | 115 |
| Mutation engine | mutate.rs | 309 |
| CLI + stats + eval harness | main.rs | 557 |
| **Total** | | **3692** |
