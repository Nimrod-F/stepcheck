# StepCheck — Trimming Plan, Round 2: 18 pp → 15 pp (including references)

Successor to `stepcheck_trim_plan_15pp.md` (the 38→15 round, now executed). The paper
currently compiles to **18 PDF pages** with the last page ~70% full → **≈17.7 effective pages**.
Target 15.0 including references ⇒ **cut ≈2.7 pp (~115 typeset lines at ~43 lines/page)**;
this plan budgets ≈2.9 pp for buffer.

The big structural moves are done. This round is (a) killing **cross-section repetition** —
several facts are stated 3–6 times — (b) one cheap float (Table 1), (c) caption/figure
shrinks, and (d) reference-entry compression. Every removed elaboration that still has value
goes to the tech report (`paper/techreport/main2.tex`), which already holds the proofs,
SC4011, conservative-analysis notes, and implementation notes.

---

## 1. Current page map (from main.pdf)

| Pages | Content | Floats |
|---|---|---|
| 1–3.2 | Front matter + §1 Intro | Fig 1 (p2) |
| 3.2–4.5 | §2 Background | Listing 1.1; **Table 1 floats to p5** |
| 4.5–10.4 | §3 Framework (~5.9 pp) | Table 1 (p5), Fig 2 arch (p6, full page-width), Table 2 (p7), Listing 1.2 |
| 10.4–12.2 | §4 Implementation | — |
| 12.2–15.9 | §5 Evaluation | Fig 3 detection (p13), Table 3 baseline (p14), Table 4 real bugs |
| 15.9–16.9 | §6 Related + §7 Conclusion + Artifact | — |
| 16.9–17.7 | References (21 entries, all cited) | — |

§3 is again the largest section and carries the most repetition; §5 is protected
(results carry the paper) except for duplicated caveats.

---

## 2. Redundancy inventory (verified against source, file:line)

| # | Fact | Stated at | Keep | Cut |
|---|---|---|---|---|
| R1 | JSONata parsed-but-⊤ | background.tex:30–34; approach.tex:108; implementation.tex:58–59; evaluation.tex:177–178 | approach (operative) + one clause in threats | background's future-work sentence (→TR); implementation's limitations sentence |
| R2 | Child-workflow concurrency caveat | approach.tex:164–165; evaluation.tex:188–190 (near-verbatim) | threats version | approach version |
| R3 | Oracle code-disjointness/scope | implementation.tex:47–53; evaluation.tex:108–110; evaluation.tex:181–184 | eval §5.4 (2 sentences) + 1-line threats caveat | compress implementation to 2 sentences |
| R4 | declared⇒error / inferred⇒warning grading | intro.tex:42–43; approach.tex:12–14; Fig 2 caption (approach.tex:31–33); Table 2 caption; approach.tex:222–225; implementation.tex:37–41 | intro + Table 2 caption + exit-code para | Fig-2-caption clause; §3.6 opening restatement; §3.1 para-2 clause |
| R5 | "sidecar / DSL / generated metadata / inference" enumeration | abstract; intro.tex:41–42; approach.tex:180–182; approach §3.6; evaluation.tex:37–39 | abstract + intro | approach §3.5 restatement → "from any premise source (§3.6)"; eval §5.2 → "once declared" |
| R6 | Sound-bug-finding-not-verification direction | abstract; intro.tex:48–51; approach.tex:96–99 | abstract + approach (operative) | intro's parenthetical restatement (keep the theorem pointer) |
| R7 | Six-defect walk | intro.tex:21–25; background.tex:50–55 re-walks all six | intro | compress §2.2 latency paragraph to 2 sentences ("all six deploy because each is valid structure; the platform does not infer contracts, protocols, idempotency, footprints, or Saga intent") |
| R8 | Conservative = under-approximating polarity | approach.tex:40–43; approach.tex:162–165 | §3.1 (near Table 2) | §3.4 restatement |
| R9 | Zero-migration / adopt-on-raw-ASL | intro.tex:43–44; approach.tex:7–9,11–12; implementation.tex:9–11 | intro + approach opening | implementation's Rust-rationale sentences |

**Bug found while auditing (fix regardless of trimming):** evaluation.tex:156 says
"the harder shapes **Reviewer-style** synthetic chains miss" — a leftover from the
review response that leaks reviewer context into an anonymous submission. Reword to
"harder shapes than synthetic chains".

---

## 3. Per-section cut list (savings in typeset lines; 43 ≈ 1 pp)

### Abstract (−4 lines)
- Compress the layered-semantics sentence (main.tex:88–91) to one clause; the full
  enumeration survives in §1/§3.6 (R5).
- Merge the results sentence: keep 193/66, six real bugs, 616 mutants, µs-scale; drop
  "as a pre-deployment check" (implied) and tighten the 33-callbacks clause.

### §1 Introduction (−7 lines)
- Fig 1 caption: shorten the marker-(2) elaboration to "(2) marks a one-edge
  ship-before-pay mutation, not an existing edge" (−2).
- Cut the R6 restatement at intro.tex:48–51: keep "reports a missing-field access only
  when every execution reaching the read lacks the field (Thm 1)", drop the
  "sound bug-finding … rather than complete verification" sentence (abstract + §3.3 have it).
- Contribution bullets 2 and 4 currently wrap to 3 lines each; rewrite each to ≤2 lines
  (bullet 4: "Evidence on 193 AWS and 66 CNCF workflows: six real bugs rediscovered,
  84/126 oracle-confirmed detections with zero false positives, 100% recall on 616 mutants").

### §2 Background (−20 lines ≈ 0.45 pp, incl. Table 1)
- **Cut Table 1 (background.tex:71–88) → TR.** Its content is already carried by R1–R4
  (requirements) + Table 2 (per-pass sources). Replace the pointer sentence
  (background.tex:68–69) with: "Native facts cover control flow and JSONPath data
  movement; contracts, typestate, idempotency, persistence, and compensators must be
  declared or inferred; resources and time bounds are partially native (full matrix in
  the TR)." (−13 lines: table + caption + whitespace)
- Cut the JSONata future-work sentence (background.tex:32–34, R1) → TR "extensions" note;
  keep the 5/193 (2.6%) statistic (−3).
- Compress the §2.2 defect-latency paragraph (background.tex:50–55, R7) (−4).

### §3 Framework (−35 lines + figure shrink ≈ 0.95 pp) — largest target
- **§3.1:** drop the R4 clause from the Fig 2 caption (caption 5 lines → 2) and shrink
  the figure from `\textwidth` to `0.85\textwidth` (−6 lines equivalent).
- **Table 2:** tighten the two longest cells — structural row lists five defect kinds
  (keep three + "…"), Saga row → "missing/incomplete compensation for persistent
  effects"; caption 4 lines → 2 (−5).
- **§3.2 IR:** delete the `\subsection` heading; fold the single paragraph into the end
  of §3.1 (−3).
- **§3.3 data-flow:** in the convergence paragraph (approach.tex:113–126) cut
  (i) the widening-alternative sentence — TR has the full Remark, keep "a depth-k
  widening would be a drop-in alternative (TR)"; (ii) the "at most one third of the
  permitted rounds" clause; (iii) the scale-sweep single-pass clause (−6). Merge the
  last two sentences of the restock example (−2). Compress the post-Theorem-1 paragraph
  (approach.tex:142–150): proof sketch → one sentence + TR pointer; SC1110 rationale →
  two sentences (−4).
- **§3.4 conservative:** trim the resource-key list to four examples + "and other
  statically named targets (full list in TR)"; cut the R8 polarity restatement; cut the
  R2 child-workflow caveat (threats keeps it) (−5).
- **§3.5 semantic:** compress the premise enumeration (R5) in the opening paragraph
  (−3); compress the post-Proposition paragraph (approach.tex:193–199) — keep the
  edge-local characterization + ship-before-pay clause, drop the "Unlike Theorem 1…
  comparable depth" sentence (Table 2's Claim column already scopes it) (−3).
- **§3.6:** cut the R4 restatement in the opening (−2); compress inference to
  "read-like ⇒ idempotent, write-like ⇒ non-idempotent, resource-acquiring ⇒ persistent,
  notification ⇒ non-idempotent; no signal ⇒ abstain; explicit always overrides" and the
  annotation-rot pair (SC0011/SC0012) to one sentence (−5).

### §4 Implementation (−17 lines ≈ 0.4 pp)
- Cut the Rust-rationale sentences (implementation.tex:8–11, R9); keep line count +
  "single dependency-light binary" (−3).
- Packaging: compress the CI-matrix + crates.io/npm sentences to one clause
  ("packaged for crates.io and npm; a copy-pasteable GitHub Actions gate ships with the
  artifact") → full detail to TR §6 (−4).
- Cut the parenthetical JSON-diagnostic example message (implementation.tex:33–35) —
  Fig 1's narrative already shows the ChargeCard/`$.total` case → example to TR §6 (−3).
- Trim the exit-code paragraph's closing sentence ("Either way … like a compiler or
  linter") (−2).
- Compress the oracle/mutation paragraph (implementation.tex:47–55, R3) to ~3 sentences;
  the code-disjointness rationale stays (one sentence), the mutation-class enumeration
  drops (Table 3 lists the classes) (−4).
- Limitations: cut the JSONata sentence (R1); keep distributed-Map coarseness and the
  emitter-fidelity sentence (−2). *Do not cut the emitter-independence firewall sentence
  (implementation.tex:44–45).*

### §5 Evaluation (−14 lines ≈ 0.33 pp) — protect all numbers
- §5.2: tighten the unbounded-callback explanation by one clause; R5 enumeration
  (evaluation.tex:37–39) → "once declared" (−4).
- §5.6 cost: fix the "Reviewer-style" leak (see §2 above); merge the DSL-conciseness and
  smoke-test sentences (−3).
- §5.8 threats: compress the oracle caveat (evaluation.tex:180–184) to 1.5 lines — the
  convergence-argument cross-reference survives as "(§3.3)" (−3); compress the
  industrial-corpus exclusion rationale (evaluation.tex:185–188) to 1.5 lines, full
  rationale → TR extended limitations (−2); keep the child-workflow caveat (R2 keeper);
  final smoke-test sentence → clause (−2).
- **Do not touch:** §5.3 mutation/baseline numbers, §5.4 84/126 + oracle zero
  counterexamples, §5.5 real-bug table, precision numbers (89%, 6/6, 1/7, 77%, 80%),
  the SC4011 chain-break experiment, §5.7 CNCF paragraph.

### §6 Related Work (−3 lines)
- Trim the Beldi clause ("—a Beldi-style runtime … opposite ends" → "pursuing the same
  failure-atomicity goal from the opposite end").
- Tighten the Flux sentence: keep "Flux could supply the idempotency premises SC3001
  consumes", drop "promoting its inference-driven warnings to declared-tier guarantees"
  (already said in §3/§5).

### §7 Conclusion + Artifact (−4 lines)
- Artifact paragraph: replace the five-item TR-contents enumeration with "a companion
  technical report with the full formal development, algorithms, feature matrices, and
  extended limitations."

### References (−8 lines ≈ 0.2 pp)
- Drop "Accessed YYYY-MM-DD" and version/license notes ("apache-2.0; v0.8.0",
  "server-side syntax validation") from the 6 web references.
- Drop the **Cadence** entry (cited once, always alongside Temporal + Durable Functions;
  update related.tex:5–6 to "Temporal [ref]" only). 21 → 20 entries.
- All other 20 entries are cited and stay.

---

## 4. Tech report additions (paper/techreport/main2.tex)

| TR location | New content (from the paper) |
|---|---|
| §1 Scope | former Table 1 (facts / native? / source) |
| §2 (after Remark on widening) | the fixpoint-margin detail: "≤ one third of permitted rounds at any machine; acyclic scale-sweep converges in one pass" |
| §5 Conservative notes | full resource-key list (EventBusName, ECS, Bedrock ModelId, StateMachineArn/Name); child-workflow caveat elaboration |
| §5 or new remark | JSONata/workflow-variables extension note (expression abstraction for reads/writes) |
| §6 Artifact notes | packaging detail (CI matrix, crates.io, npm wrapper, Actions gate); JSON diagnostic example record |
| **new §7 "Extended limitations"** | industrial-corpus exclusion rationale (full); smoke-test scope (Express-only, no live callback); anything else displaced from threats. **Note:** the paper's Artifact paragraph already promises "extended limitations" in the TR, but main2.tex has no such section — adding it fixes an existing inconsistency. |

---

## 5. Budget check

| Source | Savings |
|---|---|
| Abstract + §1 | ≈0.26 pp |
| §2 (incl. Table 1 cut) | ≈0.45 pp |
| §3 (prose + Fig 2 shrink + Table 2) | ≈0.95 pp |
| §4 | ≈0.40 pp |
| §5 | ≈0.33 pp |
| §6 + §7 | ≈0.16 pp |
| References | ≈0.20 pp |
| **Total** | **≈2.75 pp** → lands ≈15.0 |

**Contingency (if compile still overshoots):** shrink Fig 1 to `0.88\linewidth`
(−0.1 pp); trim Listing 1.2 sidecar to 6 lines (−0.05); demote the "Threats to
validity" `\subsection` to a `\paragraph` (−2 lines); tighten Table 3 caption.

**Do-not-cut list (protect throughout):** Fig 1; Table 2 with Claim column; Theorem 1,
Proposition 1, Theorem 2 statements + the A1–A4 model sentence; the restock-loop
example; the CLI/CI adoption paragraph (exit codes, `--infer`, `--annot`,
`--deny-warnings`); the emitter-independence firewall sentence; Table 3; the real-bugs
table; Fig 3 (detection); all §5 numbers; the SC4011 chain-break experiment; the CNCF
paragraph; the 786-fixpoint convergence sentence.

## 6. Execution order

1. **Pass A — mechanical (no rewriting):** Fig 2 shrink + caption; Fig 1 caption;
   Table 2 cells/caption; delete §3.2 heading; references compression + Cadence drop;
   Artifact paragraph; fix the "Reviewer-style" leak. (~0.8 pp)
2. **Pass B — redundancy kills (R1–R9):** apply the Keep/Cut columns of §2's table.
   (~1.1 pp)
3. **Pass C — section compression:** §2.2, §3.3 convergence, §3.6 inference, §4
   packaging/oracle, §5.8 threats, with the paired TR insertions from §4 of this plan.
   (~0.9 pp)
4. Recompile after each pass; stop cutting when `pdfinfo` reports 15.
