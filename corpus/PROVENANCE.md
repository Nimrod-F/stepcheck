# Corpus provenance and licensing

Every workflow in `corpus/` is either mined from a public repository or authored by us.
This file records, for each subset, where the files come from, under which licence the
upstream publishes them, and whether that licence permits redistribution inside this
artifact. Licences were verified against the upstream repositories (GitHub licence
metadata and, where that was inconclusive, the repository's `LICENSE`/`LICENSE.txt`
file) on 23 September 2026.

StepCheck itself is licensed under Apache-2.0 (see `LICENSE` at the repository root).
That licence covers our code and the workflows we authored; it does **not** override the
licences of the third-party workflows listed below, which remain under their own terms.

## Summary

| Subset | Files | Distinct | Upstream | Upstream licence | Redistributable |
|---|---:|---:|---|---|---|
| `asl/` | 193 | 169 | `aws-samples/step-functions-workflows-collection` (170), `aws-samples/aws-stepfunctions-examples` (23) | MIT-0 | yes |
| `aws-templates/` | 98 | 97 | `aws-samples/serverless-patterns` | MIT-0 | yes |
| `aws-solutions/` | 25 | 25 | `aws-solutions` org (7 solutions, see below) | Apache-2.0 | yes, with attribution |
| `cncf/` | 66 | 65 | `serverlessworkflow/specification` | Apache-2.0 | yes, with attribution |
| `cncf-typed/` | 4 | 4 | authored by us | Apache-2.0 | yes |
| `industrial/` | 6 (+README) | 5 | `aws-samples` patterns + `aws-samples/aws-serverless-airline-booking` | MIT-0 | yes |
| `realbugs/` | 12 pairs + issues | — | five public repos (see below) | mixed, **2 repos unlicensed** | partly |
| `wild-external/` | 95 (+manifest) | 93 | 16 public repos (see below) | mixed, **9 repos unlicensed** | partly |
| `dataflow/`, `dsl/`, `wild-annot/` | 10 | 10 | authored by us | Apache-2.0 | yes |
| `examples/` | 1 (+README) | 1 | authored by us (paper's running example, Fig. 1) | Apache-2.0 | yes |
| `gold-labels*.json`, `manifest.json` | 3 | 2 | authored by us (labels over the `asl/` corpus) | Apache-2.0 | yes |

MIT-0 is MIT without the attribution clause, so the AWS sample workflows carry no
redistribution obligation. Apache-2.0 and MIT do require that the copyright and licence
notice travel with the files; the notices are in the "Attribution" section below.

## Per-source detail

### `asl/` — 193 files, 169 distinct

Mined from two AWS sample collections, both MIT-0:

- `aws-samples/step-functions-workflows-collection` — 170 files
- `aws-samples/aws-stepfunctions-examples` — 23 files

`manifest.json` records the upstream repository and path for every file.

**Duplicates.** 20 groups of byte-identical files account for 24 redundant copies, so the
193 files contain 169 distinct definitions. This is a property of the upstream
collections, which ship the same state machine under SAM, CDK and Terraform variants (for
example `lambda-orchestration` appears four times and `saga-pattern` three times). We
mined every parseable definition rather than de-duplicating, so the counts reported in the
paper are per file.

### `aws-templates/` — 98 files, 97 distinct

Deployment artifacts (SAM/CloudFormation templates plus their sibling ASL files) from
`aws-samples/serverless-patterns`, MIT-0. Analysed directly by the CloudFormation front
end.

### `aws-solutions/` — 25 files

Production definitions from the AWS Solutions Library, all Apache-2.0:

| Solution | Files |
|---|---:|
| `aws-solutions/media2cloud` | 16 |
| `aws-solutions/distributed-load-testing-on-aws` | 3 |
| `aws-solutions/account-assessment-for-aws-organizations` | 1 |
| `aws-solutions/automated-security-response-on-aws` | 1 |
| `aws-solutions/instance-scheduler-on-aws` | 1 |
| `aws-solutions/network-orchestration-for-aws-transit-gateway` | 1 |
| Customizations for AWS Control Tower | 1 |

### `cncf/`, `cncf-typed/` — 66 + 4 files

CNCF Serverless Workflow examples from `serverlessworkflow/specification` (Apache-2.0).
One pair inside `cncf/` is byte-identical upstream. `cncf-typed/` is authored by us.

### `industrial/` — 6 workflows, 5 distinct

Five `aws-samples` patterns plus the Serverless Airline Booking `ProcessBooking` machine
extracted from its SAM template, all MIT-0. See `industrial/README.md` for the extraction
procedure.

**Duplicate.** `aws-saga-cdk.asl.json` is byte-identical to
`aws-saga-travel-reservation.asl.json`: the CDK and SAM variants of the upstream
`saga-pattern` sample produce the same definition, so this set contains six files but
five distinct workflows.

### `realbugs/` — mined fix-commit pairs

| Repository | Files | Licence |
|---|---:|---|
| `allenheltondev/serverless-ai-fitness` | 4 | MIT |
| `aws-samples/aws-batch-runtime-monitoring` | 2 | MIT-0 |
| `sparameswaran/airway-shipment-orchestrator` | 2 | MIT-0 |
| `manikanta5827/leave-management` | 2 | **none** |
| `nicktodd/video-translation-stepfunctions` | 2 | **none** |
| `demo-missingfield-*` | 2 | authored by us (synthetic demonstrator) |

`realbugs/issues/` holds the issue-tracker cases. `campus-compute-32-REAL-cdk-synth.asl.json`
is a real `cdk synth` extraction from `scttfrdmn/campus-compute` (Apache-2.0);
`coffee-workshop-56-*` derives from `aws-samples/serverless-coffee-workshop` (MIT-0); the
remaining files are minimal reproductions we authored from the structure described in the
public issue, not upstream copies. `eval/issue-tracker-defects.json` records each issue
URL and the verbatim reported error.

### `wild-external/` — 95 files, 93 distinct, 16 repositories

| Repository | Files | Licence |
|---|---:|---|
| `vdaron/StatesLanguage` | 15 | Apache-2.0 |
| `skyflow-workflow/skyflow_backend` | 15 | Apache-2.0 |
| `wmfs/statebox` | 15 | MIT |
| `ChristopheBougere/asl-validator` | 15 | Apache-2.0 |
| `mugglmenzel/step-functions-example-workflow` | 1 | Apache-2.0 |
| `yskszk63/sam-local-asl` | 1 | MIT |
| `aws-iot-builder-tools/iot-workflow-management-and-execution` | 1 | MIT-0 |
| `Thrubit/freight-booking-workflow` | 4 | **none** |
| `Thrubit/credit-card-transaction-workflow` | 4 | **none** |
| `Thrubit/launch-vehicle-manufacturing-workflow` | 4 | **none** |
| `Thrubit/network-outage-management-workflow` | 4 | **none** |
| `Thrubit/vehicle-order-fulfillment-workflow` | 4 | **none** |
| `Thrubit/vehicle-recall-management-workflow` | 4 | **none** |
| `Thrubit/mission-control-operations-workflow` | 4 | **none** |
| `Thrubit/payment-settlement-workflow` | 3 | **none** |
| `pssolanki111/pyDelhi_step_functions` | 1 | **none** |

`wild-external/manifest.json` records the repository, branch and path of every file.

## Files whose upstream publishes no licence

Thirty-six files come from repositories that are public but carry no licence file, which
means all rights are reserved and we have no redistribution grant: 32 in `wild-external/`
(the eight `Thrubit` repositories and `pssolanki111/pyDelhi_step_functions`) and 4 in
`realbugs/` (`manikanta5827/leave-management` and
`nicktodd/video-translation-stepfunctions`).

These files are therefore **not redistributed in the public artifact**. The manifests keep
their repository, branch and path, and `eval/mine_wild.js` and `eval/mine_realbugs.js`
re-fetch them from upstream, so the affected measurements remain reproducible with network
access. The measurements reported in the paper were computed over the full set as mined.

## Attribution

The Apache-2.0 and MIT sources above require that their copyright and licence notices
accompany redistribution:

- AWS Solutions Library solutions: Copyright Amazon.com, Inc. or its affiliates,
  Apache License 2.0.
- CNCF Serverless Workflow specification examples: Copyright The Serverless Workflow
  Specification Authors, Apache License 2.0.
- `vdaron/StatesLanguage`, `skyflow-workflow/skyflow_backend`,
  `ChristopheBougere/asl-validator`, `mugglmenzel/step-functions-example-workflow`,
  `scttfrdmn/campus-compute`: Apache License 2.0, copyright their respective authors.
- `wmfs/statebox`, `yskszk63/sam-local-asl`, `allenheltondev/serverless-ai-fitness`:
  MIT License, copyright their respective authors.
- AWS sample repositories (`aws-samples/*`, `aws-iot-builder-tools/*`): Copyright
  Amazon.com, Inc. or its affiliates, MIT-0.

Full licence texts are available at the upstream repositories linked above; Apache-2.0 is
also reproduced in this repository's `LICENSE`.
