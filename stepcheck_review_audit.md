# Reviewer-Style Audit — "Static Verification of AWS Step Functions with Sound Data-Flow Analysis and Workflow Semantics" (StepCheck)

Target venue: ICSOC 2026 (A-ranked, practice/engineering-friendly). Page-limit issues deliberately out of scope per author instruction.

**Verdict preview:** This is a strong, unusually honest engineering paper with one likely factual error about ASL semantics (§3.3 Choice-on-absent-field), one over-claimed adversarial benchmark (§5.7), a headline metric (100% recall) that is true-by-construction and must be framed even more defensively than it already is, and a small number of presentation/consistency nits. All numbers I could cross-check internally are arithmetically consistent (a rarity — see §5 audit). With the fixes below, this reads as an accept-range ICSOC paper.

---

## 1. Abstract & Keywords

| Category | Assessment |
|---|---|
| **Review summary** | Positions StepCheck as a static verifier for deployed ASL: a sound (no-false-positive) data-flow core plus seven layered checks, evaluated on 193 AWS + 66 CNCF workflows, 616 mutants, 6 real bugs. |
| **Technical correctness** | Careful: "sound in the bug-finding sense—no false positives under the modeled JSONPath semantics—rather than the verification sense" is exactly the right disclaimer, placed early. "Detects all 616 injected defects—100% recall on the targeted mutation operators, each constructed to fall within its check's remit" is honest but the parenthetical caveat is doing enormous load-bearing work in a sentence most readers will skim. "Tens of microseconds per workflow" is supported (§5.7). |
| **Novelty** | The unifying thesis ("checks the deployed artifact under layered semantics, grading severity by provenance") is stated and is the right defense against the "bag of lints" criticism — good that it appears already in the abstract. |
| **Clarity** | Dense but well-ordered: problem → tool → guarantee → evaluation. The soundness dual-definition sentence is long (3 clauses, 2 em-dash asides). |
| **Readability** | "sound in the bug-finding sense—no false positives…—rather than the verification sense of no false negatives" — consider splitting into two sentences. "relatively sound and complete against the declared model" appears before the reader can know what "declared" means. |
| **Reviewer concerns** | (1) A skimming reviewer reads "100% recall" and writes "recall on self-designed mutants is circular" before reaching the caveat. (2) "Sound" in the title-adjacent position ("Sound Data-Flow Analysis") will draw formal-methods fire; the abstract's disclaimer mitigates but the title itself says "Sound" unqualified. |
| **Suggested revisions** | (a) Consider titling the property "true-positives-only" or "precise" in the title, or keep "Sound" but add a footnote-level anchor to the bug-finding sense on first page. (b) Move "each constructed to fall within its check's remit" from parenthetical to its own sentence: "This measures per-check reliability, not coverage of arbitrary bugs." (c) Consider dropping one of "Static verification / Static analysis / Workflow verification" from keywords — redundant triple. |
| **Score** | Correctness **8** · Clarity **7** · Novelty **8** · Readability **7** · Reviewer readiness **8** |

---

## 2. Section 1 — Introduction

| Category | Assessment |
|---|---|
| **Review summary** | Motivates via Figure 1 (six latent defects in one order workflow), establishes that existing validators are schema-only, that the platform never rolls back committed effects, positions durable-execution systems as complementary, and states four contributions. |
| **Technical correctness** | The claim that statelint / asl-validator / ValidateStateMachineDefinition check only schema/structure is correct and later substantiated empirically (§5.3) — good hygiene. The "retries or routes the error… but never rolls back effects" claim about the platform is accurate. One presentation issue: Figure 1's caption says the workflow *has* six latent defects, but defect (2) (typestate) is described as "*reordering* ShipOrder before ChargeCard" — i.e., a hypothetical edit, not a defect present in the drawn workflow. Either the figure should show the reordered variant, or the caption should say "six defect classes, one hypothetical (2)". A careful reviewer will notice. |
| **Novelty** | Well-scoped: the "service-oriented computing writ small" framing is a smart, explicit ICSOC-fit move. Contributions are concrete and falsifiable. Complementarity with durable execution is stated twice (here and §6) — good, defuses the obvious "why not Temporal?" review. |
| **Clarity** | Excellent problem→gap→challenge→solution arc. The sentence defining the "central research challenge" (recover facts, no annotations, no runtime, clear soundness boundary) is the best sentence in the section. |
| **Readability** | Some sentences run 40+ words with double em-dash asides (house style throughout the paper). The phrase "reads as one verifier rather than a bag of lints" is memorable — keep it. Minor: "Prior work leaves this setting open (Section 6)" — forward-referencing related work in the intro is fine, but the durable-execution paragraph partially duplicates §6 verbatim in structure. |
| **Reviewer concerns** | (1) "We claim nothing about their prevalence" appears only in §2.2 — a reviewer reading the intro alone may object "are these defects actually common?"; consider one sentence of hedging in the intro. (2) Contribution 1 says "no false positives under the modeled JSONPath semantics" — the gap between *modeled* and *actual* AWS JSONPath is the real soundness exposure and is only validated empirically; consider acknowledging in one clause. |
| **Suggested revisions** | (a) Fix the Figure 1 defect-(2) framing. (b) Add a half-sentence on prevalence hedging. (c) Deduplicate the durable-execution positioning between §1 and §6 (keep the fuller version in §6). |
| **Score** | Correctness **9** · Clarity **8** · Novelty **8** · Readability **8** · Reviewer readiness **8** |

---

## 3. Section 2 — Background and Problem Setting

| Category | Assessment |
|---|---|
| **Review summary** | Fixes the ASL data plane (five-stage I/O pipeline), explains mechanistically why the six defect classes are latent, distils four design requirements (R1–R4) and two organizing axes (native/semantic, declared/inferred). |
| **Technical correctness** | The five-stage pipeline description (InputPath → Parameters → task+ResultSelector → ResultPath → OutputPath) is accurate, including the subtle and correct observation that ResultPath merges into the *pre-InputPath* input — wait, verify this: in ASL, ResultPath merges the result into the state's *raw input* (the input as received, i.e., pre-InputPath). ✔ Correct as written, and it matters for the transfer functions. Copy semantics for Parallel/Map ("each receive their own copy of the document, so the document itself cannot race") is accurate. Restriction to JSONPath mode with JSONata treated opaquely is clearly scoped, with a date anchor (Nov 2024). One-year platform maximum for callbacks: accurate. Table 1 is a genuinely good artifact — the fact/exposure/source triage is the paper's conceptual backbone. |
| **Novelty** | R1–R4 is a clean requirements derivation; the "recovering semantics is necessary rather than optional" dichotomy argument is tight. |
| **Clarity** | Very good. The canonical order example with protocol δ introduced here and reused everywhere is exemplary shared vocabulary. Listing 1.1 grounds the abstraction well. |
| **Readability** | The concurrency paragraph in §2.2 is one long block covering copies, shared keys, static resolvability, and write classes — split into two paragraphs (channel model vs. write-footprint classes). "Several of these are recognised, under-tooled failure modes" — cite-check [23,25] proximity: [23] is a FaaS characterization, [25] a microservice-recovery review; fine but loose. |
| **Reviewer concerns** | (1) The four omitted facts (contract, typestate, idempotency, persistence) are asserted as *the* missing semantics — a reviewer could ask about others (data privacy flows, cost, quotas); a sentence scoping "the four our defect classes need" would pre-empt. (2) JSONata is the AWS-recommended mode for *new* machines since late 2024; by 2026, "rare in our corpus" (§5.9) may reflect corpus age, not current practice. This belongs in threats but a reviewer may raise it here. |
| **Suggested revisions** | (a) Scope the "fourfold" claim to the targeted defect classes. (b) Split the concurrency paragraph. (c) Consider a one-line note that the corpus predates JSONata's rise (forward-ref to §5.9). |
| **Score** | Correctness **9** · Clarity **9** · Novelty **7** · Readability **8** · Reviewer readiness **8** |

---

## 4. Section 3 — Verification Framework

| Category | Assessment |
|---|---|
| **Review summary** | The technical core: architecture and IR (§3.1–3.2), sound data-flow analysis with Theorem 1 (§3.3), conservative concurrency/temporal passes (§3.4), declared semantic checks with Theorems 2–3 (§3.5), and annotation/inference machinery (§3.6). |
| **Technical correctness** | **[MAJOR — verify]** §3.3 states: "a Choice comparison returns false on an absent field rather than failing the execution, so flagging it as an error would be unsound." To the best of my knowledge this is **wrong for AWS Step Functions**: in JSONPath mode, a Choice rule whose `Variable` path does not resolve fails the execution with a `States.Runtime` error — this is precisely why AWS added the `IsPresent` operator (2020). If so: (i) excluding Choice operands from SC1101 is over-conservative (a missed *error* class), not required for soundness, so Theorem 1 survives; but (ii) the stated justification is incorrect, and (iii) SC1110's "dead branch, warning" framing inverts — the branch is not merely dead, the execution aborts, so it could be a sound *error*. Verify against the current ASL spec and either fix the claim or cite the exact spec passage if AWS semantics changed. This is exactly the kind of detail a formal-methods reviewer checks. **[Minor]** Table 4: `Parameters "k.$": p` binds k ↦ ⊤ rather than σ↓p — sound but throws away precision the analysis already has; either justify (aliasing/complexity) or note as deliberate. **[Minor]** Table 4 does not state what σ[r ↦ result] means when σ = ⊤ (presumably stays ⊤) — one cell-note fixes it. **[Minor]** Catch-edge transfer joins {Error, Cause ↦ ⊤} at a fixed location, but a Catch's own ResultPath controls where the error object lands; the simplification is sound (over-approx) but should be stated. **[Good]** The convergence-gating argument (silence, never a partial report, on non-convergence) is a genuinely careful soundness detail most bug-finding papers omit. **[Good]** Theorem 3's A1–A4 assumption box is model transparency done right; A4 (Parallel/Map as one fallible unit) is coarse — a per-branch Catch inside a Parallel is not modeled — expect a question. SC3001's "narrow transient errors fire pre-effect is a convention we assume, not prove" is honestly flagged. |
| **Novelty** | The data-flow provenance analysis over concrete nested JSON shapes through the ASL pipeline is the paper's real novelty and is convincingly differentiated from the BPMN/BPEL anomaly line (opaque named elements, hand-built nets). The provenance-graded severity model is a design contribution in its own right. Typestate/Saga checks are adaptations of known theory — correctly presented as such. |
| **Clarity** | §3.3 is excellent: the worked restock-loop example showing *why* a fixpoint (not a single pass) is needed, and why the back-edge join prevents a false positive, is textbook-quality exposition. Table 2 + Table 3 (pass families + guarantee boundary) are the two tables reviewers will screenshot. Notation is consistent (δ, T: A→B, s_in→s_out reused from §2.2). |
| **Readability** | §3.5 compresses a type system into one paragraph plus two theorems; the inference rules living only in the TR is acceptable for ICSOC but the sentence "The full well-typedness judgment Γ ⊢ W : A ⇒ B and its three inference rules are in the technical report; here we use only what the checks decide" should come *first* in the subsection, not mid-way. Minor typography: SC5002's "MaxConcurrency = 1̸" renders badly (should be ≠ 1); the SC6002 geometric sum "P k interval·backoff k" has broken math rendering in the extracted text — check the camera-ready PDF. |
| **Reviewer concerns** | (1) The Choice-semantics issue above. (2) Which JSONPath fragment exactly is modeled — filters `[?()]`, slices, wildcards, `..`? Table 4 covers records + element-shape arrays; a formal reviewer will ask where filter expressions go (presumably ⊤/opaque — say so). (3) Theorem 2's "precise" claim rests on edge-local checks being equivalent to a global typing derivation; the Choice branch-agreement rule is the non-obvious case and only the TR carries it. (4) Inference keyword sets (§3.6): four small keyword lists is a thin mechanism for a paper section — but §5.6's honest accuracy measurement rescues it. |
| **Suggested revisions** | (a) Fix or precisely source the Choice-on-absent-field claim; if AWS aborts, promote SC1110's operand case to a sound error and gain a stronger result for free. (b) Add one sentence enumerating the modeled JSONPath fragment and stating that everything else lifts to ⊤. (c) Note the ⊤-merge and Catch-ResultPath simplifications in Table 4. (d) Fix the ≠ and Σ typography. (e) Consider citing Livshits et al., "In Defense of Soundiness" (CACM 2015) when defining the bug-finding sense of soundness — it gives reviewers a familiar anchor. |
| **Score** | Correctness **7** (pending Choice fix → 9) · Clarity **8** · Novelty **8** · Readability **7** · Reviewer readiness **7** |

---

## 5. Section 4 — Implementation

| Category | Assessment |
|---|---|
| **Review summary** | 4,394-line Rust CLI; two traits realize the two architectural axes; three frontends; CLI/CI usage with exit-code policy; execution oracle and mutation engine as self-validation; Table 5 feature-coverage matrix. |
| **Technical correctness** | Internally consistent. The claim "the check reads the ingested Asl and never re-emits it, so raw-Asl verification is entirely independent of the emitter" is important and correctly firewalls the lossy-emitter limitation from the verification claims. The disclosed emitter gap (I/O-processing fields, machine-level TimeoutSeconds) is candid; the Succeed/Fail `"End"` anecdote is a nice credibility touch. Rust 1.96 / serde details fine. |
| **Novelty** | None claimed; appropriately an engineering section. The code-disjoint execution oracle as a *soundness witness* is the one idea here reviewers will like — consider promoting it to a named mini-contribution. |
| **Clarity** | Good. Listing 1.4 (two traits) is exactly the right amount of code. The exit-code / `--deny-warnings` adoption policy is precisely what a software-engineering reviewer wants to see and is often missing from academic tools. |
| **Readability** | The "Running the verifier" paragraph mixes CLI mechanics, CI policy, and emitter independence — three paragraphs would breathe better. En-dash vs double-hyphen in CLI flags (`–infer`, `–annot`, `–json`) appears to be a PDF ligature artifact — verify flags render as `--infer` etc. in camera-ready. |
| **Reviewer concerns** | (1) 4,394 LOC for eight analyses + three frontends + emitter + oracle + mutation engine will strike some reviewers as small — pre-empt by noting Rust density or per-module counts (the TR reference covers it, but one number in-paper helps). (2) "planned npx wrapper" — reviewers discount planned features; keep but de-emphasize. |
| **Suggested revisions** | (a) Split the CLI paragraph. (b) Fix `--flag` rendering. (c) One clause on why the oracle being code-disjoint matters (shared bugs are the classic failure of self-validation). |
| **Score** | Correctness **9** · Clarity **8** · Novelty **6** · Readability **8** · Reviewer readiness **8** |

---

## 6. Section 5 — Evaluation

| Category | Assessment |
|---|---|
| **Review summary** | Three RQs (detection, semantic recovery, cost); wild-corpus findings, 616-mutant study vs. a three-validator panel, data-flow soundness/power/reach, 6 mined real bugs, inference accuracy vs. a doubly-annotated gold set, cost/scalability, CNCF generalization, threats. |
| **Technical correctness** | **Arithmetic audit (all pass):** corpus state counts sum to 1,354 ✔; Fig 5b partition 63+36+55+39 = 193 ✔ and 63+36 = 99 flagged ✔; Table 6 per-class and totals (166+141+103+19+21+166 = 616; statelint 306 = 50%, asl-val 288 = 47%, AWS 302 = 49%) ✔; §5.4 84/126 = 67% ✔; Table 8 (77/110 = 70.0%, 96/110 = 87.3%, 110/137 = 80.3%) ✔; warning precision 18/30 = 60%, 16/23 = 70%, 2/7 = 29% ✔; 193 × 39 µs ≈ 7.5 ms ✔. **[Issue 1]** §5.4: the applicable-population **126** for the injection study is never explained (why not 193?) — presumably injection preconditions; one sentence needed. **[Issue 2 — over-claim]** §5.7 calls the synthetic chain "adversarial for the O(|S|·|K|) bound… since every state adds a distinct field and so maximizes |K|." But an *acyclic chain* converges in one topological pass regardless of |K| — the round count that O(|S|·|K|) bounds is driven by *back-edges*. The synthetic maximizes shape width, not fixpoint iterations. Either add a looped synthetic (a chain of nested loops) or soften to "adversarial for shape width." A formal reviewer will catch this. **[Issue 3]** κ = 1.00 on 116 doubly-labelled tasks will raise eyebrows; the abstention-drains-ambiguity explanation is plausible and stated, but consider reporting raw agreement counts and the abstention overlap between annotators to make it concrete. **[Good]** The 100%-recall construct-validity discussion (§5.9) and the refusal to credit incidental schema nits to baselines (§5.3) are unusually scrupulous. The near-diagonal confusion matrix is the right specificity evidence. Reporting SC3001 precision at 29% rather than hiding it is the paper's single most credibility-building number. |
| **Novelty** | The evaluation *design* (separating detection power from in-the-wild evidence; independent oracle; mined fix-commit replay) is itself above venue norms. |
| **Clarity** | Well-signposted by RQ. §5.2's error/warning separation mirrors the severity model — good structural echo. Table 7 (six real bugs) is compact and convincing. |
| **Readability** | §5.6 is the densest subsection (gold-set construction + accuracy + warning precision in three paragraphs); a small table for the warning-precision numbers (SC4001 16/23, SC3001 2/7, SC4010 6/6) would help — they're currently prose-only. "The asymmetry is the headline of this experiment, and it is honest" — the self-praising register ("honest", "we measure rather than hide", repeated ~4×) should be trimmed to once; reviewers prefer to conclude honesty themselves. |
| **Reviewer concerns** | (1) Mutation circularity (anticipated, but expect the question anyway). (2) Corpus is didactic AWS samples (median 6 states) — acknowledged; the strongest counter is the six real bugs + 30k-state synthetic; consider one sentence on why enterprise ASL is inaccessible (proprietary). (3) 117 SC4001 warnings across 193 workflows ≈ heavy triage load on adoption; §5.2 should state the recommended posture explicitly (warnings non-blocking by default) — it does in §4, cross-reference it. (4) The one-workflow AWS deployment (§5.7) is anecdotal — fine, but call it a smoke test. (5) 42 undetected injections in §5.4 attributed to ⊤-collapse — good analysis; a reviewer may ask what fraction of *corpus* field reads sit downstream of opaque results (the "two-thirds recoverable" claim generalizes from the injection sample). |
| **Suggested revisions** | (a) Explain the 126. (b) Fix the "adversarial" scalability claim or add a loop-heavy synthetic. (c) Add the warning-precision mini-table. (d) Trim the honesty self-references. (e) Label the AWS deployment a smoke test. |
| **Score** | Correctness **8** · Clarity **8** · Novelty **8** · Readability **7** · Reviewer readiness **8** |

---

## 7. Section 6 — Related Work

| Category | Assessment |
|---|---|
| **Review summary** | Four threads: durable execution (complementary), formal verification at other layers, behavioural types + Sagas, and the data-flow-anomaly line; plus adjacent threads (shape inference, contract checking, idempotency verification, commuting effects). Table 9 positions StepCheck. |
| **Technical correctness** | Characterizations of Temporal/Cadence/Beldi/Durable Functions, TLA+-at-Amazon, and the BPMN/BPEL anomaly line are accurate. Table 9's "Sound: native core" cell is appropriately scoped. |
| **Novelty (of positioning)** | The complementarity framing (static obligations before deploy vs. runtime enforcement) is the correct and defensible position. |
| **Clarity** | Well-organized by thread; Table 9 is effective. |
| **Readability** | Good; the densest citation clusters ([33,35,26,31]) could each get a half-clause of differentiation, but acceptable. |
| **Reviewer concerns** | **[Gap]** No engagement with formal semantics of serverless *platforms/languages* — most obviously Jangda et al., "Formal Foundations of Serverless Computing" (OOPSLA 2019); also check for any published formal ASL semantics (there has been academic interest in mechanizing ASL — a quick search before submission is cheap insurance, since a reviewer citing a missed ASL-semantics paper is the most damaging possible related-work miss for a paper whose Theorem 1 says "under the modeled JSONPath semantics"). Data-passing/orchestration optimization work (e.g., SONIC) is safely out of scope. (2) IaC-analysis paragraph cites [12] (a knowledge-base paper) for "automated reasoning over cloud configuration" — Amazon's Zelkova/Tiros line would be the more recognizable citation. |
| **Suggested revisions** | (a) Add Jangda et al. and search for prior ASL formalizations; if one exists, position the modeled semantics against it explicitly. (b) Reconsider the [12] citation. |
| **Score** | Correctness **8** · Clarity **8** · Novelty **n/a** · Readability **8** · Reviewer readiness **7** |

---

## 8. Section 7 + Artifact — Conclusion & Availability

| Category | Assessment |
|---|---|
| **Review summary** | Restates thesis, results, and the type-systems analogy; artifact section details contents, reproduction requirements (no AWS account needed), and distribution plans. |
| **Technical correctness** | Consistent with the body. The "no AWS account required to reproduce any figure" + committed recorded results for the server-side validator is excellent artifact-evaluation hygiene. |
| **Novelty** | The type-systems analogy ("moved whole classes of error from run time to compile time") is apt and a good closing register for ICSOC. |
| **Clarity/Readability** | Tight. Future work (effect/ownership info, element-level Map shapes, SAM/CDK frontends) is concrete rather than boilerplate. |
| **Reviewer concerns** | None significant. Artifact anonymization link present ✔; double-blind hygiene throughout appears intact (Anonymous authors, anonymized repo, "our corpus" without identifying repos beyond public AWS ones). |
| **Suggested revisions** | If ICSOC 2026 has an artifact badge track, say one sentence mapping the artifact to the badge criteria. |
| **Score** | Correctness **9** · Clarity **9** · Novelty **7** · Readability **9** · Reviewer readiness **9** |

---

# Four-Reviewer Simulation

## Reviewer A — Formal Methods

**Overall stance: weak accept → accept after rebuttal, if the semantics questions are answered crisply.**

Likely questions:
1. **Theorem rigor.** All three theorems are stated with assumptions and proof sketches, full proofs in a TR. Acceptable for ICSOC, but Theorem 2's "precise" claim depends on edge-local checks being equivalent to a global typing derivation — the Choice branch-agreement case is the non-trivial step and lives only in the TR. Expect: *"Sketch the Choice case in the paper."*
2. **Soundness assumptions.** The guarantee is "under the modeled JSONPath semantics." What exactly is modeled? Filters, slices, wildcards, recursive descent, reference paths, intrinsic functions inside paths? The paper says intrinsics lift to ⊤ but never enumerates the path fragment. Expect: *"Give the grammar of the modeled fragment."* Also: the Choice-on-absent-field claim (§3.3) — if AWS actually raises `States.Runtime`, the justification for excluding Choice operands is wrong (see §4 audit above); this reviewer is the one who will check.
3. **Abstract interpretation.** The domain (finite-height lattice of nested-record may-shapes, key-union join, monotone transfers, ⊤-absorbing) is standard and correct as presented; the convergence-gated reporting is a nice touch they will explicitly praise. Question: why does `"k.$": p` bind k ↦ ⊤ instead of σ↓p — deliberate precision loss or an implementation shortcut?
4. **Lattice correctness.** Height bound |K| argued from the finite key universe ✔. But the scalability synthetic (§5.7) is acyclic and therefore does *not* stress fixpoint iteration despite being labeled "adversarial for the O(|S|·|K|) bound" — this reviewer will flag the mismatch. Also: is the join over Catch edges *before* or *after* the failing state's partial pipeline? (The error document ≠ the state's input document; the model appears to join at the handler's entry — state it.)

## Reviewer B — Software Engineering

**Overall stance: accept-leaning; this is the audience the paper serves best.**

Likely questions:
1. **Practicality.** Zero-annotation baseline + sound-native-errors-only CI gate + non-blocking inferred warnings is exactly the right adoption ladder, and the exit-code policy shows the authors have run this in anger. 117 SC4001 warnings on 193 sample workflows implies a real triage load on day one — the paper should state expected warnings-per-workflow for a typical repo and the recommended first-week posture in one place.
2. **Usability.** Diagnostics carry code/severity/state/fix-hint ✔, JSON output ✔. Missing: any developer-facing study or even anecdote (time-to-fix, comprehensibility of SC1101 messages). Not required at this venue, but expect *"any user feedback?"*
3. **Annotations.** 69 lines of TOML per saga (~8 lines/task) is a credible, quantified burden — good. Question: annotation *drift* — what happens when the workflow changes and the sidecar goes stale? Is there a staleness check (e.g., sidecar names a state that no longer exists → SC4002 covers compensators, but schemas/protocol edges?). Worth one sentence.
4. **CI integration.** `stepcheck check` gating like a linter, `--deny-warnings` ratchet, planned npx: convincing. Question: monorepo scale — checking hundreds of definitions per commit is trivially fast per the numbers; say so explicitly ("a 500-workflow repo verifies in ~20 ms").

## Reviewer C — Cloud Systems

**Overall stance: weak accept; realism largely convincing, corpus is the soft spot.**

Likely questions:
1. **AWS realism.** The ASL pipeline model, one-year callback cap, at-least-once retries, no platform rollback, copy semantics in Parallel/Map, resource keying by TableName/QueueUrl/TopicArn/Bucket — all accurate. Two realism gaps: (i) JSONata is AWS's recommended mode for new machines since Nov 2024, and the analysis is silent across it — by 2026 this is a shrinking-coverage concern, currently relegated to threats; (ii) distributed Map's ItemProcessor is "simplified (element shape)" — large-scale distributed Map is precisely where enterprises live.
2. **Workflow corpus.** 193 workflows from two AWS *sample* repositories, median 6 states, max 33 — didactic by construction, acknowledged. The mined-fix-commit study partially compensates (real, independently introduced defects). Expect: *"Any industrial corpus, even 10 workflows under NDA-level description?"*
3. **Scalability.** Sub-millisecond to 30k states is more than sufficient; no concern — beyond the "adversarial" mislabel (Reviewer A's point).
4. **Deployment.** One Express-machine smoke test proves emitted ASL runs unmodified; fine but thin. The emitter's lossy fields (InputPath/OutputPath/ResultSelector, machine-level timeout) are firewalled off the verification path — the paper is careful here and this reviewer should be satisfied, but may ask when full-fidelity re-emission lands since the DSL story depends on it for brownfield round-trips.

## Reviewer D — General Systems

**Overall stance: borderline → weak accept; novelty is real but incremental-looking if skimmed, significance argument rests on 6 bugs.**

Likely questions:
1. **Novelty.** Each ingredient exists elsewhere (abstract interpretation, typestate, Sagas, data-flow anomalies in BPMN). The genuinely new artifact is (i) the shape analysis over concrete nested JSON through ASL's five-stage pipeline with a no-false-positive theorem, and (ii) the provenance-graded severity architecture. The paper knows this and says it; the risk is a skimming reviewer sees "eight lints." Recommendation: the "one thesis" framing should appear in the intro's first paragraph after Figure 1, not only in the contributions.
2. **Comparison.** The three-validator panel including AWS's own server-side check is the right baseline and the 0-detection result on all four semantic classes is the paper's cleanest differentiator. Missing comparison: could an LLM-based or test-generation baseline catch these? (Unfair but increasingly asked in 2026 — one sentence in related work pre-empts.)
3. **Significance.** 6 confirmed real bugs from 39 mined pairs across 5 repos (one official AWS sample) is existence evidence, correctly not claimed as prevalence. The 33 in-the-wild SC6001 findings (unbounded callbacks on payment/approval steps) are arguably the strongest practical-significance datum in the paper — promote them.
4. **Impact.** Deploys-unchanged + zero runtime overhead + open source + package-manager distribution = plausible adoption path; the CNCF frontend shows the architecture generalizes. The type-systems analogy lands.

---

# Cross-Cutting Summary

**Must-fix before submission (ordered):**
1. **Verify the Choice-on-absent-field semantics claim (§3.3).** If ASL raises `States.Runtime` on unresolvable Choice paths (as `IsPresent`'s existence suggests), correct the text — and note you can then *strengthen* the tool: Choice operands become soundly flaggable errors.
2. **Fix the "adversarial" scalability framing (§5.7)** — an acyclic chain doesn't stress the fixpoint; add a loop-bearing synthetic or reword.
3. **Explain the 126 applicable workflows in §5.4.**
4. **Fix Figure 1's defect (2)** (hypothetical reorder presented as a present defect).
5. **Related-work insurance:** add Jangda et al. (OOPSLA'19) and search for any published formal ASL semantics.

**Should-fix:**
6. Enumerate the modeled JSONPath fragment in one sentence; note the ⊤-merge and Catch-ResultPath simplifications in Table 4.
7. Warning-precision mini-table in §5.6; trim repeated "we measure rather than hide" register to one occurrence.
8. Typography: ≠ in SC5002, the retry-budget Σ formula, `--flag` rendering.
9. Report raw agreement counts behind κ = 1.00.
10. Deduplicate the durable-execution positioning between §1 and §6.

**Overall scores (paper as submitted):**

| Dimension | Score /10 |
|---|---|
| Technical correctness | 8 (7.5 until the Choice claim is resolved) |
| Clarity | 8 |
| Novelty | 8 |
| Readability | 7.5 |
| Reviewer readiness | 8 |

**Predicted outcome at an A-ranked practice-oriented venue:** with must-fixes applied, likely scores around (A: weak accept/accept, B: accept, C: weak accept, D: weak accept) — accept-range, with the rebuttal load concentrated on Reviewer A's semantics questions and Reviewer C's corpus question. The paper's biggest asset in review is its candor discipline (29% SC3001 precision reported, construct-validity of the 100% recall scoped, emitter gaps disclosed); its biggest liability is any single AWS-semantics detail found wrong, because the whole soundness story asks the reader to trust the modeled semantics.
