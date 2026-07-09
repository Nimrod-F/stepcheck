# StepCheck — Full Paper Audit & Four-Persona Review Simulation

**Paper:** *Static Verification of AWS Step Functions with Sound Data-Flow Analysis and Workflow Semantics* (`paper/main.tex`, LLNCS)
**Target venue:** ICSOC 2026 (A-ranked, practical/engineering track)
**Review date:** 2026-07-06 (post-trim version)

---

## Executive Summary

**Editorial verdict: Borderline Accept → Accept after minor revision.**

The paper is in the strongest shape it has been: claims are calibrated, the theorems are
honestly scoped, the evaluation separates controlled from in-the-wild evidence, and it now
compiles to **exactly 15 pages including references** — the trim goal is already met. No
reviewer persona finds a fatal flaw. The residual risk is concentrated in three places:

1. **The JSONata blind spot is never quantified.** JSONata is AWS's recommended mode for
   new state machines (since late 2024), and the data-flow core — the headline
   contribution — degrades to ⊤ on exactly those workflows. This is Reviewer C's sharpest
   weapon.
2. **The mutation study's 100% recall is near-tautological** (self-injected mutants
   targeting the tool's own checks), and the abstract doesn't say "self-injected."
3. **The semantic tier has zero in-the-wild validation** — no real workflow carries
   annotations, so the "layered semantics" pitch rests on built-in examples.

All three are fixable with sentences, not experiments.

**Realistic ICSOC score vector:** A: weak accept · B: accept · C: weak accept · D: weak
accept — a paper that gets in when the revision addresses the P1 list at the end of this
document.

---

## Part 1 — Per-Section Academic Audit

### Abstract — B+

Dense, well-calibrated, and quantified. "No false positives *under the modeled JSONPath
semantics*" is exactly the right hedge, and "sound bug-finding rather than complete
verification" appears in the intro to back it.

- **Fix:** "detects all 616 targeted mutants" (`main.tex:92`) omits that the mutants are
  **self-injected by StepCheck's own mutator**. A reviewer who discovers this in §5.3
  after reading the abstract feels oversold. One word — "616 self-injected targeted
  mutants" — removes the risk.
- **Nit:** "seven workflow checks" (abstract) vs. "eight passes" (Table 2, Fig. 3) is
  internally consistent (7 around the data-flow core) but forces the reader to do
  arithmetic. Consider "seven further checks."

### §1 Introduction — A−

The thesis — *verify the deployed artifact under layered semantics* — is crisp and
repeated at the right places. Figure 1 with six numbered latent defects is an effective
anchor, and every contribution bullet is falsifiable.

- **Fix:** contribution bullet 2 (`intro.tex:49`) says "no-false-positive data-flow
  analysis" *without* the "modeled semantics" hedge that the abstract and Theorem 1
  carry. Inconsistent hedging is where reviewers accuse you of overclaiming. Add
  "(under the modeled JSONPath semantics)".
- **Nit:** "zero observed data-flow false positives" (bullet 4) is technically vacuous on
  the wild corpus, since SC1101 fires zero times there; the real content is the
  oracle-confirmed 84. The current wording survives scrutiny, but only barely.

### §2 Background — A−

The ASL slice (`background.tex:11-19`) is precise and exactly as large as the analyses
need. R1–R4 are a good rubric, and Table 1 (facts and their sources) is the clearest
single artifact in the paper. The JSONata scoping sentence (`background.tex:30-31`)
correctly closes the old C2 blocker.

- **Gap:** the paper never says **how prevalent JSONata is** — in the corpus or in the
  ecosystem. One sentence ("N of 193 corpus workflows use JSONata mode") would convert an
  open flank into a measured limitation.

### §3 Verification Framework — B+

The three-theorem structure with proofs deferred to the technical report (which exists
and compiles — `paper/techreport/main2.pdf`) is appropriate for LNCS. Specific issues,
all raised again by Reviewer A below:

- Termination is handled by an **iteration cap + report suppression**
  (`approach.tex:106-110`) rather than widening; the cap value and its hit-frequency on
  the corpus are never stated.
- **SC1010 wording tension:** Table 2 says "successor requires fields not produced
  *upstream*" (`approach.tex:69`) but Theorem 2 checks the **edge-local** condition
  in(T₂) ⊆ out(T₁) (`approach.tex:164-165`). These differ for pass-through fields
  produced two steps earlier.
- **SC1110's exclusion from Theorem 1** (`approach.tex:132`) is asserted but never
  justified. (The likely reason — `IsPresent` guards make reading an absent operand
  legitimate — is worth the half-sentence.)
- The word **"conservative"** for the under-approximating concurrency/temporal checks
  inverts standard static-analysis usage (conservative usually = over-approximating /
  sound). §3 does define it, but a formal-methods reader will stumble.

### §4 Implementation — A−

The strongest engineering section: single binary, exit-code CI contract, `--infer` →
`--annot` escalation path, and above all the **code-disjoint execution oracle**
(`implementation.tex:42-47`) — differential testing of a soundness theorem is a
genuinely credible move. The limitations paragraph is honest.

- **Nits:** "compact for eight analysis passes… a density that follows from a shared IR
  and Rust's expressive pattern matching" (`implementation.tex:5-8`) is
  self-congratulatory filler; the "*planned* `npx` wrapper" promise weakens rather than
  strengthens.

### §5 Evaluation — B

Broad (RQ1–RQ3, six sub-studies), and the hedges are mostly in the right places
("per-check reliability… not complete bug coverage"; "existence evidence, not a
prevalence estimate"). Weaknesses, each picked up by a persona below: mutant circularity;
baseline-strawman risk; **warning-tier precision never adjudicated** (117 SC4001
warnings on 193 curated samples — how many are real?); scale sweep uses only **acyclic
synthetic chains**; a single Express-mode smoke deployment; corpus is small and curated
(median 6 states).

- **Concrete bug:** "then fires on 84 injected missing-field defects **in the typed
  tier**" (`evaluation.tex:94`) contradicts §5.4's own framing of the same 84 as
  corpus-scale native detections ("84 of 126 (67%)"). If the 84 needed declared schemas,
  the native tier's power is untested; if not, "typed tier" is a leftover phrase. Either
  way this sentence must be fixed — it is the kind of internal inconsistency that costs
  a score point.
- Arithmetic all checks out: 616 = 166+141+103+19+21+166; baseline totals 306/288/302 ✓;
  193 × 39 µs ≈ 7.5 ms ✓.

### §6 Related Work — B−

Correct positioning in four compressed paragraphs; the BPMN/BPEL data-flow contrast
(hand-built models vs. the executable artifact) is the right one for ICSOC's heritage.

- **Gap (real):** **Flux (Ding et al., OSDI'23) is in the bib as `ding2023flux` but never
  cited in the text.** Automated idempotence verification for stateful serverless is
  *directly* adjacent to SC3001 — a reviewer who knows it will ask why it's absent, and
  the honest answer ("complementary: Flux could *supply* the idempotency premises
  StepCheck consumes") actually strengthens the layered-premises story. Similarly
  Beldi (`zhang2020beldi`, transactional serverless workflows) and Temporal/Cadence are
  bib-only.

### §7 Conclusion + Artifact — A−

Proportionate; no new claims. Anonymized artifact link present; tech report exists. Fine.

---

## Part 2 — Reviewer Panel Simulation

### Reviewer A — Formal Methods · **Weak Accept (leaning Accept)**

*"The soundness claim is scoped correctly and I appreciate a paper that says 'sound
bug-finding, not complete verification' in its own abstract. My concerns are about what
is left informal."*

**On theorem rigor.** Theorem 1 has the right logical shape: the shape domain
over-approximates *may-presence*, so definite absence in the abstract state implies
absence in every concrete document — the standard dual argument. Theorem 2, as stated in
the paper, is **nearly definitional**: the checker checks exactly the edge condition, so
"no finding iff edge condition" carries little content; the actual content (equivalence
with a typing judgment on the series-parallel fragment) lives entirely in the technical
report. Acceptable for LNCS, but the paper should not present Thm 2 with the same weight
as Thm 1. Theorem 3's four assumptions (single post-commit fault, feasible paths, unique
generic catcher, linear handlers where reachability = execution) are strong but
*declared* — good practice. Note what they exclude: **a failing compensator** (what if
`RefundPayment` itself fails?) and branching handlers.

**On soundness assumptions.** Three questions the paper must survive:

1. **Which JSONPath fragment is modeled?** Plain dotted paths clearly; what of wildcards,
   filters, array slices — lifted to ⊤ or rejected? Never stated.
2. The **iteration cap**: what is its value, and was it ever hit on the 193-workflow
   corpus or the 30k-state sweep? If the answer is "never," say so — it converts an
   ad-hoc guard into a non-issue.
3. Why is **SC1110 outside Theorem 1** when a `Choice` on a definitely-absent operand
   also fails at runtime? (If the answer is `IsPresent`-style presence tests, write it
   down.)

**On abstract interpretation.** The transfer functions mirroring the five-stage I/O
pipeline are the paper's real technical meat, and the cited framing (Cousot & Cousot) is
apt. But the domain is only informally a lattice: join = key-union with ⊤ absorbing is
stated; a partial order, monotonicity of transfers, and the absence of infinite ascending
chains are not — and indeed the last **fails** (shapes can grow unboundedly through
cycle re-embedding, as the paper admits). The orthodox fix is a **depth-k widening to
⊤**, which preserves soundness *and* guarantees termination, instead of
cap-and-suppress. The paper should say why it didn't do this (one sentence: e.g.,
suppression was simpler and never triggered in practice).

**On lattice correctness.** The differential oracle (`implementation.tex:42-47`)
partially substitutes for a mechanized proof and I value it — but the oracle walks
**acyclic** paths only, so the fixpoint/loop-join logic (the one part with the
termination subtlety) is exactly the part the oracle does not exercise. State this in
threats.

Also: fix the "conservative" terminology and the SC1010 edge-local/upstream tension noted
above.

---

### Reviewer B — Software Engineering · **Accept**

*"This is what a practical verification paper should look like: single binary, runs on
the artifact you already have, exit codes a CI understands, and an adoption gradient
from zero annotations to declared contracts."*

**Practicality.** The zero-migration story is credible and demonstrated: all 193 raw
definitions parsed, tolerant lowering of SAM placeholders and intrinsics, findings in
tens of microseconds. The graded severity design (declared ⇒ error, inferred ⇒ warning)
is a genuinely good CI ergonomics decision and the recommended policy
(`implementation.tex:31-37`) is exactly right.

**Usability.** Weakest of my four criteria — **no evidence beyond design intent**. The
`--json` output carries remediation notes, but no example diagnostic is shown, message
quality is unevaluated, and there is no developer study or even anecdote. More
important: **warning-tier precision is never measured.** 117 SC4001 + 36 SC3001 + 28
SC4010 warnings across 193 *curated sample* workflows is a lot of yellow; if most are
noise, `--infer` mode trains developers to ignore the tool. Adjudicating a random sample
of ~30 warnings (true / plausible / noise) would cost a paragraph and answer my biggest
question.

**Annotations.** The 69-line TOML sidecar for a 4-task saga is a fair burden datapoint,
and "inference abstains when no signal matches" is the right default. Two open
questions:

1. **Annotation rot** — nothing checks the sidecar against the actual Lambda code, so
   declared premises can silently drift from reality; the paper says truth of premises
   is out of scope (fine) but should name drift as the operational risk and note the
   "generated metadata" source as the mitigation path.
2. Do name-based inference heuristics survive non-English or abbreviated naming
   conventions?

**CI integration.** "Gates a pull request exactly like a compiler or linter" is
asserted, not demonstrated. Even a 5-line GitHub Actions snippet, or one sentence
reporting that the authors run it on their own repo's CI, would ground it. The `npx`
wrapper being "planned" should be cut or done.

---

### Reviewer C — Cloud Systems · **Weak Accept**

*"The tool solves real AWS problems and the real-bug table proves it. But the evaluation
lives in a sample-repository bubble, and the paper is silent about the direction AWS
itself is moving."*

**AWS realism.** The defect classes are the right ones — I have personally seen the
broad-retry double-charge and the missing callback timeout in production. The six mined
real bugs (Table 4), including one in an official AWS sample, are the paper's most
persuasive evidence. **However: JSONata.** AWS has recommended JSONata and workflow
variables for new state machines since late 2024; the data-flow core — the paper's
headline contribution — degrades to ⊤ on exactly those workflows. The paper handles
this *soundly* but never quantifies it: how many of the 193 are JSONata-mode? What
fraction of new AWS samples are? Without a number and a one-sentence roadmap, the
contribution reads as aimed at the trailing edge of the platform. Also unaddressed:
resource-key extraction covers DynamoDB/SQS/SNS/S3 — what about EventBridge, ECS,
Bedrock, and above all **nested workflows (`states:startExecution`)**, where
cross-workflow races escape SC5001 entirely?

**Workflow corpus.** 193 workflows / 1,354 states, median **6** states, max 33, all from
curated sample collections. That is the shallow end of production Step Functions (real
order/data-pipeline machines run 50–300 states with deep Map nesting). The threats
section admits the skew; the real-bug mining (39 pairs, 5 repos) partially compensates.
A handful of large open-source production definitions (e.g., from AWS's own service
reference architectures) would materially raise confidence.

**Scalability.** Microsecond latencies and the 30k-state sweep are convincing for the
*easy* case — but synthetic **acyclic chains** are precisely the shape where a fixpoint
analysis is trivial. The interesting stress is wide `Parallel` fan-out, deeply nested
`Map`, and loop-heavy graphs where shape joins and iteration counts grow. One extra
sweep dimension (branching factor) would close this.

**Deployment.** One Express-mode smoke test of one DSL-generated workflow is thin. No
Standard-mode test, no live `.waitForTaskToken` callback, no evidence at scale that
emitted ASL passes AWS's server-side validation (though the baseline study implies
mutant ASL parses). "Zero runtime overhead" is true by construction for a static tool —
fine to state once, but it is not a finding.

---

### Reviewer D — General Systems · **Weak Accept**

*"The layered-provenance architecture is the idea I'd steal. The evidence is honest but
thinner than its packaging."*

**Novelty.** Checking the *deployed executable artifact* rather than a hand-built model
is a real delta over the BPMN/BPEL data-flow lineage (Sun et al., Trčka et al.), and the
provenance-graded severity (native/declared/inferred → error/warning) is a transferable
design pattern I have not seen articulated this cleanly for workflow verification. The
JSON-shape abstract interpretation through ASL's five-stage I/O pipeline is a modest but
genuine technical contribution. Novelty is sufficient for ICSOC.

**Comparison.** The baseline table (Table 3) shows the existing validators scoring 0 on
four semantic classes — but they *never claimed* those classes. This demonstrates
**complementarity, not superiority**, and the paper mostly frames it that way; keep it
that way and consider saying "no existing tool attempts these classes" explicitly. The
missing comparisons are the adjacent research tools: **Flux** (OSDI'23, idempotence
verification — could feed SC3001's premises), **Beldi** (transactional serverless
workflows), and the durable-execution engines (Temporal, Cadence) beyond the one Durable
Functions sentence. All are already in the bib; they cost three sentences.

**Significance.** Honestly medium. Six confirmed real bugs is existence proof, not
impact; 100% mutant recall is expected by construction; the strongest results are the
oracle-validated soundness (84/84) and the CNCF port touching zero passes. The paper's
own hedging ("existence evidence, not a prevalence estimate") is to its credit.

**Impact.** Good: open artifact, single-binary adoption path, and the CNCF
generalization suggests the IR outlives the AWS-specific frontend. If the semantic tier
ever gets real-world annotations, this becomes a platform; today that tier is validated
only on the authors' two built-in sagas — the paper should not hide that (it doesn't,
but only a careful reader of §5.2 notices "silent on the wild corpus because no workflow
declares a compensator").

---

### Devil's Advocate — Strongest Counter-Argument (no CRITICAL findings)

> "Strip the framing and here is what was measured: a mutation study in which **the
> tool's own mutator injects exactly the defect classes the tool's own checks target**
> (100% recall is close to true-by-construction); a baseline comparison in which
> competitors score zero **on classes they never attempt**; a headline soundness theorem
> whose check **fires zero times on all 193 real workflows**; and a semantic tier that
> has **never seen a real annotation** — SC4011 is silent everywhere except the authors'
> two built-in examples. The independently verifiable evidence is: six mined real bugs
> (two of them from the flagship SC1101 check), an 84/84 differential-oracle validation,
> and a clean CNCF port."

**Verdict: MAJOR, not CRITICAL** — because the paper pre-empts each point with explicit
hedges ("per-check reliability," "existence evidence," "silent because no workflow
declares a compensator") and the residual evidence (real bugs + oracle + port) is
genuine. This does not block acceptance, but it defines exactly what the abstract must
not oversell — hence the "self-injected" fix.

*Ignored alternative:* the paper never discusses whether **schema-based contract
generation** (from Lambda handler types / EventBridge schemas) could replace
hand-written sidecars — the strongest response to the "nobody will write annotations"
attack, and it's one sentence.

---

## Part 3 — Consensus, Disagreements, Decision

**Consensus (3+ reviewers):**
- Claims are well-calibrated post-revision (A, B, D).
- JSONata coverage must be quantified (A, C, D).
- Warning-tier precision / real-world semantic-tier validation is the evidence gap
  (B, C, D).
- The real-bug table and differential oracle are the paper's most credible assets
  (all four).

**Disagreement:** Reviewer B considers the adoption story sufficient as designed;
Reviewer C wants demonstrated deployment evidence. Arbitration: for an
engineering-track venue, B's bar is the operative one, but C's JSONata point stands
regardless.

**Decision: Minor Revision → Accept.**
Score vector: **A: weak accept · B: accept · C: weak accept · D: weak accept.**

---

## Revision Roadmap (prioritized; all fit within the current 15 pages)

### P1 — do before submission (cheap, kills known attack lines)

1. Fix the **"typed tier"** sentence at `evaluation.tex:94` — internal inconsistency,
   worst finding in the audit.
2. Add "**self-injected**" to the abstract's 616-mutant claim (`main.tex:92`) and the
   missing "modeled JSONPath semantics" hedge to intro bullet 2 (`intro.tex:49`).
3. Report the **JSONPath/JSONata split of the corpus** + one roadmap sentence
   (§2.1 or §5.1).
4. State the **iteration-cap value and that it was (presumably) never hit** on corpus +
   sweep (§3.3); note the oracle covers acyclic paths only (threats).
5. **Cite Flux** (`ding2023flux`) as a complementary idempotency-premise source and add
   one sentence on Temporal/Cadence (§6) — both already in the bib.
6. One-line rationale for **SC1110 staying a warning** (`IsPresent` guards) (§3.3).

### P2 — should do if time permits

7. Clarify SC1010's out(T) semantics (edge-local vs. accumulated pass-through) in
   Table 2 + Thm 2.
8. Adjudicate a random sample of ~30 inferred warnings and report precision
   (one paragraph in §5.2).
9. Add a branching/nesting dimension to the scale sweep (§5.6).
10. Replace the "planned npx" promise with a shipped wrapper or delete it; optionally
    show a 5-line CI config.

### P3 — camera-ready polish

11. Cut the "density follows from…" self-praise in §4.
12. Mention `states:startExecution` cross-workflow races as an SC5001 limitation.
13. Note schema-derived contract generation as the answer to annotation rot.
