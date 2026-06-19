# ICSOC 2026 Paper + System — Implementation Plan

**Paper (working title):** *Safe Serverless Workflow Orchestration through Typestate
Verification, Retry Safety, and Compensation Checking*
**Tool:** **StepCheck** — a static verifier for AWS Step Functions / Amazon States
Language (ASL) workflows. CLI binary `stepcheck`.

---

## 1. The reframing (why the design changed from the draft)

The existing draft (`ICSOC2026 (4).pdf`) proposes a **TypeScript embedded DSL** that
checks four properties and compiles to ASL. Two problems:

1. Its evaluation is **fabricated** (the TS PDFs literally say *"replace the invented
   Evaluation tables"*).
2. An *embedded* DSL can only check workflows **written in the DSL** — so it **cannot be
   evaluated on real workflows mined from GitHub**, which is exactly what you asked for.

**Decision — language: Rust.** To evaluate on *real* workflows we need a **standalone
analyzer that ingests existing ASL JSON directly** (serde parser → IR → pass pipeline →
diagnostics, rustc-style). That requirement settles the language: Rust. It is also the
stronger research-artifact story ("a real tool that finds real bugs"). The original typed
DSL is **preserved as one frontend** for greenfield authoring; it is no longer the whole
system.

## 2. The honesty model (two layers)

Raw ASL carries **no** declared task types, business-state labels, idempotency flags, or
"compensation" notion. So StepCheck has two analysis layers:

- **Layer A — Native (no annotations needed):**
  - *Structural validation*: single `StartAt`; all `Next`/`Default`/`Catch` targets exist;
    no unreachable states; every non-terminal has a transition; `Choice` exhaustiveness.
  - *Data-flow contract checking* (JSONPath): track which fields are available in the
    state document; flag a `Parameters`/`InputPath`/`Choice` reference to a field no
    predecessor could have produced (the paper's *amount* vs *total* bug — found
    statically, on real ASL).
- **Layer B — Annotation-enhanced (full precision):**
  - A lightweight sidecar file attaches per task: idempotency, persistence + compensation
    target, business typestate in/out, optional I/O schema.
  - Annotations can be **inferred** from task/Resource names (`charge|pay|create|reserve|
    send` → non-idempotent/persistent; `get|read|list|validate` → idempotent), giving a
    zero-effort default whose accuracy is itself measured.

Every claim in the paper states which layer produced it. No injected error is ever
reported as "found in the wild."

## 3. Architecture (Rust, modular via two traits)

Single crate `stepcheck`, compiler-style modules:
`ir` · `frontend::{asl,dsl}` · `annot` · `analysis::{structural,contract,typestate,retry,
compensation}` · `diagnostics` · `emit` · `cli`.

Genericity spine:
```rust
trait Frontend { fn load(&self, src:&str) -> Result<WorkflowGraph>; } // new input format
trait Pass     { fn code(&self)->&str; fn run(&self, wf:&WorkflowGraph,
                                              ann:&Annotations, sink:&mut DiagnosticSink); } // new check
```
New checks = new `Pass`; new formats = new `Frontend`. (Realizes the draft's
"Extensibility" design goal.)

Diagnostics carry a code (`SC0xxx` structural, `SC1xxx` contract, `SC2xxx` typestate,
`SC3xxx` retry, `SC4xxx` compensation), severity, state, span, and a suggestion;
human + JSON output.

## 4. Evaluation methodology (every number reproducible)

- **E1 Corpus characterization** — N real workflows from GitHub (`aws-samples/*`, CNCF);
  table of #states + feature histogram. Provenance recorded in `corpus/manifest.json`
  (repo, path, commit, license).
- **E2 Findings in the wild** — native analyses on *unmodified* real ASL; report genuine
  issues; manually validate a sample for precision.
- **E3 Mutation-based detection** — inject one error per class into real workflows
  (`stepcheck mutate`); measure recall + false-positive rate on originals. (Real analog of
  the fabricated Table 2.)
- **E4 Inference accuracy** — hand-labeled gold subset vs name-based heuristics
  (precision/recall).
- **E5 Performance** — verification time per workflow + scaling vs #states.
- **E6 DSL conciseness** — DSL LOC vs ASL LOC.
- **E7 AWS round-trip** — emit ASL → deploy a tiny Step Functions stack → execute →
  confirm `SUCCEEDED` → tear down. Evidence for "deployable unmodified + zero runtime
  overhead." (Pending your go-ahead.)

## 5. Paper changes

- **Soften formalism** (practical venue): collapse Sec 3's set-theory (S, δ⊆S×S,
  Effects(T), Algorithm 1, O(n)) into an example-driven prose "Workflow Model."
- **Rewrite Implementation (Sec 7)** for the Rust tool with real LOC.
- **Rewrite Evaluation (Sec 8)** with E1–E7 real results.
- **Reframe contributions/abstract** around verifying *existing* ASL + annotation/inference
  + modular Rust tool + real-corpus eval + AWS round-trip.
- **Fix related work** (broken `[?]` refs), integrate new citations, expand comparison.
- **Redraw figures in TikZ**; Springer **LNCS** format; target **~15 pages** (full paper).

## 6. Deliverables on disk

```
stepcheck/   Rust crate (the tool)
corpus/      real workflows (.asl.json) + manifest.json + annotations/
eval/        harness + results (CSV + .tex) + mutation outputs
infra/       AWS round-trip template + deploy/teardown + captured evidence
paper/       LaTeX (llncs.cls, splncs04.bst, references.bib, sections/, figures/)
PLAN.md      this plan
```

## 7. Phases

- **P0** research workflow (running) + this design — done.
- **P1** curate real corpus + provenance manifest.
- **P2** Rust core: IR + ASL frontend + structural pass + diagnostics → parse whole corpus.
- **P3** annotations + inference; contract/typestate/retry/compensation passes; DSL
  frontend + ASL emitter.
- **P4** eval harness + mutation → E1–E6 tables.
- **P5** AWS round-trip (E7) — after your OK.
- **P6** rewrite paper in LNCS with real numbers; TikZ figures; compile PDF.

**Success criterion:** every number in the paper traces to a command anyone can re-run on
the committed corpus + tool.
