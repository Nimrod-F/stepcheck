# Paper claims and how to reproduce them

Each row is a number from *StepCheck: Sound Static Verification of AWS Step Functions
Workflows* (ICSOC 2026). `node eval/reproduce.js --tier N` checks every row of tier N and
below, prints PASS/FAIL against the paper and writes fresh outputs to `eval/repro/`.
Commands run from the repository root. `stepcheck` is the release build
(`stepcheck/target/release/stepcheck`) or the one on `PATH` in the Docker image.

| Tier | Needs | Time |
|---|---|---|
| 0 | the `stepcheck` binary (+ `cargo` for the tests) and Node ≥ 18 | ~2 min |
| 1 | tier 0, plus Python 3; the build-gate study also needs `pm4py` 2.7.23.1 | ~5 min |
| 2 | statelint 0.8.0, asl-validator 4.0.0, pm4py, BPMN Analyzer 2.0, BProVe/BPMNOS + Maude 3.5.1 (all in the Docker image) | 1–2 h |
| 3 | network access to GitHub (no token) | ~1 min |

## Section 2 — running example (Fig. 1)

| Claim | Command | Evidence | Tier |
|---|---|---|---|
| ASL alone decides defects (1) `SC1101` and (5) `SC6001` | `stepcheck check corpus/examples/running-example.asl.json` | `corpus/examples/README.md` | 0 |
| name-based inference adds (3) `SC3001`, (4) `SC5001`, (6) `SC4001` as warnings | `stepcheck check --infer corpus/examples/running-example.asl.json` | same | 0 |
| the definition passes AWS validation | `aws stepfunctions validate-state-machine-definition --type STANDARD --definition file://corpus/examples/running-example.asl.json` | needs AWS credentials | – |

## Section 4.1 — RQ1, detection power (Table 1)

| Claim | Command | Evidence | Tier |
|---|---|---|---|
| 193 workflows, 1,355 states | `stepcheck eval corpus/asl --infer` | `eval/results.json` | 1 |
| 616/616 designed mutants detected; per-class *n* 166/141/103/19/21/166 | same | same | 1 |
| exact 95% CIs [.82, 1] at *n* = 19 and [.98, 1] at *n* = 166 | computed from the counts | `eval/mutation-stats.json` | 1 |
| hard suite: 328 of 688 (0.48); per-class *n* and recall in Table 1 | `stepcheck eval corpus/asl --infer --result-shapes --hard` | `eval/results-hard-mutants.json` | 1 |
| schema validators: AWS / statelint / asl-validator columns | `node eval/validator_panel.js corpus/asl --out eval/repro/validator-panel.json` (`STEPCHECK_AWS=1` adds the AWS column) | `eval/validator-panel.json` | 2 (AWS: recorded) |
| soundness verifiers: BProVe / BPMN Analyzer / Woflan columns | `python eval/asl2bpmn/compare.py --limit 193 --out eval/repro/asl2bpmn-comparison-full.json` | `eval/asl2bpmn-comparison-full.json` | 2 |
| valid workflows reported unsound: 56 / 16 / 16 of 193 | same | same, `clean_baseline` | 2 |

## Section 4.2 — RQ2, data-flow soundness and coverage

| Claim | Command | Evidence | Tier |
|---|---|---|---|
| `SC1101` reports nothing on the clean corpus | `stepcheck eval corpus/asl --infer` | `eval/results.json` | 1 |
| 88 of 126 injected missing-field reads detected | `stepcheck eval corpus/asl --infer --strict-input --result-shapes` | `eval/results-dataflow-result-shapes.json` | 1 |
| certificate: all 88 certified, 1,146 obligations, 0 failed | `stepcheck dataflow-cert corpus/asl --infer --result-shapes` | `eval/dataflow-cert.json` | 1 |
| bounded oracle: 0 counterexamples | `stepcheck oracle corpus/asl --infer --result-shapes` | `eval/oracle-result-shapes.json` | 1 |
| modeled fragment covers 92% (742 of 803) of document-field reads | `stepcheck path-coverage corpus/asl` (803 = all reads minus `$$` and `States.*`) | `eval/path-coverage.json` | 1 |
| hard operator: 0 of 126 | hard-suite command above | `eval/results-hard-mutants.json` | 1 |

## Section 4.3 — RQ3, real-world evidence

| Claim | Command | Evidence | Tier |
|---|---|---|---|
| 39 fix-commit pairs mined, 7 flagged, 6 genuine | `stepcheck eval-pairs corpus/realbugs --infer` replays the kept pairs (5 ship, 2 re-fetched in tier 3); tier 3 also re-mines the 30 reproducible pairs (`node eval/mine_realbugs.js --repos … --until 2026-06-22 --out <dir>`) and replays them | `corpus/realbugs/mined-pairs.json` (every pair, commit, verdict), `corpus/realbugs/realbugs.json` | 1 (recorded) / 3 |
| 3 user-filed issues, each flagged with `SC1101` | `stepcheck check` on `corpus/realbugs/issues/*` (sidecars for turbofan and coffee-workshop) | `eval/issue-tracker-defects.json` | 1 |
| 226 diagnostics on the 193 workflows: 181 inferred-tier, 45 native, 33 `SC6001` | `stepcheck eval corpus/asl --infer` (`baseline`) | `eval/results.json` | 1 |
| a code-disjoint checker confirms all 45 native findings | `node eval/native_census_verify.js` | `eval/diagnostic-precision-census.native.json` | 1 |
| 95 definitions from 16 independent repositories | `node eval/fetch_unredistributed.js`, then `stepcheck scan corpus/wild-external --infer` | `eval/results-wild-external.json`, `corpus/wild-external/manifest.json` | 1 (recorded) / 3 |
| SAM: 13 of 55 workflows flagged | `stepcheck check --json --infer` on each file of `corpus/aws-templates`, grouped by pattern directory | `eval/aws-templates-consolidation.json` | 1 |
| AWS Solutions Library: 14 of 26 machines flagged | `stepcheck scan corpus/aws-solutions --infer` (`machines`, `flagged_machines`) | `eval/aws-solutions-consolidation.json` | 1 |
| 66 CNCF examples run unchanged; 12 declare records | `stepcheck eval corpus/cncf --infer` | `eval/results-cncf.json`, `eval/cncf-jq-coverage.json` | 1 (12: recorded) |
| automotive case study (30 findings) | — | private module; not distributable | not reproducible |

## Section 4.4 — RQ4, build-time cost

| Claim | Command | Evidence | Tier |
|---|---|---|---|
| six industrial-topology workflows, 65 states | `stepcheck stats corpus/industrial` | — | 1 |
| the gate fails the build on every workflow with an applicable mutation (1 and 5 defects) | `python eval/industrial/wse.py --out eval/repro/industrial-case.json` | `eval/industrial-case.json` | 1 (needs pm4py) |
| gate latency below 21 ms at p95 (60 runs per workflow) | same; also `python eval/industrial/aws_solutions_cdk_gate.py` for the CDK templates | same, `eval/aws-solutions-cdk-gate.json` | 1 (machine-dependent) |
| 10,000-state workflow in about 31 ms | `python eval/gen_scale.py 10000 <dir>/w.json && stepcheck eval <dir>` | `eval/scale/scale.csv` | 1 (machine-dependent) |

Timings were measured on an Intel Core i7-8550U laptop (16 GB, Windows 11) with a Rust 1.96
release build; expect other values on other machines. The ordering and orders of magnitude
should hold.
