# External-validity mining: independent repositories (beyond aws-samples)

Reproduce: `node eval/mine_wild.js` then
`stepcheck scan corpus/wild-external --infer` and
`stepcheck path-coverage corpus/asl corpus/cncf corpus/wild-external`.

## Corpus
- 95 ASL workflows from 16 public repos OUTSIDE the three aws-samples collections.
- NOTE: 32 of those 95 (the 8 `Thrubit/*` repos and `pssolanki111/pyDelhi_step_functions`) come from
  repositories that publish no licence, so they are NOT redistributed in this artifact; re-fetch them
  with `node eval/mine_wild.js` before re-running the scan. Findings below were computed over all 95.
  See `corpus/PROVENANCE.md`.
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
| aws (193) | 1168 | 742 (63.5%) | 742/803 = 92.4% | 61 (7.6%) | 225 | 140 |
| cncf (66) | 17 | 15 (88.2%) | 15/15 (jq field reads) | 0 | 0 | 2 |
| wild (95) | 420 | 353 (84.0%) | 353/370 = 95.4% | 17 (4.6%) | 20 | 30 |
| COMBINED | 1605 | 1110 (69.2%) | 1110/1188 = 93.4% | 78 (6.6%) | 245 | 172 |

Interpretation: of the JSONPath reads that are genuine document-field references,
92% (AWS) / 93% (combined) fall in the modeled dotted fragment; only ~7% use
bracket/wildcard/filter syntax abstracted to Maybe. The `$$` context-object and
`States.*` intrinsic operands are not document references and cannot miss a field.
CNCF uses jq expressions; the 15 jq field reads StepCheck lowers all fall in the dotted fragment.
