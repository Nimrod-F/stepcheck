# aws-solutions — production AWS Solutions Library workflows

Step Functions definitions from the **AWS Solutions Library** (GitHub org
`aws-solutions`): maintained, enterprise-deployed production solutions, not
didactic samples: an artifact class the public sample corpus lacks.

Note: many AWS solutions moved from the `aws-solutions` org to
`aws-solutions-library-samples` (e.g. Media2Cloud is now
`aws-solutions-library-samples/guidance-for-media2cloud-on-aws`). The `aws-solutions`
org's remaining repos are largely CDK-based; Step Functions are often defined in
CDK code (no committed ASL), so those solutions require `cdk synth` before
analysis.

Current set:
- **Media2Cloud** — 16 committed `stateMachineDefinition.json` files (audio / video /
  image / document analysis, face-indexer, ingest, removal; `Parallel` fan-out to
  child machines, nested `Map`s, 123 states).
- **Customizations for AWS Control Tower** — its committed CloudFormation template
  (`.cfn.yaml`), several state machines (ServiceControlPolicy, StackSet).
- **Network Orchestration for AWS Transit Gateway** — its committed CloudFormation
  template, the Orchestrator state machine.
- **Instance Scheduler on AWS** — `cdk synth` artifact; the only Step Functions
  machine is CDK custom-resource waiter plumbing and is correctly clean.
- **Automated Security Response on AWS** — `cdk synth` artifact; the orchestrator
  machine reports a retry-safety warning and an unbounded callback warning.
- **Distributed Load Testing on AWS** — `cdk synth` artifacts for the standard,
  ALB/ECS, and headless stacks; all three Step Functions machines are clean.
- **Account Assessment for AWS Organizations** — `cdk synth` artifact for the hub
  stack; its policy explorer Step Functions machine is clean.

CloudFormation templates and CDK `cdk.out/*.template.json` artifacts are read
directly by the CFN front end (multi-machine). Filenames flatten the upstream path
with `__`.

Run:

```bash
stepcheck scan corpus/aws-solutions --infer          # -> eval/aws-solutions-consolidation.json
```

Result: **26** state machines analysed, **14** flagged---native **SC0007**
(a `Choice` with no `Default`) on the Control Tower and Transit Gateway
orchestrators, native **SC6001** (callback with no timeout/heartbeat) on Automated
Security Response, plus advisory **SC4001** (persistent create without
compensation) and **SC3001** (broad retry on a non-idempotent
create/delete/publish/send).

The CDK-only solutions needed per-repo build fixes before synth: dependencies must
be installed at the owning `source/` or CDK package, Distributed Load Testing also
needs its web UI and legacy Lambda packages built/installed plus `cp.exe` on PATH
on Windows, and Account Assessment requires its synth environment variables and a
`deployment/regional-s3-assets/lambda.zip` asset (a placeholder suffices for
template synthesis when Poetry is unavailable). The checked-in artifacts are the
resulting deployment templates, not hand-extracted ASL.

For the CDK synth subset, the build-time gate metric is reproduced by
`python eval/industrial/aws_solutions_cdk_gate.py` ->
`eval/aws-solutions-cdk-gate.json`. The harness emits each template's ASL, injects
0/1/5 defects, embeds the mutated ASL back into a copy of the same CloudFormation
template, and times `stepcheck check --infer --deny-warnings` on that deployment
artifact. StepCheck catches 6/6 one-defect mutants and 5/5 five-defect mutants;
gate p95 stays 18--20 ms. `asl-validator` accepts only 2/6 clean CDK definitions,
so mutant silent-pass is reported only over those accepted-clean denominators
(2/2 at one defect, 0/1 at five defects).

Analysing Media2Cloud also surfaced and fixed a precision bug: the
`--result-shapes` ablation modelled `.waitForTaskToken` callbacks as their base
service envelope, but a callback's result is the arbitrary `SendTaskSuccess`
payload (now `Top`); see `dataflow_waitfortasktoken_result_is_opaque` in the tests.

Sources: <https://github.com/aws-solutions-library-samples/guidance-for-media2cloud-on-aws>,
<https://github.com/aws-solutions/aws-control-tower-customizations>,
<https://github.com/aws-solutions/network-orchestration-for-aws-transit-gateway>,
<https://github.com/aws-solutions/instance-scheduler-on-aws>,
<https://github.com/aws-solutions/automated-security-response-on-aws>,
<https://github.com/aws-solutions/distributed-load-testing-on-aws>,
<https://github.com/aws-solutions/account-assessment-for-aws-organizations>.
