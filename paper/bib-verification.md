# Bibliography Verification Log

Paper: *Safe Serverless Workflow Orchestration through Typestate Verification,
Retry Safety, and Compensation Checking* (ICSOC 2026, Springer LNCS).

Verification date: **2026-06-18**; addenda below record later citation additions.
Tools used: arXiv `get_abstract`/`search_papers`, Crossref REST API (DOI
resolution), Semantic Scholar `get_paper`/`search_papers`.
Validation: `references.bib` compiles cleanly under `splncs04.bst`
(BibTeX exit 0, 0 warnings, 44 entries formatted).

**Headline result:** the candidate list's *arXiv* IDs were almost all correct,
but its *DOIs* were systematically wrong (8 of 10 DOIs pointed at unrelated
papers), and one arXiv ID and one S2 ID were fabricated/incorrect. Every wrong
identifier below was traced to the correct one or the entry was dropped. The
three "suspicious 2026 arXiv IDs" the synthesis flagged (⚠) all turned out to be
**genuine** — the high sequence numbers (e.g. 2604.24550) are normal for current
arXiv monthly volumes, not a sign of fabrication.

Final count: **44 references** (40 citable works + 4 official web resources).

---

## VERIFIED (id/metadata confirmed exactly as claimed)

### arXiv preprints (confirmed via arXiv get_abstract — title + authors match)
| Cite-key | arXiv ID | Note |
|---|---|---|
| garciaLopez2018faas | 1807.11248 | VERIFIED |
| arjona2021triggerflow | 2106.00583 | VERIFIED (FGCS / Triggerflow) |
| yu2021pheromone | 2109.13492 | VERIFIED (Pheromone) |
| wen2021empirical | 2101.03513 | VERIFIED |
| li2023dataflower | 2304.14629 | VERIFIED |
| burckhardt2021netherite | 2103.00033 | VERIFIED (Durable Functions + Netherite preprint) |
| zhang2020beldi | 2010.06706 | VERIFIED (Beldi) |
| kulkarni2025characterizing | 2509.23013 | VERIFIED |
| li2025jointlambda | 2505.21899 | VERIFIED (Joint-lambda) |
| zhang2025causalmesh | 2508.15647 | VERIFIED (CausalMesh, Dafny) |
| cauli2022iac | 2202.12592 | VERIFIED |
| denielou2012pmpst | 1208.6483 | VERIFIED |
| carbone2018choreographies | 1808.05088 | VERIFIED |
| garciaMolina1987sagas (also DOI) | — | see DOI section |
| colombo2014compensation | 1404.0849 | VERIFIED |
| hellerstein2019calm | 1901.01930 | VERIFIED (CALM) |
| chang2025sagallm | 2503.11951 | VERIFIED (SagaLLM) |
| mohammad2025resilient | 2512.16959 | VERIFIED (author corrected, see below) |
| bartoletti2012lending | 1211.3624 | VERIFIED |
| fu2024schema | 2406.11227 | VERIFIED (Compound Schema Registry) |
| burnay2020restscript | 2007.08048 | VERIFIED (SafeRESTScript) |
| sarkar2024hastee | 2401.08901 | VERIFIED (HasTEE+) |
| khan2025iac | 2510.03902 | VERIFIED (MACOG) |

**Flagged-but-genuine 2026 arXiv IDs (the ⚠ entries — all resolve correctly):**
| Cite-key | arXiv ID | Note |
|---|---|---|
| chen2026mono2sls *(dropped, see below)* | 2604.24550 | arXiv ID is REAL (Mono2Sls, pub. 2026-04-27). Not fabricated. |
| *(Edge-Cloud-Space — dropped)* | 2605.04316 | arXiv ID is REAL (Malazi et al., pub. 2026-05-05). Not fabricated. |
| *(NEST — dropped)* | 2604.21795 | arXiv ID is REAL (Larsen et al., pub. 2026-04-23). Not fabricated. |

### DOI / venue entries (confirmed via Crossref or Semantic Scholar)
| Cite-key | Identifier | Note |
|---|---|---|
| burckhardt2021durable | doi:10.1145/3485510 | VERIFIED — distinct OOPSLA 2021 paper (see CORRECTED) |
| campos2011channels | doi:10.4204/EPTCS.69.2 | VERIFIED |
| gerbo2019typestate | doi:10.4204/EPTCS.291.3 | VERIFIED |
| garciaMolina1987sagas | doi:10.1145/38713.38742 | VERIFIED (SIGMOD '87, pp. 249-259) |
| acciai2006transactions *(dropped for trim)* | arXiv:cs/0610137 | VERIFIED (real) but trimmed |
| soethout2019atomic *(dropped for trim)* | arXiv:1908.05940 | VERIFIED (real) but trimmed |

### Foundational references added (searched fresh, all VERIFIED via Crossref)
| Cite-key | Identifier | Note |
|---|---|---|
| stromYemini1986typestate | doi:10.1109/TSE.1986.6312929 | IEEE TSE SE-12(1):157-171, 1986 |
| fahndrich2004typestates | doi:10.1007/978-3-540-24851-4_21 | ECOOP 2004, pp. 465-490 (DeLine & Fähndrich) |
| honda1993dyadic | doi:10.1007/3-540-57208-2_35 | CONCUR'93, LNCS 715, pp. 509-523 |
| honda2008multiparty | doi:10.1145/1328438.1328472 | POPL 2008, pp. 273-284 |
| vanDerAalst1998petri | doi:10.1142/S0218126698000043 | J. Circuits Syst. Comput. 8(1):21-66, 1998 |

### Real but ID-less papers confirmed by Semantic Scholar title+author+year match
(originally carried fabricated S2 hex IDs; the *papers* are real)
| Candidate | S2 result | Decision |
|---|---|---|
| Barkaoui, Ayed, Sbaï — "Workflow Soundness Verification based on Structure Theory of Petri Nets" (2007) | Found in S2 (Kamel Barkaoui, R. Ayed, Zohra Sbaï, 2007) | Paper is real, but no DOI and obscure venue (IJCIS). **Dropped for trim/quality** (redundant with blondin2022soundness + vanDerAalst1998petri). |
| Missaoui, Sbaï, Barkaoui — "Model Checking Verification of Web Services Composition" (2016, ACT4SOC) | Found in S2 (exact title/authors/venue/year match) | Paper is real, no DOI. **Dropped for trim** (redundant with bianculli2007bpel). |
| Reddy — "Theoretical Frameworks for API-First and Shift-Left Quality Engineering..." (2026, JISEM) | Found in S2 (N. Reddy, 2026, JISEM) | Paper is real. **Dropped for quality** (low-tier journal; redundant with the stronger CDC refs lehva/ayas/wu). |

### Official web resources (cited as @misc with accessed date 2026-06-17)
awsStepFunctions, cncfServerlessWorkflow, temporal, cadence — standard,
stable documentation/spec URLs. Included per task requirement.
awsStatelint — AWS Labs `statelint` GitHub repo (Apache-2.0), the reference
ASL validator used as the evaluation baseline (Section 6); v0.8.0 installed
from RubyGems and run over the corpus + mutants. Added 2026-06-19.

---

## CORRECTED (old id -> new id)

| Cite-key | Field | OLD (wrong) | NEW (verified) | Evidence |
|---|---|---|---|---|
| crafa2016actors | arXiv | **1610.05524** (resolved to an unrelated *math* paper on hyper-Bessel fractional differential equations) | **1607.02927** | arXiv get_abstract confirmed 1607.02927 = "On the chemistry of typestate-oriented actors", Crafa & Padovani, 2016 |
| udomsrirungruang2025mpst | DOI + year | **10.1145/3632927** (resolved to "Sound Gradual Verification with Symbolic Execution") + year 2024 | **10.1145/3704872**, year **2025** (PACMPL 9, POPL) | Crossref bibliographic search |
| casetta2026mpst | DOI | **10.4204/EPTCS** (incomplete — no volume) | **10.4204/EPTCS.444.7** (EPTCS vol. 444, pp. 68-78) | Crossref |
| blondin2022soundness | DOI | **10.1007/978-3-031-13188-2_15** (resolved to an eBPF VM proof paper) | **10.1007/978-3-031-13188-2_23** | Crossref (correct CAV 2022 LNCS chapter) |
| bianculli2007bpel | DOI | **10.1109/SOCA.2007.29** (resolved to "Mobile Agent and Web Service Integration Security Architecture") | **10.1109/SOCA.2007.5** | Crossref |
| seco2020contracts | DOI | **10.22152/programming-journal.org/2020/4/16** (resolved to "Constructing Hybrid Incremental Compilers") | **10.22152/programming-journal.org/2020/4/10** | Crossref |
| lehva2019cdc | DOI | **10.1007/978-3-030-35333-9_7** (resolved to a GQM+/OKR case study) | **10.1007/978-3-030-35333-9_35** | Crossref (PROFES 2019, pp. 497-512) |
| ayas2022cdc | DOI | **10.1109/SEAA56994.2022.00010** (resolved to "Negative Transfer in Cross Project Defect Prediction") | **10.1109/SEAA56994.2022.00022** | Crossref (SEAA 2022, pp. 92-99) |
| wu2022eventdriven | DOI | **10.1109/APSEC57359.2022.00057** (resolved to "RP2A: Rare Process-Pattern Analysis") | **10.1109/APSEC57359.2022.00064** | Crossref (APSEC 2022, pp. 467-471) |
| burckhardt2021durable | venue/id | listed as OOPSLA but tagged with arXiv:2103.00033 (which is the *Netherite* preprint) | DOI **10.1145/3485510** (PACMPL 5, OOPSLA) — separated from the Netherite preprint, now its own entry `burckhardt2021netherite` | Crossref + arXiv |
| cao2006bpel *(dropped for trim)* | DOI | **10.1109/CIT.2006.6** (resolved to an EPC/RFID retrieval paper) | correct DOI is **10.1109/CIT.2006.185** | Crossref (recorded here in case re-added) |
| clempner2014analytical *(dropped for trim)* | DOI | **10.2478/amcs-2014-0070** (404 Not Found) | correct DOI is **10.2478/amcs-2014-0068** | Crossref (recorded here in case re-added) |
| mohammad2025resilient | author | "Sajib Mohammad" | **"Muzeeb Mohammad"** (per arXiv) | arXiv get_abstract |
| burnay2020restscript | author | "Nicolas Burnay" | **"Nuno Burnay"** (per arXiv) | arXiv get_abstract |

---

## DROPPED (with reason)

| Candidate | Reason |
|---|---|
| Reddy 2026 — "Theoretical Frameworks for API-First and Shift-Left Quality Engineering in Microservices Architectures" (JISEM) | **Quality.** Real S2 record, but single-author paper in a low-tier journal; redundant with the stronger consumer-driven-contract references (lehva2019cdc, ayas2022cdc, wu2022eventdriven). Original S2 hex id was also fabricated/non-resolving. |
| Barkaoui, Ayed, Sbaï 2007 — "Workflow Soundness Verification based on Structure Theory of Petri Nets" (IJCIS) | **Trim/quality.** Paper is real (S2), but no DOI, obscure venue; the Petri-net-soundness point is already carried by blondin2022soundness and vanDerAalst1998petri. Original S2 hex id (`df8a2d2d…`) did **not** resolve (NotFound). |
| Missaoui, Sbaï, Barkaoui 2016 — "Model Checking Verification of Web Services Composition" (ACT4SOC) | **Trim.** Paper is real (S2, exact match), but no DOI; redundant with bianculli2007bpel for BPEL/model-checking. Original S2 hex id was fabricated. |
| chen2026mono2sls (2604.24550) | **Trim/relevance.** arXiv ID is genuine, but monolith-to-serverless *migration* via LLM agents is tangential to a static *verifier*. |
| Malazi et al. 2026 — Edge-Cloud-Space (2605.04316) | **Trim/relevance.** Genuine arXiv ID; LEO/edge-continuum orchestration is out of scope for the ASL verifier. |
| Larsen et al. 2026 — NEST (2604.21795) | **Trim.** Genuine arXiv ID; network-data-plane session-type monitoring is adjacent but covered by the session-type foundations (honda*, denielou). |
| psarakis2025styx (2512.17429) | **Trim.** Genuine; durable-execution point already covered by burckhardt2021durable/netherite + zhang2020beldi. |
| colosi2025wasm (2512.04089) | **Trim/relevance.** Genuine; WebAssembly portability tangential to the verification contribution. |
| acciai2006transactions (cs/0610137) | **Trim.** Genuine; process-calculus transaction theory secondary to the Saga-completeness story. |
| soethout2019atomic (1908.05940) | **Trim.** Genuine (appeared twice in candidate list); atomic-commit/PSAC point secondary. |
| zeng2024semantic (2412.12493) | **Trim.** Genuine; LLM-transaction semantic-error handling tangential. |
| ripon2014bpel (1402.5592) | **Trim.** Genuine; BPEL-vs-cCSP comparison redundant with the BPEL model-checking line. |
| lange2012synthesising (1204.2566), jaber2019choreographies (1905.13529) | **Trim.** Genuine; choreography *synthesis* is secondary to the typestate/MPST conformance angle (kept honda*, denielou, carbone, casetta). |
| ferreira2025mlops (2506.06202), edwards2024schema (2412.06269) | **Trim.** Genuine; data-contract/schema angle already carried by seco2020contracts, fu2024schema, and the CDC trio. |
| sarkar2023hastee (2307.13172) | **Trim.** Genuine; superseded for citation by the newer HasTEE+ (sarkar2024hastee). |
| clempner2014analytical, cao2006bpel | **Trim.** Real works (correct DOIs recorded above); Petri-net/BPEL points already covered. |

---

## Could not resolve
None. Every candidate either (a) verified to a correct identifier, (b) had its
wrong identifier corrected to a verified one, or (c) was deliberately dropped for
relevance/quality/redundancy (with the underlying paper's real status noted
above). No citation in the final `references.bib` is unverified.

---

## Addendum 2026-07-08 (production Figure 1 revision)

The introductory figure was rewritten around AWS Serverless Airline Booking. Two references were
added and BibTeX still exits cleanly:

| Cite-key | Identifier | Note |
|---|---|---|
| awsServerlessAirline | https://github.com/aws-samples/aws-serverless-airline-booking | VERIFIED -- public AWS `aws-samples` repository; the `ProcessBooking` ASL is in `src/backend/booking/template.yaml` on the `master` branch. |
| eismann2022stability | doi:10.1016/j.jss.2022.111294 | VERIFIED via Crossref -- Eismann et al., "A Case Study on the Stability of Performance Tests for Serverless Applications", Journal of Systems and Software 189:111294, 2022. |

---

## Addendum 2026-06-19 (data-flow / concurrency / temporal revision)

The paper was extended with a sound data-flow analysis (abstract interpretation) and
concurrency/temporal analyses; the title changed accordingly. Two references were
added (BibTeX still exit 0):

| Cite-key | Identifier | Note |
|---|---|---|
| cousot1977 | doi:10.1145/512950.512973 | VERIFIED — Cousot & Cousot, "Abstract Interpretation", POPL 1977, pp. 238-252; the canonical reference for the data-flow analysis framing. |
| soethout2019psac | arXiv:1908.05940 / Programming Journal 4(1), 2020 | VERIFIED — same real paper previously logged as `soethout2019atomic` (then trimmed); re-added to ground the concurrency-interference related work. Title: "Path-Sensitive Atomic Commit", Soethout, van der Storm, Vinju. |
