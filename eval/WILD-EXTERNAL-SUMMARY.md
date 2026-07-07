# External-validity mining: independent repositories (beyond aws-samples)

Reproduce: `node eval/mine_wild.js` then
`stepcheck scan corpus/wild-external --infer` and
`stepcheck path-coverage corpus/asl corpus/cncf corpus/wild-external`.

## Corpus
- 95 ASL workflows from 16 public repos OUTSIDE the three aws-samples collections.
- 50 domain workflows (13 repos): skyflow_backend, iot firmware mgmt, 8 Thrubit
  domain repos (freight/credit-card/payment/manufacturing/recall/outage/order/mission),
  sam-local-asl, step-functions-example-workflow, pyDelhi_step_functions.
- 45 library/validator fixtures (3 repos): StatesLanguage, statebox, asl-validator.

## Findings (stepcheck scan --infer)
- SC1101 x1 (SOUND, error): Thrubit/credit-card-transaction-workflow, transaction-reversal.
    Map `ApplyReversalEntries` iterates ItemsPath `$.ledgerEntries`; `SetReversalDefaults`
    (Pass) reconstructs the document via Parameters into a closed 7-field record that drops
    `ledgerEntries`, so no state on any path produces it -> runtime ItemsPath failure.
    Flow-sensitive miss no adjacent-field check sees.
- SC5001 x1 (concurrency warning): aws-iot-builder-tools firmware_upgrade: 3 Parallel
    branches all write DynamoDB table `FWUTaskTokens` (last-writer-wins interference).
- SC6001, SC0007, SC0003: on validator fixtures (some are by-design invalid; excluded from claims).
- Inferred semantic warnings on domain workflows: SC3001 x10, SC4001 x18, SC4010 x13.

## Declared-tier wild evidence (corpus/wild-annot/payment-settlement-pipeline.toml)
Payment-settlement pipeline (independent). Declaring the payment authorization and
settlement as persistent (they are) promotes the finding to declared tier:
- SC4001 ERROR on FlagFraudulentPayment (durable effect, no compensation).
- SC4010 on AuthorizePayment and SettlePayment: if SettlePayment fails after
  AuthorizePayment succeeds, the authorization hold is never voided (leaked effect).

## Modeled-fragment coverage (stepcheck path-coverage)
Field-reads = `.$` operands of Parameters/ItemSelector + Map ItemsPath.
| corpus | reads | precise (dotted+$) | of DOCUMENT reads | complex | ctx ($$) | intrinsic |
|--------|------:|-------------------:|------------------:|--------:|---------:|----------:|
| aws (193) | 1168 | 740 (63.4%) | 740/803 = 92.2% | 63 (7.8%) | 225 | 140 |
| cncf (66) | 2 | 0 | n/a (runtime expr) | 0 | 0 | 2 |
| wild (95) | 420 | 353 (84.0%) | 353/370 = 95.4% | 17 (4.6%) | 20 | 30 |
| COMBINED | 1590 | 1093 | 1093/1173 = 93.2% | 80 (6.8%) | 245 | 172 |

Interpretation: of the JSONPath reads that are genuine document-field references,
92% (AWS) / 93% (combined) fall in the modeled dotted fragment; only ~7-8% use
bracket/wildcard/filter syntax abstracted to Maybe. The `$$` context-object and
`States.*` intrinsic operands are not document references and cannot miss a field.
