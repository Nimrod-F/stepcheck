# aws-templates — real deployment artifacts (serverless-patterns)

Step Functions workflows mined from **`aws-samples/serverless-patterns`**, a
recognized real-world collection of AWS serverless reference patterns. Unlike the
curated `.asl.json` corpora, these are the **deployment artifacts** teams ship:
SAM / CloudFormation `template.yaml` files (with `!Sub`/`!GetAtt` intrinsics and
inline `DefinitionString` / `Definition` / `DefinitionUri`) alongside their
`statemachine/*.asl.json` definitions.

They are analysed **directly** by StepCheck's CloudFormation/SAM front end
(`stepcheck/src/cfn.rs`): `stepcheck check <template.yaml>` finds every
`AWS::StepFunctions::StateMachine` / `AWS::Serverless::StateMachine`, resolves the
intrinsics, and follows `DefinitionUri` to the sibling ASL file.

Filenames are the upstream paths flattened with `__`. Reproduce the consolidation:

```bash
stepcheck scan corpus/aws-templates --infer          # or per-file check
# -> eval/aws-templates-consolidation.json (deduped by pattern)
```

Result: **55** distinct pattern workflows analysed; **13** flagged (advisory
retry/compensation plus two sound-tier findings: a `Choice` with no `Default`
(`SC0007`) and an unbounded callback (`SC6001`)). Source:
<https://github.com/aws-samples/serverless-patterns>.
