# Running example (paper, Fig. 1)

`running-example.asl.json` is the order-processing workflow drawn in Fig. 1 of the
StepCheck paper (ICSOC 2026), written out as a deployable ASL definition so the
figure's claims can be re-checked.

The figure draws the two DynamoDB writes as dashed edges from `ChargeCard` and
`ScreenFraud` to `orders[id]`. Here they are explicit `dynamodb:putItem` states
(`RecordPayment`, `RecordScreening`) at the end of each `Parallel` branch, and the
`Parallel` state itself is named `Fulfil`. Otherwise the states and edges match the
figure.

## Defects

| Marker | Defect | Where | Check | Needs |
| --- | --- | --- | --- | --- |
| (1) | reads `$.total`, which no state produces (`ReserveInventory` closes the document to `{orderId, amount}`) | `ChargeCard` | `SC1101` (error) | the artifact only |
| (2) | ship before pay | one-edge mutation, not in this file | `SC2001`/`SC1010` | declared typestate/contracts |
| (3) | `States.ALL` retry of a non-idempotent charge | `ChargeCard` | `SC3001` | idempotency fact |
| (4) | both `Parallel` branches overwrite table `orders` | `Fulfil` | `SC5001` | persistence fact for the writes |
| (5) | callback without `TimeoutSeconds`/`HeartbeatSeconds` | `AwaitApproval` | `SC6001` | the artifact only |
| (6) | reservation with no compensation | `ReserveInventory` | `SC4001` | persistence/compensation fact |

`SC5001` counts a task as a write only when it is known to be non-idempotent or
persistent, so an unannotated `putItem` is not flagged. That is why (4) needs an
effect fact even though the shared table is named in the artifact.

## Reproduce

```sh
statelint running-example.asl.json                    # passes
asl-validator --json-path running-example.asl.json    # passes
aws stepfunctions validate-state-machine-definition --type STANDARD --definition file://running-example.asl.json   # result: OK
stepcheck check running-example.asl.json              # native: 1 error, 1 warning
stepcheck check running-example.asl.json --infer      # inference: 1 error, 9 warnings
```

Native mode reports (1) as `SC1101` and (5) as `SC6001`. With `--infer`, facts read
from task names (`charge`, `reserve`, `putItem`, ...) add (3) `SC3001`, (4) `SC5001`
and (6) `SC4001`, all as warnings. Inference also flags the other durable tasks
(`CreateOrder`, `ChargeCard`, `RecordPayment`, `RecordScreening`, `ShipOrder`) with
`SC4001`, since none of them has a compensating path. Declared facts in a sidecar
(see `../dsl/order.sidecar.toml` for the format) replace these name-based guesses.

Checked on 24 Sep 2026 with statelint (Ruby 3.3), asl-validator, AWS's
`ValidateStateMachineDefinition` (eu-central-1, `STANDARD` type: `OK`, no diagnostics)
and StepCheck 0.1.5 built from this repository. The workflow must be deployed as a
Standard state machine: Express rejects the `.waitForTaskToken` callback in
`AwaitApproval`.
