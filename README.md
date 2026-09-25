# StepCheck (ICSOC 2026 artifact)

This repository is the artifact for the paper *"StepCheck: Sound Static Verification of
Deployed AWS Step Functions Workflows"*: the **StepCheck** tool, the evaluation corpora,
the evaluation harness, and the AWS round-trip scripts. The paper is accepted at
ICSOC 2026 (to appear). The companion **technical report** (formal development, proofs and
evaluation detail, cited from the paper) is included at
[`docs/techreport.pdf`](docs/techreport.pdf). The version of this artifact cited in the paper
(release 0.1.5) is archived at [doi:10.5281/zenodo.22960209](https://doi.org/10.5281/zenodo.22960209);
[doi:10.5281/zenodo.22916631](https://doi.org/10.5281/zenodo.22916631) always resolves to the latest
version.

StepCheck is a static verifier for AWS Step Functions / Amazon States Language (ASL)
workflows. Its centrepiece is a **sound data-flow / field-provenance analysis**
of the ASL I/O-processing pipeline — an abstract interpretation over a may-present
lattice of document shapes that flags missing-field references with no false positives,
going beyond the JSONPath *syntax* that `statelint`/`asl-validator` check. Around it
StepCheck runs seven further checks — structural well-formedness, typed contracts,
**typestate** (business-protocol) conformance, **retry safety** (idempotency),
**concurrency interference** (Parallel/Map shared writes), **retry/timeout budget**, and
**Saga compensation completeness** — all before deployment, compiling verified workflows
back to executable ASL. Because raw ASL omits task semantics, StepCheck pairs sound
*native* checks with a lightweight *annotation* layer whose defaults are *inferred* from
task names and resource bindings.

## Implementation source hierarchy

![StepCheck implementation source hierarchy](docs/figure3-source-hierarchy.png)

## Layout

| Path | What |
|---|---|
| `stepcheck/` | the tool, in Rust (≈6.2k non-blank, non-comment lines excluding tests). `src/{ir,asl,dsl,cncf,cfn,annot,diag,concrete,mutate,main}.rs` and `src/passes/{structural,dataflow,contract,typestate,retry,compensation,concurrency,temporal}.rs` (eight passes). **Four front ends** (raw ASL, the typed DSL, CNCF Serverless Workflow, and CloudFormation/SAM/CDK templates) lower to one IR; the passes are unchanged across formats. `src/concrete.rs` is the code-disjoint bounded execution oracle. |
| `corpus/asl/` | 193 real Step Functions workflows mined from public AWS repos. `corpus/manifest.json` records provenance; `corpus/gold-labels.json` is the inference gold set (23 workflows, 172 hand-labelled tasks; protocol in `eval/GOLD-LABELLING.md`). `corpus/dsl/` holds the typed-DSL worked example; `corpus/dataflow/` the data-flow demonstrators. |
| `corpus/cncf/` | 66 real CNCF Serverless Workflow examples (second format). `corpus/cncf-typed/` holds the typed order workflow whose declared JSON Schemas let the contract check run natively (no inference). |
| `corpus/industrial/` | six industrial-topology workflows (65 recursive states) incl. four `aws-samples` Sagas and Serverless Airline Booking `ProcessBooking` — the CI-gate / cost study set. |
| `corpus/aws-templates/`, `corpus/aws-solutions/` | deployment artifacts for the CloudFormation front end: `aws-samples/serverless-patterns` SAM templates (55 workflows) and AWS Solutions Library machines (26, partly CDK-generated). |
| `corpus/realbugs/` | mined fix-commit pairs and issue-quoted workflows for the real-defect study; `corpus/wild-external/` holds the independent-repository set (overfitting check) and `corpus/wild-annot/` the declared-tier wild demo. The studies were run over 95 definitions from 16 repositories and 39 fix-commit pairs; the 36 files whose upstream publishes no licence are **not redistributed here** and are re-fetched by `node eval/mine_wild.js` / `node eval/mine_realbugs.js`. See `corpus/PROVENANCE.md` for every source, its licence, and what ships. |
| `eval/` | the evaluation harness and results: `SUMMARY.md`, `results.json`, `results-hard-mutants.json` (688 boundary mutants), `results-dataflow.json` (typed-tier data-flow recall), `dataflow-cert.json` (proof-certificate re-check), `scan-asl.json` (per-file in-the-wild diagnostics), `inference_accuracy.json` + `holdout-inference.json` (inference vs gold, full set and hold-out partition), `baseline-statelint.json` + `statelint_baseline.js` (six-class validator baseline), `asl2bpmn/` (workflow-net encoding and the Woflan / BPMN Analyzer / BProVe formal-verifier baselines), `GOLD-LABELLING.md` + `score-human-gold.js` (gold-set protocol and warning precision), `stats.tex`, `fixpoint-stats.json` (fixpoint round/bound utilisation), `scale/scale.csv` (100 → 30,000-state scaling) and `scale/topologies/` (deep `Map` and wide `Parallel` cases), `WILD-EXTERNAL-SUMMARY.md`. |
| `infra/` | the AWS round-trip: Express, Standard, and live `.waitForTaskToken` callback scripts, the 100-run Express/Standard benchmark (`bench_express_vs_standard.sh`), and captured execution evidence/history. Summaries: `eval/aws-roundtrip-modes.json`, `eval/deploy-runtime-bench.json`. |
| `docs/` | `techreport.pdf` — the companion technical report (*StepCheck: Sound Static Verification of Deployed AWS Step Functions Workflows*: formal development, proofs, baseline encoding, mutation operators and sidecar syntax, cited from the paper) — and figures. |

## Install

StepCheck ships as a single self-contained binary. To use it on your own workflows
(rather than to reproduce the paper, which is the "Build & run" section below):

```bash
# Node / npm - installs the `stepcheck` command
npm install -g @nimrod-f/stepcheck

# Rust / crates.io - builds from source
cargo install stepcheck
```

Both publish version 0.1.5. The npm package is a thin wrapper: its postinstall script
downloads the prebuilt binary for your platform (linux, macOS or Windows; x64 or arm64,
Node >= 16) and puts it on your PATH as `stepcheck`. The command is `stepcheck` either
way, so the scope in the package name does not leak into usage. If no prebuilt binary
matches your platform, or the download is unavailable, fall back to `cargo install
stepcheck`, which compiles it locally; you can also point the wrapper at another release
host with `STEPCHECK_REPO=owner/repo npm install -g @nimrod-f/stepcheck`.

```bash
# verify a workflow, using name-based inference for the effect facts ASL omits
stepcheck check workflow.asl.json --infer

# check the deployment artifact a team actually ships
stepcheck check template.yaml

# check against declared premises (contracts, typestate, idempotency, compensators)
stepcheck check workflow.asl.json --annot workflow.sidecar.toml
```

### What it reports, and when it stays silent

Severity tells you how much evidence is behind a finding. An **error** rests on the
artifact itself or on premises you declared, so a CI gate can block on it; a **warning**
rests on premises inferred from naming conventions (`--infer`), so it never blocks.

The data-flow check `SC1101` reports a missing field only when it can prove the field is
absent on *every* path, which is why it is silent more often than a linter would be. It
fires when the document is one the workflow itself constructs:

```bash
stepcheck check corpus/dataflow/native-bad.asl.json
# error[SC1101]: 'ChargeCard' reads '$.order.total', a field no execution reaching it can have produced
```

If the read instead resolves against the raw execution input, that input is unconstrained,
the analysis abstracts it to "any field may be present", and it stays silent by design
rather than guessing. To check those reads, declare the input schema in a sidecar and pass
it with `--annot`; the same sidecar unlocks the contract, typestate, retry and compensation
checks, which need facts ASL cannot express. `corpus/dsl/order.sidecar.toml` is a worked
example and `corpus/PROVENANCE.md` documents the corpora used above.

`stepcheck` exits `0` when clean, `1` when it reports errors and `2` on a parse failure,
so it drops into a build pipeline unchanged; add `--deny-warnings` to fail on warnings
too, once a workflow carries enough annotations for that to be meaningful. `--json` emits
one stable object per finding (code, severity, state, message, remediation hint). See
"Use StepCheck as a CI gate" below for a copy-pasteable GitHub Actions job.

## Build & run the tool

```bash
cd stepcheck && cargo build --release
BIN=target/release/stepcheck

# verify a real workflow (with inference)
$BIN check ../corpus/asl/<wf>.asl.json --infer

# the typed-DSL worked example (valid vs reordered)
$BIN demo order      | $BIN check /dev/stdin --annot ../corpus/dsl/order.sidecar.toml
$BIN demo order-bad  | $BIN check /dev/stdin --annot ../corpus/dsl/order.sidecar.toml   # 5 errors

# reproduce the evaluation
$BIN stats ../corpus/asl                 # corpus characterization
$BIN eval  ../corpus/asl --infer         # in-the-wild + mutation study + timing
node ../eval/inference_accuracy.js       # inference accuracy vs gold
$BIN dataflow-cert ../corpus/asl --result-shapes   # re-checks the data-flow proof-certificate obligations
$BIN path-coverage ../corpus/asl         # fraction of document-field references in the modeled fragment (92%)
$BIN fixpoint-stats ../corpus/asl ../corpus/cncf ../corpus/dsl > ../eval/fixpoint-stats.json  # fixpoint round/bound utilisation (termination)
node ../eval/asl2bpmn/encode.js ../corpus/asl --limit 30 --out ../eval/asl2bpmn/out --summary ../eval/asl2bpmn-summary.json  # workflow-net encoding for the formal-verifier baseline
node ../eval/asl2bpmn/run_baselines.js --summary ../eval/asl2bpmn-summary.json --out ../eval/bpmn-baseline.json  # runs/records the formal-verifier baselines
```

## Continuous integration & packaging

- **CI** (`.github/workflows/ci.yml`): builds and runs the full test suite on Linux,
  macOS, and Windows on every push and pull request (`cargo build`/`cargo test` gate;
  `node eval/check_repro.js` replays the deterministic fixpoint claims; `cargo fmt`/
  `cargo clippy` advisory).
- **Release** (`.github/workflows/release.yml`): pushing a `vX.Y.Z` tag builds a
  self-contained binary for Linux x86_64/aarch64, macOS x86_64/arm64, and Windows
  x86_64, attaches them to the GitHub release, and publishes the crate to
  **crates.io** and the wrapper package to **npm**.
- **Install**: `cargo install stepcheck` (Rust) or `npm install -g @nimrod-f/stepcheck`
  (the `npm/` wrapper downloads the matching prebuilt binary and exposes the `stepcheck`
  command; the npm package is scoped because the unscoped name collides with an existing
  package). Both honour the same exit-code contract, so a Rust- or Node-centric CI adds
  StepCheck as a build gate with one line.

### Use StepCheck as a CI gate

A copy-pasteable GitHub Actions step that gates a pull request exactly like a compiler or linter —
it fails the build on any **error** (add `--deny-warnings` to also fail on inferred warnings once a
workflow is annotated):

```yaml
# .github/workflows/verify-workflows.yml
name: Verify Step Functions
on: [pull_request]
jobs:
  stepcheck:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm install -g @nimrod-f/stepcheck        # or: cargo install stepcheck
      - name: Verify every workflow definition
        run: |
          for f in $(git ls-files '*.asl.json'); do
            stepcheck check "$f" --infer || exit 1     # exit 0 clean · 1 error · 2 parse failure
          done
```

Publishing is gated on two optional repository secrets, `CARGO_REGISTRY_TOKEN` and
`NPM_TOKEN`; the release jobs skip themselves when a token is absent. The release
repository that hosts the downloadable binaries is set in `stepcheck/Cargo.toml`
(`repository`) and baked into the npm installer (`npm/install.js`); end users can
override the latter with `STEPCHECK_REPO=owner/repo` only if hosting the binaries
elsewhere.

## Headline results (all reproducible)

- **In the wild (RQ3)**: on the 193 unmodified AWS sample workflows an inference-mode run
  emits **226** diagnostics — **181** advisory inferred-tier warnings (SC3001/SC4001/SC4010)
  and **45** native-tier findings across four codes, dominated by **33** unbounded
  `.waitForTaskToken` callbacks (SC6001); **99/193** workflows are flagged. All 45 native
  findings are adjudicated by a code-disjoint verifier that re-checks each finding's
  structural predicate against its ASL source: every one is confirmed.
- **Mutation study (RQ1)**: **616** injected defects across six classes, **100%** detection
  with exact 95% CIs per class. The schema validators (`statelint`, `asl-validator`, AWS
  `ValidateStateMachineDefinition`) catch only the two schema-level classes — `statelint`
  totals **306/616 (50%)** — and **0** of unsafe retry, missing compensation, concurrency,
  or temporal. Three formal workflow-soundness verifiers run locally (Woflan, BPMN
  Analyzer 2.0, BProVe) express only dangling transitions (95% / 87% / 69%), reach at most
  37% on compensation and 5% on concurrency, and report **8.3% / 8.3% / 29%** of the *valid*
  workflows as unsound under our ASL→BPMN encoding (a property of that encoding and the
  classical soundness definition, not a defect count; see the technical report).
- **Hard-mutant study (operator–check independence)**: a matched suite of **688** boundary
  mutants (`--hard`) — each a genuine defect moved to the edge of what the analysis can prove.
  Aggregate recall **0.48** (328/688; 0.24 before 0.1.5 extended the SC1003 scan to `ResultSelector`/`ItemSelector`): data-flow, concurrency, and temporal drop
  to **0** at their ⊤ boundary (reports the analysis declines to make, not false alarms),
  retry degrades to **0.68**, and compensation to **0.79** via the sibling check SC4010. Two
  variants are generalization controls rather than ⊤ boundaries — the dangling edge
  hidden in a nested sub-machine, and the broken payload moved from `Parameters` into a
  `ResultSelector` — and both stay exact at **1.00**, since the structural pass recurses
  into sub-machines and SC1003 reads every payload template. See
  `eval/results-hard-mutants.json`.
- **Data-flow provenance, SC1101 (RQ2)**: no report on the clean corpus; a
  missing-field read injected into each of the **126/193** applicable workflows is detected
  in **88 (70%)** with `--result-shapes` — the other 38 sit behind constructs the analysis
  soundly lifts to ⊤. A proof-certificate checker certifies all 88 reports (1,146
  widened-fixpoint postcondition obligations, `stepcheck dataflow-cert`) and the concrete
  execution oracle finds zero present-field counterexamples. **92%** of the corpus's
  document-field references fall in the exactly-modeled fragment (`stepcheck path-coverage`).
- **Real defects**: in **39** mined fix-commit pairs a diagnostic appears only on the buggy
  revision in 7; adjudication confirms **6** genuine in-scope defects. In **3** user-filed
  issues that quote a runtime error, it flags the underlying missing-field read (SC1101) in
  all **3**. On **95** ASL definitions from **16** independent repositories it produces
  representative findings on workflows we neither authored nor curated.
  - The six fix-commit defects come from `aws-samples/aws-batch-runtime-monitoring` (SC1101),
    `allenheltondev/serverless-ai-fitness` (SC1101 and an SC1110 dead subscriber branch),
    `manikanta5827/leave-management` (SC6001), `sparameswaran/airway-shipment-orchestrator`
    (SC0002) and `nicktodd/video-translation-stepfunctions` (SC0003/4/5); see
    `corpus/realbugs/README.md` for the per-bug table.
  - The three issues are `scttfrdmn/campus-compute` #32, `dataPlor/turbofan` #1 and
    `aws-samples/serverless-coffee-workshop` #56, each with its URL and the verbatim reported
    error in `eval/issue-tracker-defects.json`.
  - The 16 independent repositories are `vdaron/StatesLanguage`,
    `skyflow-workflow/skyflow_backend`, `wmfs/statebox`, `ChristopheBougere/asl-validator`,
    `mugglmenzel/step-functions-example-workflow`, `yskszk63/sam-local-asl`,
    `aws-iot-builder-tools/iot-workflow-management-and-execution`,
    `pssolanki111/pyDelhi_step_functions` and eight `Thrubit/*` domain repositories
    (freight-booking, credit-card-transaction, payment-settlement,
    launch-vehicle-manufacturing, vehicle-recall-management, network-outage-management,
    vehicle-order-fulfillment, mission-control-operations). `corpus/wild-external/manifest.json`
    records the branch and path of every file, and `corpus/PROVENANCE.md` the licence of every
    repository.
- **Inference accuracy vs gold** (23 workflows, 172 hand-labelled tasks, one labelled set):
  **76.4%** idempotency, **85.0%** persistence at **81.4%** coverage. Warning precision on
  gold-labelled tasks: **47/50 (94%)** overall, **34/37 (92%)** for compensation warnings
  (SC4001); on the 13-workflow hold-out partition, SC4001 27/29. See `eval/GOLD-LABELLING.md`.
- **Deployment artifacts (CloudFormation front end)**: on `aws-samples/serverless-patterns`
  SAM templates, with no manual extraction, StepCheck covers **55** workflows and flags
  **13** (two native findings, SC0007 and SC6001; the rest advisory); on the AWS Solutions Library (**26** machines, partly
  CDK-generated) it flags **14** alongside clean true negatives, and re-running the gate on
  six CDK-synth templates catches every applicable mutation.
- **Cost (RQ4)**: mean **≈61 µs** / median **≈35 µs** / max **≈1.8 ms** per workflow on the
  193-workflow corpus (eight passes incl. the data-flow fixpoint; single-run wall-clock figures
  in `eval/results.json`, so they vary between runs); industrial set mean
  **≈91 µs**; AWS Solutions mean **≈0.28 ms**. End-to-end CI-gate latency stays below
  **21 ms** at p95 (CDK templates p50/p95 15.7/20.9 ms). The widened fixpoint (k=12)
  converges on all 262 committed definitions in at most 7 rounds. Scale: a synthetic
  **10,000**-state workflow verifies in ≈**31 ms**, 30,000 states in ≈**142 ms**
  (`eval/scale/scale.csv`); **0** runtime overhead.
- **AWS round-trip**: the emitter round-trips the corpus with exact ASL-object equality for
  **192/193**, and verified definitions deploy unmodified and complete **100/100** live runs
  in both Express and Standard mode, plus a live `.waitForTaskToken` callback execution
  resumed by external `SendTaskSuccess` (durable 24- and 30-event histories captured in
  `infra/`).
- **Cross-format (CNCF)**: the front end lowers all **66** CNCF Serverless Workflow examples
  to the same IR with **no change to the IR or any pass**; the control-flow checks apply
  unchanged and the mutation study catches every injected control-flow defect. Field-flow
  generalizes less: **12/66** examples declare records the sound SC1101/SC1110 checks can
  model; the other 54 are schema-free, so those checks stay silent rather than claim
  coverage. Where CNCF declares JSON Schemas, SC1010 enforces them natively
  (`stepcheck check corpus/cncf-typed/order-bad.yaml`).
- **Automotive case study**: a private, anonymized production module (three Express
  workflows, 34 states) summarized in the paper — an inference-mode run reported 30 findings:
  25 retry/timeout mismatches (SC6002; Lambda tasks retrying up to 126 s against declared
  10–20 s machine timeouts) and 5 concurrency warnings (SC5001). After the team authored a
  declared-tier sidecar, re-checking reported the same 5 concurrency warnings, confirming that
  the inferred durability was correct. A lead developer confirmed all 30 as genuine, deployable defects, each
  subsequently fixed. The module itself is not distributable and is not in this repository.

## Reproduce the AWS round-trip (optional; creates & deletes resources)

```bash
bash infra/deploy_run.sh                    # Express synchronous run
bash infra/deploy_run_standard.sh           # Standard async run + durable history
bash infra/deploy_run_callback.sh           # Standard live callback + SendTaskSuccess
bash infra/bench_express_vs_standard.sh     # 100-run Express/Standard benchmark
bash infra/teardown.sh                      # delete created state machines + Lambdas
```

## Licence

StepCheck is released under the Apache License 2.0 (see `LICENSE`). That licence covers
the tool and the workflows we authored; the third-party workflows in `corpus/` remain
under their upstream licences, which `corpus/PROVENANCE.md` records source by source,
together with the attribution those licences require.
