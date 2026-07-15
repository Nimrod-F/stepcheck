#!/usr/bin/env bash
# E7b -- AWS round-trip, STANDARD mode: deploy the StepCheck-verified ASL to real
# AWS Step Functions as a STANDARD state machine, run it asynchronously, poll to
# SUCCEEDED, and capture the full durable execution history (the audited event log
# that Standard retains and Express does not). Reuses two pre-existing IAM roles
# (no role creation). Teardown is infra/teardown.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

REGION=eu-central-1
ACCT=${ACCT:-$(aws sts get-caller-identity --query Account --output text)}
LAMBDA_ROLE="arn:aws:iam::${ACCT}:role/lambda-basic-role"
SFN_ROLE="arn:aws:iam::${ACCT}:role/TesseraBenchmark-StepFunctions-Role"
FN=stepcheck-demo-echo-std
SM=stepcheck-demo-order-standard
export AWS_DEFAULT_REGION=$REGION

echo "### 0. clean any leftovers from a previous run"
SM_ARN_OLD=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
[ "$SM_ARN_OLD" != "None" ] && [ -n "$SM_ARN_OLD" ] && aws stepfunctions delete-state-machine --state-machine-arn "$SM_ARN_OLD" || true
aws lambda delete-function --function-name "$FN" 2>/dev/null || true

echo "### 1. package + create the echo Lambda (stands in for the step tasks)"
mkdir -p infra/lambda
printf 'exports.handler = async (event) => ({ ...event, processedBy: "stepcheck-demo" });\n' > infra/lambda/index.js
( cd infra/lambda && zip -q -j ../echo.zip index.js )
LAMBDA_ARN=$(aws lambda create-function --function-name "$FN" \
  --runtime nodejs20.x --role "$LAMBDA_ROLE" --handler index.handler \
  --zip-file fileb://infra/echo.zip --timeout 10 \
  --query FunctionArn --output text)
aws lambda wait function-active-v2 --function-name "$FN"
echo "    lambda: $LAMBDA_ARN"

echo "### 2. take the StepCheck-VERIFIED ASL and bind FunctionName -> the lambda ARN (no other edits)"
node -e '
const fs=require("fs"); const arn=process.argv[1];
const wf=JSON.parse(fs.readFileSync("corpus/dsl/order.asl.json"));
function walk(states){ for(const s of Object.values(states||{})){ if(!s||typeof s!=="object")continue;
  if(s.Type==="Task" && s.Parameters && "FunctionName" in s.Parameters) s.Parameters.FunctionName=arn;
  if(s.Iterator)walk(s.Iterator.States); if(s.ItemProcessor)walk(s.ItemProcessor.States);
  if(Array.isArray(s.Branches))s.Branches.forEach(b=>walk(b.States)); } }
walk(wf.States);
fs.writeFileSync("infra/order.deploy.standard.asl.json", JSON.stringify(wf,null,2));
' "$LAMBDA_ARN"
echo "    wrote infra/order.deploy.standard.asl.json"

echo "### 3. create a STANDARD state machine from the verified definition"
SM_ARN=$(aws stepfunctions create-state-machine --name "$SM" \
  --definition file://infra/order.deploy.standard.asl.json --role-arn "$SFN_ROLE" --type STANDARD \
  --query stateMachineArn --output text)
echo "    state machine: $SM_ARN"

echo "### 4. start an ASYNCHRONOUS (durable) Standard execution"
EXEC_ARN=$(aws stepfunctions start-execution --state-machine-arn "$SM_ARN" \
  --input '{"customerId":"c-001","items":["sku-42"],"orderId":"o-001","amount":42}' \
  --query executionArn --output text)
echo "    execution: $EXEC_ARN"

echo "### 5. poll describe-execution until the execution leaves RUNNING"
STATUS=RUNNING
for i in $(seq 1 60); do
  STATUS=$(aws stepfunctions describe-execution --execution-arn "$EXEC_ARN" --query status --output text)
  [ "$STATUS" != "RUNNING" ] && break
  sleep 1
done
aws stepfunctions describe-execution --execution-arn "$EXEC_ARN" > infra/execution-evidence-standard.json

echo "### 6. capture the durable, audited event history (Standard-only artifact)"
aws stepfunctions get-execution-history --execution-arn "$EXEC_ARN" --max-items 1000 \
  > infra/execution-history-standard.json
NEVENTS=$(node -e 'console.log((JSON.parse(require("fs").readFileSync("infra/execution-history-standard.json")).events||[]).length)')

echo
echo "=========================================================="
echo " MODE             : STANDARD (durable, async)"
echo " EXECUTION STATUS : $STATUS"
echo " HISTORY EVENTS   : $NEVENTS (durable audited log; Express retains none)"
echo " STATE MACHINE    : $SM_ARN"
echo "=========================================================="
echo "$SM_ARN" > infra/.last_sm_arn_standard
