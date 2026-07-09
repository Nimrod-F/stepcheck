# StepCheck — Trimming Plan: ~38 pp → 15 pp (including references)

Guiding principle: at a practical/engineering A-venue, the paper's identity is **tool + guarantees stated (not developed) + evaluation + adoption story**. Everything that *develops* formal machinery moves to the technical report (TR); everything that *states* a guarantee and its assumptions stays, in one or two sentences with a TR pointer. The evaluation is the last thing to shrink — it is what carries a practice-track paper.

A cut of this size (~60%) cannot come from sentence-tightening; it requires removing whole artifacts (2 figures, 4 tables, 4 listings) and rewriting §3 from a 12-page development into a 4-page systems description.

---

## 1. Page budget

Estimated current sizes (from the PDF) vs. target:

| Section | Now | Target | Δ |
|---|---|---|---|
| Abstract + front matter | 1.2 | 0.75 | −0.45 |
| §1 Introduction (Fig 1, Fig 2) | 2.3 | 1.5 | −0.8 |
| §2 Background (Table 1, Listing 1.1) | 4.0 | 1.5 | −2.5 |
| §3 Framework (Fig 3, Tables 2–4, 3 theorems, Listings 1.2–1.3) | 12.0 | 4.25 | −7.75 |
| §4 Implementation (Table 5, Listings 1.4–1.5) | 2.8 | 0.75 | −2.0 |
| §5 Evaluation (Figs 4–5, Tables 6–8) | 9.5 | 4.0 | −5.5 |
| §6 Related work (Table 9) | 2.5 | 1.0 | −1.5 |
| §7 Conclusion + Artifact | 2.0 | 0.75 | −1.25 |
| References (38 entries) | 3.0 | 2.0 | −1.0 |
| **Total** | **~39** | **~14.5** | (0.5 pp buffer) |

---

## 2. Artifact triage (figures / tables / listings)

| Artifact | Decision | Rationale |
|---|---|---|
| **Fig 1** (order workflow, 6 defects) | **KEEP, shrink to ~0.35 pp** | The motivating spine; every section references it. Tighten caption to one line per defect. |
| **Fig 2** (pipeline, 3 frontends) | **CUT** | Fully subsumed by Fig 3. Biggest free win in the paper. |
| **Fig 3** (verification pipeline) | **KEEP, simplify** | Remove the legend box, the execution-oracle and diagnostics-engine boxes (one sentence each in text), and per-pass SC-code labels (they're in Table 2). Target 0.4 pp. |
| **Table 1** (facts vs. exposure) | **KEEP, compress** | Conceptual backbone; merge to 5 rows (fold the two "omitted, semantic" contract/typestate rows; fold idempotency+persistence into one "external-effect facts" row). |
| **Table 2** (eight passes) | **KEEP, absorb Table 3** | Add one terse "assumptions" column; this becomes the single guarantee-boundary artifact in the paper. |
| **Table 3** (guarantee boundary) | **MOVE TO TR** | Its content survives as Table 2's new column + one paragraph. Reviewers still get the boundary; they lose only the elaboration. −0.8 pp. |
| **Table 4** (abstract domain) | **MOVE TO TR** | Replace with a 5–6 line prose description of the domain (⊤ / key-record, key-union join, finite key universe, per-stage monotone transfers) + Theorem 1 statement. −0.5 pp. |
| **Table 5** (ASL feature coverage) | **MOVE TO TR** | Keep a 3-sentence limitations paragraph: JSONata → ⊤; distributed Map simplified; emitter lossy on I/O-processing fields and never on the verification path. −0.8 pp. |
| **Table 6** (mutant detection vs. validators) | **KEEP, compress** | The paper's cleanest differentiator. Merge the three baseline columns' counts+percentages into "n (x%)" and drop the Appl. column into the caption. |
| **Table 7** (six real bugs) | **KEEP, compress** | Strongest practical evidence. Shorten defect descriptions to ≤ 8 words each. |
| **Table 8** (inference accuracy) | **CUT → one sentence** | Two numbers (70.0% idempotency, 87.3% persistence, 80.3% coverage) carry it in prose. Keep Fig 4c *or* the sentence, not both. |
| **Fig 4** (corpus overview, 3 panels) | **CUT panels (a),(b); keep (c) only or cut entirely** | Corpus stats become one sentence ("1,354 states, median 6, max 33; 166 Choice, 64 Map, 33 Parallel"). Panel (c) survives only if Table 8 is cut. −0.3 pp. |
| **Fig 5** (recall + wild coverage) | **KEEP** | Best evaluation figure; (a) and (b) together tell the whole §5.3 story and let you cut prose around them. |
| **Table 9** (positioning) | **CUT or shrink to 4 rows** | Nice-to-have; the prose already positions each thread. If kept: drop the "Artifact checked" column and the IaC row. −0.5–0.7 pp. |
| **Listing 1.1** (real ASL task) | **KEEP, trim to 4 lines** | The one piece of concrete ASL a reader needs; cut the Retry block. |
| **Listing 1.2** (sidecar TOML) | **KEEP, trim to 5–6 lines** | The annotation-burden story is central to the practical pitch. |
| **Listing 1.3** (typed DSL) | **CUT** | One sentence: "the typed DSL emits the same facts by construction (TR §x)." −0.35 pp. |
| **Listing 1.4** (two traits) | **CUT** | "Two traits, `Frontend` and `Pass`, realize the two extension points." −0.2 pp. |
| **Listing 1.5** (JSON diagnostic) | **CUT or keep 3 lines** | Engineering flavor; keep only if space allows after everything else. |

Net artifact savings: ≈ 4.5 pp.

---

## 3. Per-section plan

### Abstract (1.2 → 0.75)
- Rewrite to ~150 words: problem (2 sentences), StepCheck + layered-semantics thesis (2), soundness in the bug-finding sense (1), results (2: 193+66 workflows, validators detect none of the semantic classes, 6 real bugs, µs-scale), deploys-unchanged (1).
- Cut from the abstract: the completeness-vs-soundness elaboration, the 30,000-state scaling number, the per-check remit caveat (moves to §5.3 where it belongs — but per the audit, keep it *somewhere* prominent).
- Keywords: drop to 5.

### §1 Introduction (2.3 → 1.5)
- Keep: Fig 1 + the six-defect walk, the validators-check-only-structure paragraph, the "central research challenge" sentence, contributions.
- **Cut:** the durable-execution paragraph (one clause survives: "durable runtimes are complementary — §6"); Fig 2.
- **Compress:** contributions from 4 bullets with sub-clauses to 4 single-sentence bullets.
- Do NOT cut: the "one thesis / not a bag of lints" framing (per audit, move it *earlier*), the six-real-bugs teaser.

### §2 Background (4.0 → 1.5)
This is the second-biggest proportional cut. The section currently teaches ASL to a reader who, at ICSOC, mostly knows it.
- **§2.1:** compress the five-stage pipeline to one paragraph + trimmed Listing 1.1. Cut the JSONata/mode discussion to one sentence (details now with the TR-side Table 5). Cut the fault-handling-controls paragraph to two sentences (Retry classes; TimeoutSeconds/callback one-year cap).
- **§2.2:** keep the canonical order example + protocol δ (3 sentences — it's the shared vocabulary) and the fourfold omitted-facts list (compressed to one sentence per fact). **Cut** the per-defect-class mechanistic walkthrough (retries at-least-once, saga obligation, timing, concurrency) — each survives as one clause inside the corresponding §3 pass description, where it does double duty. −1.5 pp alone.
- **§2.3:** keep R1–R4 as a 4-line compact list; cut the two-axes elaboration paragraph (the axes are visible in Table 2 and Fig 3).
- Table 1 compressed as above.

### §3 Framework (12.0 → 4.25) — the main surgery
Reframe from "formal development with systems context" to "systems description with stated guarantees."

- **§3.1 Architecture (now ~2 pp → 0.75):** keep the design-rationale paragraph (4 decisions — it's dense and good), Fig 3 simplified, Table 2 with the absorbed guarantee column. Cut the front-end tour to 3 sentences (raw-ASL tolerance gets one; DSL one; CNCF one). Cut the diagnostics/emitter/oracle paragraph to 2 sentences.
- **§3.2 IR (0.5 → 0.2):** one paragraph. It's an enumeration; enumerate faster.
- **§3.3 Data-flow (3.5 → 1.5):** the crown jewel — cut carefully.
  - KEEP: the bug-finding-vs-verification soundness definition (2 sentences), the prose domain description replacing Table 4, **Theorem 1 statement**, the "sound but deliberately incomplete" trade (2 sentences), and — this is the one worked example the paper keeps — the restock-loop fixpoint example, compressed to ~6 sentences. It is the best didactic asset in the paper; a practice-track reviewer understands soundness *through* it.
  - MOVE TO TR: Table 4, the full transfer walkthrough, the convergence-gating proof mechanics (keep one sentence: "on non-convergence the pass is silent, never partial — TR"), the O(|S|·|K|) derivation (state the bound, derive in TR).
  - COMPRESS: the three legacy binding checks (SC1001–1003) to one sentence; the two-tier (native/typed) discussion to one short paragraph.
- **§3.4 Concurrency/temporal (1.3 → 0.6):** keep the shared-resource channel model and the overwriting-writes-only rule (this is the interesting design decision) + the Fig-1-defect-(4) tie-in in one sentence. Compress SC6001–6003 to three sentences.
- **§3.5 Semantic checks (3 → 1.0):**
  - KEEP: the contract/typestate obligations in prose (3 sentences), **Theorem 2 and Theorem 3 statements** (trimmed — drop the "equivalently…" restatements inside each), the A1–A4 assumptions compressed from a boxed half-page to one 3-line inline list.
  - MOVE TO TR: both proof-sketch paragraphs, the ship-before-pay worked example's error-by-error enumeration (keep: "reordering yields five errors — two SC2001, three SC1010 — and no false positives"), the SC4010/SC4011 blind-spot narrative (keep 2 sentences: per-task checks miss post-commit downstream failures; SC4011 closes it under declared facts).
- **§3.6 Annotations/inference (1.7 → 0.6):** keep trimmed Listing 1.2, the 69-lines-of-TOML burden sentence, and the inference design in 4 sentences (four keyword classes, abstention, explicit-wins). Cut Listing 1.3 and the keyword examples list.

### §4 Implementation (2.8 → 0.75)
Merge into a single unheaded run of 3 paragraphs (or make it §3.7):
1. 4,394-line Rust CLI, three frontends, one binary; raw-ASL frontend parsed all 193 corpus workflows.
2. CLI/CI story — **keep intact but tight**: `stepcheck check`, `--infer`, `--annot`, exit codes, `--deny-warnings` ratchet. This paragraph is the practice-track payload; it is the last thing to cut in §4.
3. Oracle + mutation engine in 2 sentences; emitter-independence firewall in 1 sentence; Table 5 → the 3-sentence limitations note.
Cut Listings 1.4, 1.5.

### §5 Evaluation (9.5 → 4.0)
Cut methodology narration, keep every result number a reviewer would ask for.
- **§5.1 (0.6 → 0.25):** corpus provenance in 2 sentences + the one-sentence stats (replacing Fig 4a/b).
- **§5.2 (1.3 → 0.6):** keep the native-error findings (SC0007/0010, 33× SC6001) and warning counts. **Compress the three-silences paragraph to one sentence** ("SC1101, SC1110, and SC5xxx fire on none of the 193 — precision results; their power is shown by mutation"). Compress the SC4011 chain-break experiment from a paragraph to 3 sentences — but keep it; it's the only demonstration of the paper's most novel check.
- **§5.3 (1.8 → 0.9):** keep Fig 5, Table 6, the detection-criterion sentence, and the "no notion of idempotency… cannot see them — not even AWS's own" punchline. Cut: the confusion-matrix paragraph to one sentence; the naive-count-metric parenthetical to a footnote or cut; the in-the-wild asymmetry paragraph to 3 sentences (Fig 5b carries it).
- **§5.4 (1.2 → 0.6):** keep precision/oracle/reach structure but one short paragraph each. The oracle description shrinks to 2 sentences. Keep the 84/126 = 67% + ⊤-opacity explanation (and add the missing one-line explanation of the 126 population — audit must-fix).
- **§5.5 (1.0 → 0.6):** keep compressed Table 7 + the 6-of-39 protocol in 3 sentences + the "existence evidence, not prevalence" caveat in one.
- **§5.6 (1.5 → 0.6):** gold-set construction compressed to 3 sentences (147 tasks, 23 workflows, dual annotation with rubric, κ=1.00 with abstention explanation in a clause). Accuracy + warning precision in one paragraph with the key numbers (70.0/87.3; 60% overall, 70% SC4001, 29% SC3001, 6/6 SC4010). **Do not cut the 29% number** — per the audit it is the paper's credibility anchor. Cut Table 8 and Fig 4c (one of the two number-carriers survives in prose).
- **§5.7 (1.0 → 0.4):** timing in one sentence; scaling in one sentence (100 → 30,000 states, ≈0.2 ms → 0.13 s; fix the "adversarial" wording per audit); DSL conciseness one sentence; AWS deployment smoke test one sentence.
- **§5.8 CNCF (0.9 → 0.3):** one paragraph — 66 workflows, no IR/pass changes, mutation 64/64, declared-schema SC1010 exact where CNCF declares schemas, data-flow correctly silent. Keep it: vendor-neutral generalization is an ICSOC selling point. Details → TR.
- **§5.9 Threats (1.2 → 0.4):** one compact paragraph covering: opaque tasks/JSONata → ⊤; mutation recall is per-check reliability by construction; gold labels human; sample-repo corpus skews small; proofs in TR. Everything else → TR.

### §6 Related work (2.5 → 1.0)
- Keep four threads at 2–4 sentences each: durable execution (complementary), formal verification at other layers, behavioural types + Sagas, data-flow anomaly line (the direct precedent — keep its differentiation fullest).
- **Cut** the "three further threads" paragraph (shape inference, REST contracts, idempotency-by-SMT, commuting effects) to one 2-line sentence citing all four, or keep only Ding et al. (idempotency) and the Trcka anti-patterns contrast.
- Cut or shrink Table 9 (see triage).
- Add (per audit) Jangda et al. — one clause; a citation costs 2 lines, a reviewer's missing-citation complaint costs an accept.

### §7 Conclusion + Artifact (2.0 → 0.75)
- Conclusion to one paragraph (~10 lines): thesis restated, headline results in one sentence, type-systems analogy kept as the closer, future work in one sentence.
- Artifact availability to one short paragraph: link, contents in one sentence, "no AWS account needed to reproduce any figure," TR pointer. Cut the requirements/reproduction and distribution paragraphs → TR/README.

### References (3.0 → 2.0)
Prune 38 → ~27 and compress entries:
- **Cut candidates (10–11):** [12] Cauli et al. (weak fit — or replace with one Zelkova-line citation, net zero), [14] Colombo & Pace, [11] Burnay et al., [29] Seco et al. (keep one of the two contract-checking refs, not both), [30] Soethout et al., [28] Petricek et al., [26] Moser et al. *or* [31] von Stackelberg (keep one BPEL/BPMN data-flow ref besides [33] and [35]), one of the three serverless surveys [6]/[19]/[21] (keep one), one of [23]/[37] (keep one empirical-study ref), [36] Cadence (Temporal [34] + Durable Functions [10] suffice).
- **Keep untouched:** [1,3,2,4,9,5,8,10,13,15,16,17,18,20,22,24,25,27,32,33,34,35,38] + Jangda addition.
- Mechanical savings: drop "Accessed" dates where LNCS allows, drop arXiv IDs on formally published works, abbreviate venue names.

---

## 4. Technical report — table of contents (what moves where)

1. Full abstract-interpretation development: domain (former Table 4), concretization, transfer soundness lemmas, Theorem 1 proof incl. convergence gating and the O(|S|·|K|) derivation.
2. Well-typedness judgment Γ ⊢ W : A ⇒ B, the three inference rules, Theorem 2 proof (incl. the Choice branch-agreement case — per audit, sketch this one *also* in the paper in one sentence).
3. Saga model: A1–A4 discussion, R(P) formalization, SC4011 algorithm, Theorem 3 proof, reverse-order chain argument.
4. Guarantee boundary (former Table 3) and ASL feature-coverage matrix (former Table 5), incl. the lossy-field list.
5. Typed DSL: full builder API, Listing 1.3, the 20-statement → 119-line expansion.
6. Implementation detail: command set, per-module line counts, oracle design, mutation-operator catalogue.
7. Evaluation supplements: per-operator confusion matrix, gold-set rubric + per-label data, CNCF per-check results, full threats discussion, mined-pairs adjudication notes.

Every TR move gets an explicit forward pointer in the paper ("TR §n") — reviewers forgive absence far more readily than they forgive silence about absence.

---

## 5. Execution order (three passes)

**Pass 1 — free wins (~6 pp, no rewriting):** cut Fig 2, Fig 4a/b, Table 9, Listings 1.3/1.4/1.5; move Tables 3/4/5 to TR with stub sentences; prune references; compress captions.

**Pass 2 — structural compression (~10 pp):** rewrite §2.2 (fold defect mechanics into §3 passes), §3.3/§3.5 (theorem statements + one worked example, proofs out), §4 (three paragraphs), §5.2/5.3/5.6/5.9 per the plan.

**Pass 3 — line-level (~2–3 pp):** the house style runs long — double em-dash asides, restatements ("Equivalently, …", "Concretely, …", "As Section X established/noted"), and the honesty self-references (keep one). Target: every paragraph loses its last summarizing sentence unless it carries a number.

**Do-not-cut list (protect during all passes):** Fig 1; Table 2 (with guarantee column); Theorems 1–3 *statements* + A1–A4 one-liner; the restock-loop example; the CLI/CI adoption paragraph; Table 6; Table 7; Fig 5; the 29% SC3001 precision number; the construct-validity caveat on 100% recall; the emitter-independence firewall sentence; the CNCF paragraph.
