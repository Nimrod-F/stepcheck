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
- **86 / 193** workflows flagged; 1 error, 162 warnings.
- **Sound native findings (high confidence):**
  - `SC0007` × 9  — Choice state with no Default (non-exhaustive).
  - `SC0010` × 1  — malformed state (a top-level `QueryLanguage` directive nested inside `States`).
- **Inference-driven warnings (need triage):**
  - `SC3001` × 36 — broad retry on an inferred non-idempotent task.
  - `SC4001` × 117 — inferred persistent task with no compensation/error handling.

## E3 — Mutation-based detection (delta over baseline; 0 = not detected)
| Defect class | Check | Applicable | Detected | Recall |
|---|---|---|---|---|
| Broken data binding | SC1003 (contract) | 141 | 141 | 100% |
| Unsafe retry        | SC3001 (retry)    | 103 | 103 | 100% |
| Missing compensation| SC4001 (comp.)    | 19  | 19  | 100% |
| Dangling transition | SC0002 (struct.)  | 166 | 166 | 100% |
| **Total**           |                    | **429** | **429** | **100%** |

Each mutant injects exactly one defect; "detected" means the expected diagnostic
count strictly increased vs the unmutated baseline.

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

## E4b — In-the-wild warning precision (vs gold)
Of the 39 SC3001/SC4001 warnings landing on a gold-labelled task, **28/39 (72%)** are
true positives — **26/32 (81%)** for SC4001 (compensation), **2/7** for SC3001 (retry).
Every false positive is a compensator (RefundPayment/Cancel*) or an idempotent
keyed/metadata write. (A broader judge+adversarial-verify triage of all 153 findings is
in `eval/triage-verdicts.json`.)

## E9 — Baseline: statelint (AWS Labs reference linter, v0.8.0)
`node eval/statelint_baseline.js` → `eval/baseline-statelint.json`. On the 429 mutants:
| Defect class | Applicable | StepCheck | statelint |
|---|---|---|---|
| Dangling transition (SC0002) | 166 | 166 (100%) | 166 (100%) |
| Broken binding (SC1003) | 141 | 141 (100%) | 140 (99%) |
| Unsafe retry (SC3001) | 103 | 103 (100%) | **0 (0%)** |
| Missing compensation (SC4001) | 19 | 19 (100%) | **0 (0%)** |

statelint catches schema/structural defects but is blind to retry-safety and
compensation. In the wild it flags 113/193 files with 499 problems, **410** of them
schema-shape nits (BackoffRate/IntervalSeconds typing), none semantic.

## E5 — Verification cost
- Mean **18.7 µs**/workflow, median 14.9 µs, max 111.8 µs.
- **3.6 ms** to verify the entire 193-workflow corpus (~53k workflows/s).
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
- Mutation study where a class has an injection site: **retry 4/4, structural 9/9 (100%)**;
  contract/compensation have no sites in these single-task feature demos.
- **Native typed contract:** the CNCF DSL declares JSON Schemas, so the typed contract
  check (SC1010) runs with **no inference**. `corpus/cncf-typed/order-bad.yaml` (ship
  before charge) → **3 SC1010 errors** derived directly from the declared schemas.
- Verification: mean **3.9 µs**/workflow.

## Implementation size — 2,631 lines of Rust
| Component | Module(s) | LOC |
|---|---|---|
| Workflow IR | ir.rs | 277 |
| Frontends (ASL parse+emit, typed DSL, CNCF Serverless Workflow) | asl.rs, dsl.rs, cncf.rs | 664 |
| Annotations + inference | annot.rs | 251 |
| Analysis passes (structural, contract, typestate, retry, compensation, manager) | passes/* | 650 |
| Diagnostics | diag.rs | 115 |
| Mutation engine | mutate.rs | 184 |
| CLI + stats + eval harness | main.rs | 490 |
| **Total** | | **2631** |
