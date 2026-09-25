# Industrial-topology workflow set

Real-topology, service-grade workflows used for the build-gate and cost study (RQ4). This
set is distinct from the private automotive case study, which is not in this repository. Every
file is a real `aws-samples` production state machine; the AWS Serverless Airline
Booking machine is extracted from its SAM template (see below), the rest are
verbatim copies of patterns already in `corpus/asl/`, kept here so the industrial
evaluation is a self-contained, clearly-scoped set.

| File | Provenance | Topology |
|---|---|---|
| `aws-saga-travel-reservation.asl.json` | aws-samples `saga-pattern` (SAM) | 12-state reserve→pay→confirm saga, retry on every forward task, 3-step compensation chain |
| `aws-saga-cdk.asl.json` | aws-samples `saga-pattern` (CDK) | same saga, CDK-generated |
| `aws-inventory-reserve-stock.asl.json` | aws-samples `inventory-management` (SAM) | reserve-stock saga with compensation |
| `aws-checkout-processing.asl.json` | aws-samples `checkout-processing-workflow` | 12-state e-commerce checkout, Choice fan-out |
| `aws-etl-stream-aggregator.asl.json` | aws-samples `distributed-data-stream-aggregator` | Map-based ETL child workflow |
| `sab-booking-processbooking.asl.json` | **extracted** AWS Serverless Airline Booking `ProcessBooking` state machine | 12-state saga: Reserve Flight/Booking → Collect Payment → Confirm → Notify, with Release Seat / Cancel Booking / Refund Payment compensation and a Booking DLQ |

## How SAB is extracted (not reconstructed)

The upstream `aws-samples/aws-serverless-airline-booking` ships its booking state
machine as a SAM `DefinitionString: !Sub |` block in
`src/backend/booking/template.yaml` on the **`master`** branch (the `main` branch
holds only documentation and media, which is why an earlier pass missed it). We
extract that block verbatim and resolve the CloudFormation `${Fn.Arn}`/`${Fn}`
placeholders to literal Lambda ARNs; the control flow, retries, catches, and
payload bindings are the deployed ones, unchanged. Reproduce:

```
curl -sL https://raw.githubusercontent.com/aws-samples/aws-serverless-airline-booking/master/src/backend/booking/template.yaml
# take the DefinitionString block, substitute ${...} -> arn:aws:lambda:...:function:<name>
```

## Reproduce

```bash
# per-class detection vs the formal verifier (Woflan) on this set
python eval/asl2bpmn/compare.py --corpus corpus/industrial --out eval/industrial-comparison.json
# topology + clean findings + timing + CI-gate catch-rate
python eval/industrial/wse.py            # -> eval/industrial-case.json
```
