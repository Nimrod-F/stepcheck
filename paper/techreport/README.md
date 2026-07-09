# Technical Report — Formal Development and Proofs

This folder holds the **standalone technical report** that accompanies the ICSOC
submission *Sound Data-Flow Verification for Safe Serverless Workflow Orchestration*
(StepCheck). It contains the material the main paper states but does not prove inline, so
the body stays within the venue page limit:

- the **abstract-interpretation** behind the sound data-flow provenance analysis — the
  shape lattice, concretization `γ`, transfer functions, the forward fixpoint, and the
  termination bound;
- the **workflow type discipline** (contracts + typestate) and its decision-procedure
  theorem;
- the **downstream-aware Saga-compensation** check (`SC4011`), with the algorithm in full;
- complete proofs of all three theorems, plus notes on the conservative concurrency and
  temporal analyses.

It is **self-contained** and compiles independently of the main paper.

## Theorem correspondence

| Technical report      | Main paper (`paper/sections/approach.tex`) |
|-----------------------|--------------------------------------------|
| Theorem 1 (soundness) | `\label{thm:soundness}` — No false positives |
| Theorem 2 (typecheck) | `\label{thm:typecheck}` — Checks decide well-typedness |
| Theorem 3 (saga)      | `\label{thm:saga}` — Compensation reachability is decided |

## Build

```sh
pdflatex main.tex
pdflatex main.tex   # second pass for the table of contents / refs
```

Produces `main.pdf`. No bibliography tool is needed (a small embedded `thebibliography`
is used). The grounding for each section is the released Rust source:
`stepcheck/src/passes/dataflow.rs` (§2), `passes/contract.rs` + `passes/typestate.rs`
(§3), and `passes/compensation.rs` (§4); the execution oracle that empirically witnesses
Theorem 1 is `stepcheck/src/concrete.rs`.

## Artifact note

Attach the compiled `main.pdf` (or this folder) to the artifact submission. The main
paper links here via a footnote in Section 3 (Approach).
