# StepCheck — Evaluation Results (real numbers, reproducible)

All numbers produced by the committed tool over the committed corpus. Reproduce:
`stepcheck stats corpus/asl`, `stepcheck eval corpus/asl --infer`,
`node eval/inference_accuracy.js`.

## E1 — Corpus characterization
- Foreground deployment-shaped evidence: **6** industrial-topology `aws-samples` workflows
  (including AWS Serverless Airline Booking `ProcessBooking`), **55** `serverless-patterns`
  workflows read from SAM/CloudFormation definitions, and **24** AWS Solutions artifacts /
  **26** machines, including CDK-synthesized enterprise orchestrators.
- Breadth/generalization corpora: **193** workflows mined exhaustively from 2 AWS sample repos
  (`aws-samples/aws-stepfunctions-examples`, `aws-samples/step-functions-workflows-collection`),
  **66** CNCF examples, and **95** independent external ASL definitions.
- **1355** recursive states total by `stepcheck stats`; per workflow min 1 / median 6 / mean 7.0 / max 33.
- State types: Task 747, Choice 166, Pass 159, Wait 71, Fail 65, Map 64, Succeed 49, Parallel 33.
- 84 workflows use retry policies; 51 use catch handlers.

## E2 — Findings in the wild (unmodified corpus, with inference)
- **99 / 193** workflows flagged.
- **Sound native findings (high confidence):**
  - `SC0007` × 9  — Choice state with no Default (non-exhaustive).
  - `SC0010` × 1  — malformed state (a top-level `QueryLanguage` directive nested inside `States`).
    - `SC1101` × 0  — data-flow provenance: no report on the unmodified corpus (see E10).
- **New native analyses:**
  - `SC6001` × 33 — unbounded `waitForTaskToken`/Activity callback (no Timeout/Heartbeat).
  - `SC5001`/`SC5002` × 0 and `SC5003` × 2 — the conservative overwriting-write rule
    (only PutItem/PutObject, never UpdateItem-merge or item-keyed writes) finds no
    last-writer-wins race among these samples, while cross-child composition flags two
    parallel starts of the same child workflow (detection power shown by the mutation study, E3/E9).
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

## E3h — Hard-mutant study (operator–check independence)
`stepcheck eval corpus/asl --infer --result-shapes --hard` → `eval/results-hard-mutants.json`.
Same fault classes, but each variant moves the defect to the edge of what the analysis can
prove (a genuine, deployable defect): five variants sit just past a ⊤/inference boundary, two are
generalization controls. Recall is family-credited (a sibling code counts) with
count-based freshness; the comparison tools are re-run on the same mutants
(`python eval/asl2bpmn/compare.py --hard`, `STEPCHECK_AWS=1 node eval/validator_panel.js --hard`).
Exact CIs and the merged table live in `eval/mutation-stats.json` under `hard_mutants`.

| Class | Check | Easy | Hard (family, CP95) | Boundary probed |
|---|---|---|---|---|
| Structural | SC0002 | 1.00 | 1.00 [.92,1] | exact reachability (recurses into nested scopes) |
| Retry | SC3001 | 1.00 | 0.68 [.59,.77] | inference tier (opaque name/FunctionName) |
| Compensation | SC4001 | 1.00 | 0.79 [.66,.88]* | sibling SC4010 (logging-only Catch); SC4001 = 0/56 |
| Data-flow | SC1101 | 0.70 | 0.00 [0,.03] | ⊤-lift (filter-expression InputPath) |
| Concurrency | SC5001 | 1.00 | 0.00 [0,.16] | ⊤-lift (dynamically-named resource) |
| Temporal | SC6003 | 1.00 | 0.00 [0,.02] | ⊤-lift (reference-path heartbeat/timeout) |
| Contract | SC1003 | 1.00 | 1.00 [.98,1] | generalization control: the break moves into a `ResultSelector`, which the scan reads like any other payload template |

Aggregate: **328/688 = 0.48** family-credited. Before 0.1.5 the SC1003 scan did not read
`ResultSelector`/`ItemSelector`, so the contract row was 0/166 and the aggregate 162/688 = 0.24;
the mutants are unchanged.
A hard value <1 is a **measured completeness boundary, not a soundness violation**: at ⊤ the sound
analyses decline to report rather than emit a false alarm. The structural and contract rows are
generalization controls rather than ⊤ boundaries: the dangling edge hides in a nested sub-machine and
the broken `.$` moves from `Parameters` into a `ResultSelector` or a Distributed Map's `ItemSelector`.
Both stay exact, because the structural pass recurses into sub-machines and SC1003 reads every payload
template (`contract_scan_covers_every_payload_template`). Woflan catches only structural (.31) and a
compensation side effect (.13); schema validators catch structural and the contract break.
*Compensation is caught by the effect-aware sibling SC4010, not the operator's expected SC4001.

## E4 — Inference accuracy vs the hand-labelled gold set (23 workflows, 172 tasks)
One labelled set, produced by the authors with the rubric in `eval/GOLD-LABELLING.md`; no
inter-annotator agreement is claimed. `node eval/inference_accuracy.js` (gold in
`corpus/gold-labels.json`).
| Property | Labeled | Predicted | Abstained | Correct | Accuracy (of predicted) | Coverage |
|---|---|---|---|---|---|---|
| Idempotency | 172 | 140 | 32 | 107 | **76.4%** | 81.4% |
| Persistence | 172 | 140 | 32 | 119 | **85.0%** | 81.4% |

Idempotency inference is the weaker heuristic:
the write-verb heuristic misreads stable-key DynamoDB writes and cancel/release/refund
compensators (idempotent in effect) as non-idempotent. Persistence (which drives the
compensation check) holds up at 85%. The findings remain warnings because names alone do
not carry enough evidence for blocking verification.

## E4b — In-the-wild warning precision (vs the gold set)
`node eval/score-human-gold.js`. Of the SC3001/SC4001 warnings landing on a gold-labelled task,
**41/44 (93%)** are true positives — **34/37 (92%)** for SC4001 (compensation) and **7/7 (100%)**
for SC3001 (retry); with **6/6** for SC4010, all three inferred codes together are **47/50 (94%)**.
The residual SC4001 false positives are compensators (RefundPayment/Cancel*) or idempotent
keyed/metadata writes. Hold-out partition only (`node eval/score-holdout.js`): SC3001 3/3,
SC4001 27/29, SC4010 4/4.

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
- **Clean corpus:** SC1101 fires on **0 / 193** unmodified workflows, so it reports no false
  positive there; the soundness evidence is the certificate and oracle checks below.
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
  `stepcheck dataflow-cert corpus/asl --infer --result-shapes` certifies **88 / 88** emitted
  SC1101 reports by checking **1,146** finite widened-fixpoint postcondition obligations across
  **207** machines, with **0** failed obligations and **0** diagnostic mismatches. The bounded
  concrete oracle remains a separate empirical check and finds **0** present-field counterexamples.

## E11 — Formal-verifier baseline: ASL to BPMN encoder
`node eval/asl2bpmn/encode.js corpus/asl --limit 30 --out eval/asl2bpmn/out --summary eval/asl2bpmn-summary.json`
creates the BPMN input layer for Woflan/BPMN Analyzer/BProVe comparison. The 30-workflow feasibility
slice encodes **30 / 30** workflows with **0** validation failures; the generated BPMN XML files
parse successfully and cover **184** ASL states, **239** sequence flows, **21** Choice gateways,
**6** Parallel split/join regions, and **6** Map placeholders. The summary explicitly counts
lossy features: Choice guards are labels rather than executable data predicates, Retry policies
and service semantics are not encoded, and Map item data flow stays out of scope. This is the
fair-class control-flow baseline substrate; SC1101 JSON-document provenance remains StepCheck-only.
`python eval/asl2bpmn/compare.py --limit 193 --out eval/asl2bpmn-comparison-full.json` now records
the scored formal-baseline comparison. With all three academic baselines configured, clean
over-report is Woflan **16 / 193 (8.3%)**, BPMN Analyzer **16 / 193 (8.3%)**, and BProVe
**56 / 193 (29%)**. On the control-flow class SC0002, recall is StepCheck **166 / 166**, Woflan
**158 / 166**, BPMN Analyzer **144 / 166**, and BProVe **114 / 166**; the non-control-flow ASL
classes remain StepCheck-only except for structural side effects.
`eval/mutation-stats.json` consolidates the additional AP/F1 aggregates on the **full 193-workflow**
corpus, so there is no parallel 30-workflow metric table beyond feasibility. Over all seven SC
classes, StepCheck macro operating-point AP/F1 is **1.00 / 0.975** (bootstrap F1 CI
**[0.966, 0.982]**); Woflan is **0.286 / 0.216**, BPMN Analyzer **0.286 / 0.210**, and BProVe
**0.429 / 0.156**. On the strict BPMN soundness subset (`SC0002`), AP is **1.00** for all four tools
and F1 is StepCheck **1.00**, Woflan **0.975**, BPMN Analyzer **0.929**, BProVe **0.814**. BProVe has
no ASL data-flow model, so SC1101 is StepCheck versus out-of-scope, not a BProVe failure.

## E5 — Verification cost
- Mean **61 µs**/workflow (eight passes incl. the data-flow fixpoint), median 35 µs,
  max 1.8 ms; **11.9 ms** to verify the entire 193-workflow corpus (`eval/results.json`,
  `timing_us`; single-run wall-clock figures on the laptop, so they vary between runs).
- Real industrial set (`corpus/industrial`): mean **91.1 µs**/workflow, median **99.9 µs**,
  max **0.17 ms**, total **0.55 ms** across 6 workflows / 65 recursive states.
- AWS Solutions corpus (`corpus/aws-solutions`): mean **0.28 ms**/workflow, median **0.12 ms**,
  max **1.58 ms**, total **6.70 ms** across 24 workflows/artifacts / 385 recursive states.
- End-to-end CI gate latency, process start included: industrial p50/p95 **9.4/11.3 ms**;
  AWS Solutions CDK templates p50/p95 **15.7/20.9 ms**.
- Runtime overhead on AWS: **0** (verification is entirely ahead-of-deployment).

## E6 — DSL conciseness
- The order-processing workflow: **20** declarative DSL statements →
  **121-line / 2830-char** ASL (10 states). The DSL additionally encodes 5 record
  schemas, a 4-edge protocol, and per-task idempotency/persistence/compensation —
  semantics ASL cannot express.

## E7 — AWS round-trip (infra/)
- The DSL-verified workflows were deployed **unmodified** except for binding `FunctionName`
  to a stand-in Lambda ARN. Evidence summary: `eval/aws-roundtrip-modes.json`.
- **Express synchronous** (`infra/deploy_run.sh`): `infra/execution-evidence.json`,
  status **`SUCCEEDED`**, billed **500 ms / 64 MB** (≈ $0, free tier). Express retains no durable
  `get-execution-history` log.
- **Standard asynchronous** (`infra/deploy_run_standard.sh`): `infra/execution-evidence-standard.json`
  + `infra/execution-history-standard.json`, status **`SUCCEEDED`**, **24** durable history events
  (4 Lambda tasks scheduled/succeeded).
- **Standard live callback** (`infra/deploy_run_callback.sh`): `infra/execution-evidence-callback.json`
  + `infra/execution-history-callback.json`, status **`SUCCEEDED`**, **30** durable history events.
  The workflow pauses at `RequestApproval` (`lambda:invoke.waitForTaskToken`, TimeoutSeconds 3600,
  HeartbeatSeconds 900), then an external `SendTaskSuccess` resumes it with
  `{approved:true, approver:"external-reviewer", channel:"out-of-band-callback"}`.
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

## Implementation size — 7,577 physical source lines of Rust (excludes the 1,134-line test suite)
Definition: physical source lines counted by `wc -l` over `stepcheck/src/**.rs` excluding `tests.rs`
(release 0.1.5). The technical report counts non-blank, non-comment lines instead, so its figure is
smaller.
| Component | Module(s) | LOC |
|---|---|---|
| Workflow IR | ir.rs | 362 |
| Frontends (ASL parse+emit incl. full-fidelity emitter + CNCF jq model, typed DSL, CloudFormation/SAM) | asl.rs, dsl.rs, cncf.rs, cfn.rs | 1380 |
| Annotations + inference | annot.rs | 397 |
| Analysis passes (8 + SC5003 composition + manager) | passes/* | 2915 |
| Concrete-execution oracle (bounded loop unrolling) | concrete.rs | 431 |
| Diagnostics | diag.rs | 115 |
| Mutation engine | mutate.rs | 633 |
| CLI + stats + eval harness | main.rs | 1344 |
| **Total (excl. tests.rs)** | | **7577** |

## E12 — Further studies (reproducible)
- **Formal-verifier comparison:** `python eval/asl2bpmn/compare.py --limit 193` → `eval/asl2bpmn-comparison-full.json`.
  Woflan, BPMN Analyzer 2.0, and BProVe run locally on the ASL→BPMN workflow-net encoding.
  Per-class recall: StepCheck 100% all classes; formal verifiers overlap on SC0002 but are 0% on
  data/retry/temporal and mostly 0% on concurrency/compensation. Over-report on valid workflows:
  Woflan/BPMN Analyzer **16/193 (8.3%)**, BProVe **56/193 (29%)**. Exact CIs in
  `eval/mutation-stats.json` (`python eval/mutation_stats.py`). SAB-only check:
  `python eval/asl2bpmn/compare.py --corpus corpus/industrial --include sab-booking-processbooking ...`
  → `eval/sab-bpmn-comparison.json`; all three baselines accept clean SAB and catch only SC0002,
  while StepCheck catches SC0002/SC1003/SC3001/SC4001/SC6003 (SC5001 n/a on SAB).
- **Industrial-topology build gate:** `python eval/industrial/wse.py` → `eval/industrial-case.json`.
  Six industrial-topology sagas (incl. SAB reconstruction). Build-gate p95 is **16--21 ms** across
  0/1/5 injected-defect levels; it fails the build on 5/5 one-defect workflows and 3/3 five-defect
  workflows with applicable sites. CI YAML: `eval/industrial/ci-gate-example.yml`.
- **AWS Solutions CDK gate:** `python eval/industrial/aws_solutions_cdk_gate.py` →
  `eval/aws-solutions-cdk-gate.json`. Six CDK synth templates; mutants are embedded back into the
  template before timing. StepCheck catches 6/6 one-defect and 5/5 five-defect mutants; p95 is
  18--20 ms. `asl-validator` accepts 2/6 clean CDK definitions and silently passes both one-defect
  mutants over that accepted-clean denominator.
- **Hold-out inference:** `node eval/score-holdout.js` → `eval/holdout-inference.json`.
  Rules frozen before the 13 hold-out workflows were labelled; hold-out warning precision
  SC4001 27/29, SC3001 3/3, SC4010 4/4 (see `eval/GOLD-LABELLING.md`).
- **CNCF jq data-flow:** `eval/cncf-jq-coverage.json` — SC1101 fires on jq field reads;
  12/66 workflows have modelable references.
- **Concurrency composition:** SC5003 (`stepcheck scan corpus/asl` → 2 findings). Same-template
  child definitions are recursively flattened for SC5001 when referenced by logical id, `Ref`/`GetAtt`,
  `DefinitionSubstitutions`, or `StateMachineName`-derived ARNs; externally deployed children with no
  local definition remain an explicit scope boundary.
- **Emitter fidelity:** `node eval/emitter_fidelity.js` → `eval/emitter-fidelity.json` —
  193 ASL workflows; 100% states, 99.9% transitions, 100% measured JSONPath/JSONata/Map I/O fields,
  99.5% exact ASL-object equality, idempotent emit on all 193.
