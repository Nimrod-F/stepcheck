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

## E4 — Inference accuracy vs hand-labeled gold (10 workflows, 44 tasks)
| Property | Labeled | Predicted | Abstained | Correct | Wrong | Accuracy (of predicted) | Coverage |
|---|---|---|---|---|---|---|---|
| Idempotency | 44 | 35 | 9 | 29 | 6 | **82.9%** | 79.5% |
| Persistence | 44 | 35 | 9 | 32 | 3 | **91.4%** | 79.5% |

Heuristic abstains on unknown verbs (confirm/move/audit/copy/aggregate/…); errs on
e.g. `cancel` (idempotent but flagged non-idempotent) and `refund` (a compensator
flagged persistent) — motivating explicit annotation overrides.

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

## Implementation size — 2,359 lines of Rust
| Component | Module(s) | LOC |
|---|---|---|
| Workflow IR | ir.rs | 277 |
| Frontends (ASL parse+emit, typed DSL) | asl.rs, dsl.rs | 412 |
| Annotations + inference | annot.rs | 245 |
| Analysis passes (structural, contract, typestate, retry, compensation, manager) | passes/* | 650 |
| Diagnostics | diag.rs | 115 |
| Mutation engine | mutate.rs | 184 |
| CLI + stats + eval harness | main.rs | 476 |
| **Total** | | **2359** |
