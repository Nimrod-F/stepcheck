#!/usr/bin/env bash
# E7 — AWS round-trip: deploy the StepCheck-verified ASL to real AWS Step
# Functions, execute it, and capture evidence. Reuses two pre-existing IAM roles
# (no role creation). Teardown is infra/teardown.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

REGION=eu-central-1
ACCT=${ACCT:-$(aws sts get-caller-identity --query Account --output text)}
LAMBDA_ROLE="arn:aws:iam::${ACCT}:role/lambda-basic-role"
SFN_ROLE="arn:aws:iam::${ACCT}:role/TesseraBenchmark-StepFunctions-Role"
FN=stepcheck-demo-echo
SM=stepcheck-demo-order
export AWS_DEFAULT_REGION=$REGION

echo "### 0. clean any leftovers from a previous run"
SM_ARN_OLD=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
[ "$SM_ARN_OLD" != "None" ] && [ -n "$SM_ARN_OLD" ] && aws stepfunctions delete-state-machine --state-machine-arn "$SM_ARN_OLD" || true
aws lambda delete-function --function-name "$FN" 2>/dev/null || true

echo "### 1. package + create the echo Lambda (stands in for the 8 step functions)"
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
fs.writeFileSync("infra/order.deploy.asl.json", JSON.stringify(wf,null,2));
' "$LAMBDA_ARN"
echo "    wrote infra/order.deploy.asl.json"

echo "### 3. create an EXPRESS state machine from the verified definition"
SM_ARN=$(aws stepfunctions create-state-machine --name "$SM" \
  --definition file://infra/order.deploy.asl.json --role-arn "$SFN_ROLE" --type EXPRESS \
  --query stateMachineArn --output text)
echo "    state machine: $SM_ARN"

echo "### 4. run it (synchronous EXPRESS execution)"
aws stepfunctions start-sync-execution --state-machine-arn "$SM_ARN" \
  --input '{"customerId":"c-001","items":["sku-42"],"orderId":"o-001","amount":42}' \
  > infra/execution-evidence.json

STATUS=$(node -e 'console.log(JSON.parse(require("fs").readFileSync("infra/execution-evidence.json")).status)')
BILLED=$(node -e 'const j=JSON.parse(require("fs").readFileSync("infra/execution-evidence.json")).billingDetails||{};console.log((j.billedDurationInMilliseconds||"?")+"ms / "+(j.billedMemoryUsedInMB||"?")+"MB")' 2>/dev/null || echo "?")
echo
echo "=========================================================="
echo " EXECUTION STATUS : $STATUS"
echo " BILLED           : $BILLED"
echo " STATE MACHINE    : $SM_ARN"
echo "=========================================================="
echo "$SM_ARN" > infra/.last_sm_arn
