# StepCheck — ICSOC 2026 PC-Style Academic Audit (5 personas)

**Manuscript:** *Static Verification of AWS Step Functions with Sound Data-Flow Analysis and Workflow Semantics* (paper/main.tex, 15 pp incl. references, LLNCS)
**Audit date:** 2026-07-07 · **Round:** pre-submission audit (post trim to 15 pp)
**Panel (from user request):** PC Chair (EiC) · R1 SE/empirical methodology · R2 cloud/serverless & SOC domain · R3 formal methods · Devil's Advocate (systems/industry skeptic)

---

## Phase 0 — Reviewer Configuration Card

| Slot | Persona | Focus |
|---|---|---|
| EiC | ICSOC PC chair, service-oriented computing generalist | Fit, structure, readability, decision |
| R1 | Empirical SE professor (mutation testing, tool evaluation, mining studies) | Evaluation methodology, construct validity, reproducibility |
| R2 | Cloud/serverless & BPM/SOC domain expert (BPEL/BPMN verification lineage) | Literature, domain contribution, adoption realism |
| R3 | Formal methods / static analysis expert (abstract interpretation, behavioural types) | Theorem scope, lattice/fixpoint correctness, claim calibration |
| DA | Systems/industry skeptic | Strongest counter-argument, framing asymmetries, "so what?" |

---

## Section-by-Section Audit

### Abstract (main.tex:80–97)
Strong: quantitative, properly hedged ("no false positives **under the modeled JSONPath semantics**"), covers all six defect classes and headline numbers. Minor risks: "Sound Data-Flow Analysis" in the **title** can read as verification-soundness (no missed bugs) to FM readers, whereas the paper means sound *bug-finding* (no false alarms); "tens of microseconds" is a marketing number — "the whole 193-workflow corpus verifies in under 10 ms" is more meaningful. **Grade: A−**

### §1 Introduction (intro.tex)
Figure 1 with six numbered defect classes is an effective anchor; contributions list is crisp and each is measurable. Two nits: the sentence "The unifying contribution is not a bag of independent lints" (intro.tex:35–36) is defensive — it names the criticism it fears; the Fig. 1 caption clause about marker (2) being "a one-edge mutation, not an existing edge" reads like a patch (necessary, but could be smoother inside §2.2). **Grade: A−**

### §2 Background (background.tex)
Compact and functional: fixes exactly the ASL slice used, quantifies the JSONata gap (5/193, 2.6%), and R1–R4 give reviewers a rubric the evaluation later answers. The four omitted semantic facts arrive in one dense math-laden sentence (background.tex:41–45) — split it. "Full matrix in the technical report" is fine at 15 pp. **Grade: A−**

### §3 Verification Framework (approach.tex)
The heart, and mostly strong. Table 1 is load-bearing and good. The data-flow subsection is honest to a fault: correct polarity statement (sound bug finder, not complete verifier), explicit modeled-fragment boundary, non-convergence policy (suppress rather than under-report), and empirical convergence (786 fixpoints ≤ 7 rounds). Issues:
- The fixpoint paragraph (approach.tex:102–111) carries the lattice-height bound, the non-convergence policy, the widening alternative, and the empirical result in **one paragraph** — split into termination vs. policy.
- **SC1010 edge-locality question (FM):** Prop. 1 tests `in(T2) ⊆ out(T1)` on adjacencies (approach.tex:160–167). If `out(T)` means "fields T produces" rather than "document shape after T", fields produced upstream and passed through the document would false-positive. The text doesn't disambiguate. One sentence fixes it.
- **Thm 2's Saga model** (approach.tex:184–191) is so restricted (single post-commit fault, unique generic catcher, linear handlers) that sound+complete is near-definitional. State what SC4011 does when a real workflow violates the model (silent? warning?).
**Grade: B+**

### §4 Implementation (implementation.tex)
Excellent for its length: 5,045 LoC, `Frontend`/`Pass` interfaces validated by the CNCF add ("touched neither the IR nor any pass"), CI exit-code policy, and the **code-disjoint execution oracle** with the right rationale ("a shared transfer-function bug would otherwise validate itself"). Limitations paragraph present. **Grade: A**

### §5 Evaluation (evaluation.tex)
Unusual breadth for 15 pp: wild corpus (193), mutation vs. 3 baselines (616), oracle-validated injection (84/126), fix-commit mining (6 real bugs), perf + scale sweep (30k states), CNCF transfer (66), human gold labels (κ = 0.89/0.99). Weaknesses:
- **Mutant circularity:** mutants come from `stepcheck mutate` — the tool is graded on defects its own authors designed to be detectable. The caveat exists ("per-check reliability on targeted operators", evaluation.tex:57) but belongs in Threats too.
- **SC3001 inference precision is 1/7** (evaluation.tex:41–42) — honest, but the adoption-mode story should confront it (e.g., demote inferred SC3001 or discuss).
- **Baseline expectedness:** syntax validators detecting 0 semantic defects is by-construction; the comparison substantiates the gap but shouldn't be framed as a win.
- **Missing denominators:** how were the gold-covered findings (37 of 117 SC4001) selected? What were the 32/39 fix-commit pairs StepCheck missed? What was the 7th (rejected) candidate?
- **Threats to validity is a `\paragraph` nested under §5.7 (CNCF)** — structurally wrong home; readers scanning subsection heads won't find it.
**Grade: B+**

### §6 Related Work (related.tex)
Well-themed, and the complementarity moves are elegant (Flux *supplying* SC3001's premises). Gaps for an ICSOC audience: the classic workflow data-flow validation line (Sadiq et al., ADC 2004) and the WS-BPEL verification lineage (e.g., Ouyang et al.'s Petri-net-based WS-BPEL analysis; Foster et al.'s WS-Engineer) — one sentence each; ICSOC PC members grew up on these. **Grade: B+**

### §7 Conclusion (conclusion.tex)
Does its job; artifact paragraph (anonymized 4open.science + TR pointer) is exactly what ICSOC artifact-minded reviewers want. **Grade: A**

---

## Phase 1 — Reviewer Reports

### EiC — ICSOC PC Chair
**Recommendation:** Minor Revision (weak accept → accept band) · **Confidence:** 4

Topical fit is excellent — service composition correctness is core ICSOC, and the paper deliberately bridges BPM-era workflow analysis to serverless artifacts. Claims are calibrated (every strong word is scoped), the artifact story is complete, double-blind is respected, and 15 pp is met. My concerns are presentation-level: (1) terminological load — ~20 SC-codes plus a five-word tier vocabulary (native/declared/inferred/conservative/semantic) asks a lot of a generalist PC member; Table 1 anchors it, but the prose should lean on defect-class names, not codes; (2) the Threats paragraph must be a visible subsection; (3) several compressed sentences (post-trim artifacts) need re-expansion, notably approach.tex:102–111 and background.tex:41–45; (4) Table 2's `\resizebox` risks sub-LNCS font sizes — check.

**Strengths:** S1 fit and lineage; S2 claim calibration throughout; S3 requirements R1–R4 answered by the evaluation one-for-one; S4 artifact + TR completeness.
**Weaknesses:** W1 (Major) SC-code density for generalist readers; W2 (Minor) Threats placement; W3 (Minor) compressed sentences; W4 (Minor) title's "Sound" ambiguity for skim-readers.

### R1 — Empirical SE / Methodology
**Recommendation:** Minor Revision · **Confidence:** 5

The evaluation's multi-method design (controlled power kept separate from wild evidence, code-disjoint oracle, κ-quantified gold labels) is above the bar for tool papers at this venue. But three construct-validity items need explicit handling:

- **W1 (Major) — Self-generated mutants.** 616/616 = 100% recall on operators the tool's own mutator produced (evaluation.tex:53–57). The number is per-check reliability, not detection power; a hostile reviewer will say "the exam was written by the student." *Fix:* name this in Threats; keep the honest caveat where it is; consider one sentence on why operators are still representative (they map 1:1 to the defect taxonomy of Fig. 1).
- **W2 (Major) — SC3001 inference at 1/7 precision** poisons the adoption-mode pitch for the highest-stakes check (double charge). *Fix:* either demote inferred SC3001 below warning by default, or add a sentence acknowledging inference is currently unfit for idempotency and pointing at Flux (already cited) as the premise supplier.
- **W3 (Minor) — Missing denominators/protocols.** How were gold-covered findings sampled (37/117 SC4001)? What defect classes were the 32 fix-commit pairs with no pre-fix finding — out-of-scope or missed? What was the 7th adjudicated-out candidate? One sentence each.
- **W4 (Minor) — Wild FP evidence for SC1101 is weak by construction:** it fires 0 times on curated samples, so the zero-FP claim rests on the 84 injected cases plus the theorem. Fine — but say precisely that.

**Questions:** (1) What fraction of corpus JSONPath reads fall inside the modeled dotted fragment vs. conservative fallback? (2) Sampling protocol for gold coverage? (3) Were baselines given credit for *any* diagnostic on semantic mutants, or only fault-naming ones (Table 2 note says fault-naming — confirm symmetric criterion applied to StepCheck)?

### R2 — Cloud/Serverless & SOC Domain
**Recommendation:** Accept with minor revisions · **Confidence:** 5

This is the paper ICSOC exists for: it takes the data-flow-anomaly and compensation literature the community built for BPEL/BPMN and lands it on the artifact people actually deploy in 2026, with an adoption gradient (raw ASL → `--infer` → sidecar → DSL) that respects how platform teams work. The 33 unbounded callbacks in official AWS samples (evaluation.tex:28–31) and the six fix-commit rediscoveries are the kind of evidence practitioners cite.

- **W1 (Major) — No declared-tier evidence in the wild.** SC4011 is silent on all 193 wild workflows because nobody declares compensators (evaluation.tex:43–45); the declared tier is exercised only on two self-authored sagas. The strongest cheap fix: hand-annotate ~10 wild workflows (the paper already has the gold labels!) and report declared-tier findings — that would close the loop between the annotation story and reality.
- **W2 (Minor) — Missing lineage citations:** Sadiq et al. 2004 (workflow data-flow validation), Ouyang et al. (WS-BPEL Petri-net analysis) — the paper cites Trčka 2009 and Sun 2006 but skips the anchors an SOC reviewer expects.
- **W3 (Minor) — Vendor scope:** one future-work sentence on Azure Durable / GCP Workflows front ends would pre-empt the "is this an AWS ad?" grumble; CNCF already does most of this work.
- **W4 (Minor) — Annotation cost realism:** 69 lines of TOML for a 4-task saga is honest; add the marginal-cost framing (≈8 lines/task) more prominently since it's actually favorable.

**Questions:** (1) Could the sidecar be generated from CDK/SAM/Terraform (the paper hints at IaC generation — is there a prototype)? (2) For the 89%-precise SC4001, what did the 4 false positives look like?

### R3 — Formal Methods / Static Analysis
**Recommendation:** Minor Revision (borderline → weak accept) · **Confidence:** 4

The analysis design is correct and correctly described: a may-present shape lattice where alarms require *definite absence* gives sound bug reports by construction; the non-convergence policy (suppress the machine's reports rather than emit from a partial fixpoint, approach.tex:106–110) is the right call and rarely stated this honestly. The theorems are modest and their scoping is exemplary. My concerns are about depth and residual ambiguity, not correctness:

- **W1 (Major) — Thm 2 is near-definitional.** Under a model restricted to a single post-commit fault, a unique generic catcher, and linear handlers, "sound and complete relative to the declared model" is close to restating the algorithm. The value is honesty, but the paper must say what happens *outside* the model on real topologies (branching recovery, failing compensators): silent? degraded to SC4001/4010? This determines whether SC4011 is usable in practice.
- **W2 (Major) — Prop. 1 / SC1010 pass-through ambiguity.** `in(T2) ⊆ out(T1)` (approach.tex:163–164) false-positives on document-carried fields unless `out(T)` is cumulative post-state shape. Table 1's gloss ("its **immediate predecessor** does not produce", approach.tex:65–66) suggests per-task, which would contradict the ASL ResultPath-merge semantics §3.2 models so carefully. Clarify in one sentence; if per-task, justify.
- **W3 (Minor) — Novelty from an FM standpoint is thin:** dotted-field record shapes with ⊤ is a small abstract domain; the contribution is the faithful transfer functions for ASL's five-stage I/O pipeline and the polarity discipline, not the domain. Fine for ICSOC; a PL venue would want the JSONPath fragment grown (indices, wildcards).
- **W4 (Minor) — Proof presence:** Thm 1's in-paper justification is one sentence. Add the two-line invariant ("every transfer over-approximates the concrete document's key set; definite absence at a fixpoint is therefore absence in all executions") so the paper is checkable without the TR.

**Questions:** (1) SC1010 `out` semantics (above). (2) On non-convergence, are *all* SC1101/SC1110 for the machine suppressed including sub-machines that individually converged? (3) Fragment coverage statistic (shared with R1).

### Devil's Advocate — Systems/Industry Skeptic
**Verdict:** No CRITICAL findings. Two MAJOR challenges, both answerable.

**Strongest counter-argument (the review to pre-empt):** *The sound core and the motivating bugs live in different tiers.* The theorem-backed, zero-FP analysis targets missing JSON fields — the defect class with the **lowest** production severity, since the ASL runtime fails the reference immediately and deterministically, so the first integration test usually catches it. The defects that justify the paper's opening (double charges, shipped-but-unpaid orders, leaked reservations) are exactly where the evidence is weakest: SC4011 fires on zero wild workflows because its premises are never declared, and idempotency inference is 1/7 precise. So a skeptic reads: "sound where it matters least, heuristic where it matters most."
*Available rebuttal (put it in the paper):* two of the six real rediscovered bugs are SC1101 and survive in merged repos — i.e., missing-field bugs demonstrably escape testing when they hide behind Choice branches; and the layered design is precisely the mechanism for moving high-stakes checks from heuristic to declared over time, with Flux-style tools as premise suppliers. Say this in one paragraph (intro or discussion) rather than letting the reviewer assemble it.

**MAJOR-2 — Framing asymmetry.** Baseline-alone findings are dismissed as "mostly schema nits" (evaluation.tex:64) while StepCheck's own 99/193 flag rate — mostly inferred-tier warnings at 80% precision — is framed as "adoption targets" (evaluation.tex:35). Symmetric language, or a reviewer does it for you.

**MINOR:** microsecond-level perf numbers oversell (CI cares about ms, not µs); 100%-recall headline in the abstract invites the circularity attack — consider "detects all 616 injected mutants **of the six targeted classes**" (abstract already close).

**Checks passed:** no cherry-picking found (negative results are reported: SC3001 1/7, 42/126 undetected injections, 32/39 silent fix-pairs, SC1101 zero wild fires); logic chain intact; "so what?" passes on real-bug + callback evidence; no overgeneralization — hedging discipline is the paper's best feature.

---

## Phase 2 — Editorial Synthesis

### Reviewer summary

| Reviewer | Recommendation | Confidence |
|---|---|---|
| EiC (PC chair) | Minor Revision | 4 |
| R1 (SE/empirical) | Minor Revision | 5 |
| R2 (cloud/SOC) | Accept w/ minors | 5 |
| R3 (formal methods) | Minor Revision | 4 |
| DA | No CRITICAL; 2 MAJOR challenges | — |

### Consensus
1. **[ALL] Claim calibration is exemplary** — every soundness claim is scoped, negative results are disclosed; this is the paper's signature strength.
2. **[ALL] Evaluation breadth is above the ICSOC bar** — six evidence types triangulate.
3. **[EiC+R1+R3] Post-trim prose density hurts** — approach.tex:102–111 and background.tex:41–45 are the worst offenders; SC-code soup taxes generalists.
4. **[R1+R2+DA] The declared tier lacks wild evidence** — annotate ~10 wild workflows or explicitly own the gap.
5. **[R1+R3] Fragment-coverage statistic missing** — % of corpus reads in the modeled dotted fragment.

### Disagreements
- **Novelty:** R3 calls the abstract domain thin; R2 calls the artifact-level reframe exactly the contribution ICSOC wants. *Resolution:* R2 prevails for this venue — novelty is architectural + empirical, and the paper never claims domain-theoretic novelty. No action beyond keeping the title honest.
- **Mutation study value:** DA calls 100% recall near-meaningless; R1 calls it necessary per-check reliability if labeled as such. *Resolution:* keep, but move the circularity caveat into Threats and soften the abstract phrasing.

### Scores on the requested dimensions

| Dimension | Score | Descriptor |
|---|---|---|
| Technical quality | 76/100 | Strong — correct analyses, honest polarity; Thm 2 near-definitional, SC1010 ambiguity |
| Novelty | 68/100 | Adequate–Strong — architectural/empirical, right for ICSOC; thin for PL |
| Evaluation | 75/100 | Strong — breadth exceptional; mutant circularity + missing declared-tier wild evidence |
| Readability | 62/100 | Adequate — post-trim compression, SC-code density |
| Structure | 74/100 | Strong — standard and effective; Threats mis-nested |
| Reviewer confidence (expected) | 4–5 | Paper is precise enough for reviewers to verify claims — cuts both ways |
| **Overall ICSOC readiness** | **73/100** | **Submit after minor revision (~1–2 days)** — predicted PC band: weak accept → accept |

### Decision: **Minor Revision — then submit**

Rationale: no reviewer found an invalidating flaw; the DA's strongest attack is a framing risk with a rebuttal already latent in the paper's own data. Consensus items 3–5 and the two MAJOR technical clarifications (Thm 2 out-of-model behavior; SC1010 `out` semantics) are all prose-level fixes. The remaining Major (declared-tier wild evidence) is the only item requiring new experiment work, and it is optional-but-high-leverage given gold labels already exist.

### Revision Roadmap

**P1 — Must fix before submission (~1 day, prose only)**
- [ ] R1: Add one paragraph pre-empting the DA's "sound-where-it-matters-least" reading — SC1101 real bugs escaped testing; layered design + Flux-style premise suppliers move high-stakes checks toward declared tier. (Intro §1 or Eval discussion)
- [ ] R2: Clarify SC1010/Prop. 1 `out(T)` semantics — cumulative document shape vs. per-task production. (approach.tex:160–167 + Table 1 gloss)
- [ ] R3: State SC4011's behavior outside the declared Saga model (branching handlers, failing compensators). (approach.tex:184–191)
- [ ] R4: Promote Threats to Validity to a `\subsection`, out of §5.7. (evaluation.tex:171)
- [ ] R5: Move mutant-circularity caveat into Threats; symmetric framing for baseline findings ("schema nits") vs. own warnings ("adoption targets"). (evaluation.tex:57, 64, 35)
- [ ] R6: Split the two densest sentences/paragraphs: approach.tex:102–111 (termination vs. policy) and background.tex:41–45 (four facts).

**P2 — High-leverage if time permits (~0.5–2 days)**
- [ ] S1: Report modeled-fragment coverage: % of corpus JSONPath reads that are dotted-only. (Likely one script over the corpus; both R1 and R3 asked.)
- [ ] S2: Hand-annotate ~10 wild workflows using existing gold labels; report declared-tier results. (Closes the biggest evidentiary gap; R2-W1.)
- [ ] S3: One sentence each: gold-coverage sampling protocol; classes of the 32 silent fix-pairs; the 7th adjudicated-out candidate. (eval §5.2/§5.5)
- [ ] S4: Add Sadiq et al. 2004 + one WS-BPEL verification anchor (Ouyang et al.) to Related. (related.tex:28–35)
- [ ] S5: Two-line proof invariant for Thm 1 in-paper. (approach.tex:125–127)

**P3 — Polish (~2 h)**
- [ ] Check Table 2 `\resizebox` font size vs. LNCS minimum; consider dropping a column instead.
- [ ] Abstract: "tens of microseconds" → corpus-level ms figure; "detects all 616 injected mutants" → add "targeted".
- [ ] Soften "not a bag of independent lints" (intro.tex:35–36) into a positive statement.
- [ ] Smooth the Fig. 1 caption's marker-(2) disclaimer.

---

*Report generated by the academic-paper-reviewer skill (full mode, 5-reviewer panel). Reviewers reviewed independently; synthesis traces to specific reports. No CRITICAL Devil's-Advocate findings → Minor Revision decision permitted.*
