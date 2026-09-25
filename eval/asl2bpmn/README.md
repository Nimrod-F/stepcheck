# ASL→BPMN encoding and comparison with formal workflow verifiers

This directory implements a head-to-head comparison
of StepCheck against **real, ran-as-baseline formal workflow-soundness verifiers**:
Woflan, BPMN Analyzer 2.0, and BProVe/BPMNOS.

## 1. Encoder (`encode.js`)

A dependency-free ASL → BPMN 2.0 encoder scoped to the **control-flow soundness
fair class**. Encoding rules:

| ASL | BPMN |
|---|---|
| A. Task / Pass / Wait | activity (`task`) |
| B. Choice | `exclusiveGateway` + one guarded flow per rule + Default (guards are labels) |
| C. Parallel | `parallelGateway` split … branches … `parallelGateway` join |
| D. Map | activity placeholder (item data flow out of scope) |
| E. Retry/Catch | Catch → `exclusiveGateway` after the task (normal vs. one error branch each); Retry is lossy metadata |

All terminals (`End:true`, `Succeed`, `Fail`, no-successor) are merged into a
single end event so the result is a **workflow net** (one source, one sink) that a
Petri-net soundness verifier can analyse. A dangling `Next` (e.g. after a
structural mutation) materialises as a dead-end placeholder so the net still
*builds* and the verifier genuinely reports the defect rather than failing to parse.

JSON document provenance, task contracts, service semantics, and executable data
guards are **not** encoded — they are counted as lossy features in the summary and
remain StepCheck-unique (data-flow SC1101 has no BPMN control-flow expression).

```bash
node eval/asl2bpmn/encode.js corpus/asl --limit 30 --out eval/asl2bpmn/out --summary eval/asl2bpmn-summary.json
```

## 2. Comparison (`compare.py`)

`compare.py` runs the confound-free comparison (mirrors `eval/validator_panel.js`):
for each workflow, `control = stepcheck emit` and `mutant_k = stepcheck mutate --kind k`
differ by exactly one injected defect of class `k`. It records, per class:

* **native** — StepCheck's `check` reports the class's expected SC code;
* **woflan / bpmn_analyzer / bprove** — the BPMN encoding of `mutant_k` carries a
  soundness violation absent from the encoding of the control (a *fresh* negative verdict).

It also reports the **clean-baseline over-report rate**: how many valid AWS
serverless workflows each formal verifier already calls unsound (a WF-net/ASL semantic mismatch,
not a StepCheck finding). Bootstrap 95% CIs on every per-class recall.

```bash
pip install pm4py
python eval/asl2bpmn/compare.py            # -> eval/asl2bpmn-comparison.json (Woflan only)

# add BPMN Analyzer 2.0 (Kraeuter 2024):
git clone https://github.com/timKraeuter/rust_bpmn_analyzer
(cd rust_bpmn_analyzer/cli && cargo build --release)
BPMN_ANALYZER=.../rust_bpmn_analyzer/cli/target/release/rust_bpmn_analyzer_cli \
  python eval/asl2bpmn/compare.py --limit 193 --out eval/asl2bpmn-comparison-full.json

# add BProVe/BPMNOS (Corradini et al.) via parser jar + Maude model checker:
python eval/asl2bpmn/compare.py --limit 193 --out eval/asl2bpmn-comparison-full.json \
  --bpmn-analyzer .../rust_bpmn_analyzer_cli \
  --bprove-parser .../BPMNOS_Parser.jar \
  --bprove-maude-model .../BPMNOS_MODEL_CHECKER.maude

python eval/mutation_stats.py          # adds exact CIs and full-corpus AP/F1 aggregates
```

For the paper's AWS Serverless Airline Booking case study, filter the same protocol to the SAB
workflow:

```bash
python eval/asl2bpmn/compare.py --corpus corpus/industrial \
  --include sab-booking-processbooking --limit 1 --out eval/sab-bpmn-comparison.json \
  --bpmn-analyzer .../rust_bpmn_analyzer_cli \
  --bprove-parser .../BPMNOS_Parser.jar \
  --bprove-maude-model .../BPMNOS_MODEL_CHECKER.maude
```

`compare.py` runs Woflan by default and adds BPMN Analyzer when `--bpmn-analyzer`/`BPMN_ANALYZER`
is set and BProVe when `--bprove-parser` plus `--bprove-maude-model` are set. BPMN Analyzer 2.0
checks safeness, option-to-complete, proper-completion, and no-dead-activities via a Rust
reachability-graph model checker with counterexamples. The local BProVe/BPMNOS path parses BPMN
with `BPMNOS_Parser.jar` and runs Maude LTL checks for safeness, option-to-complete, and
proper-completion.

The result is a scope/complementarity comparison: all tools overlap on the one strict BPMN soundness
class (SC0002); StepCheck is unique on data-flow (SC1101), binding, retry, and temporal ASL classes
that have no WF-net expression, while compensation and concurrency only appear as structural side
effects for the formal verifiers. On the full 193-workflow corpus, clean
over-report is 16/193 for Woflan, 16/193 for BPMN Analyzer, and 56/193 for BProVe.

`eval/mutation-stats.json` is the single consolidated statistics artifact. It reports exact recall
CIs plus full-corpus AP/F1 aggregates over all seven StepCheck mutation classes using
`eval/results-dataflow-result-shapes.json` for the native SC1101 typed-tier numbers. Because these
tools are deterministic binary checkers, AP is macro operating-point precision, not ranked average
precision. BProVe has no ASL data-flow model, so SC1101 is StepCheck versus out-of-scope rather than
a BProVe failure.

## 3. Other baselines

* **Syntax-validator panel** (`validator_panel.js`) — statelint, asl-validator, and
  AWS `ValidateStateMachineDefinition` remain the second, orthogonal baseline tier.
* **Generic external runner** (`run_baselines.js`) — kept for raw verifier availability/probing;
  scored results come from `compare.py`.
