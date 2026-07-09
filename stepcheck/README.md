# stepcheck

A static verifier for AWS Step Functions / Amazon States Language (ASL) workflows.
Its centrepiece is a **sound data-flow / field-provenance analysis** of the ASL
I/O-processing pipeline that flags missing-field references with no false positives,
going beyond the JSONPath *syntax* that existing validators check. Around it,
`stepcheck` runs structural, typed-contract, typestate, retry-safety, concurrency,
temporal, and Saga-compensation checks — all before deployment.

```bash
cargo install stepcheck
stepcheck check workflow.asl.json --infer          # verify an existing definition
stepcheck check workflow.asl.json --annot facts.toml --deny-warnings   # CI gate
```

Exit status is `0` (clean), `1` (errors), or `2` (parse failure), so a `stepcheck`
check gates a pull request exactly like a compiler or linter. Findings render for a
terminal or, with `--json`, as a stable machine record. See the project repository
for the corpus, the evaluation harness, and the full command set.
