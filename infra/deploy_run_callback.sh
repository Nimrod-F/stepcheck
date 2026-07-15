#!/usr/bin/env bash
# E7c -- AWS round-trip, LIVE CALLBACK: deploy the StepCheck-verified order-callback
# ASL (a bounded .waitForTaskToken approval gate; StepCheck raises no SC6001) as a
# STANDARD state machine (Express cannot run .waitForTaskToken at all), start it,
# let it PAUSE at the callback, then act as the EXTERNAL actor and resume it with
# SendTaskSuccess, driving it to SUCCEEDED. Reuses two pre-existing IAM roles.
# Teardown is infra/teardown.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

REGION=eu-central-1
ACCT=${ACCT:-$(aws sts get-caller-identity --query Account --output text)}
LAMBDA_ROLE="arn:aws:iam::${ACCT}:role/lambda-basic-role"
SFN_ROLE="arn:aws:iam::${ACCT}:role/TesseraBenchmark-StepFunctions-Role"
FN=stepcheck-demo-echo-cb
SM=stepcheck-demo-order-callback
export AWS_DEFAULT_REGION=$REGION

echo "### 0. clean any leftovers from a previous run"
SM_ARN_OLD=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
[ "$SM_ARN_OLD" != "None" ] && [ -n "$SM_ARN_OLD" ] && aws stepfunctions delete-state-machine --state-machine-arn "$SM_ARN_OLD" || true
aws lambda delete-function --function-name "$FN" 2>/dev/null || true

echo "### 1. package + create the echo Lambda (the approval notifier stand-in)"
mkdir -p infra/lambda
printf 'exports.handler = async (event) => ({ ...event, processedBy: "stepcheck-demo" });\n' > infra/lambda/index.js
( cd infra/lambda && zip -q -j ../echo.zip index.js )
LAMBDA_ARN=$(aws lambda create-function --function-name "$FN" \
  --runtime nodejs20.x --role "$LAMBDA_ROLE" --handler index.handler \
  --zip-file fileb://infra/echo.zip --timeout 10 \
  --query FunctionArn --output text)
aws lambda wait function-active-v2 --function-name "$FN"
echo "    lambda: $LAMBDA_ARN"

echo "### 2. take the StepCheck-VERIFIED callback ASL and bind FunctionName -> the lambda ARN (no other edits)"
node -e '
const fs=require("fs"); const arn=process.argv[1];
const wf=JSON.parse(fs.readFileSync("corpus/dsl/order-callback.asl.json"));
function walk(states){ for(const s of Object.values(states||{})){ if(!s||typeof s!=="object")continue;
  if(s.Type==="Task" && s.Parameters && "FunctionName" in s.Parameters) s.Parameters.FunctionName=arn;
  if(s.Iterator)walk(s.Iterator.States); if(s.ItemProcessor)walk(s.ItemProcessor.States);
  if(Array.isArray(s.Branches))s.Branches.forEach(b=>walk(b.States)); } }
walk(wf.States);
fs.writeFileSync("infra/order.deploy.callback.asl.json", JSON.stringify(wf,null,2));
' "$LAMBDA_ARN"
echo "    wrote infra/order.deploy.callback.asl.json"

echo "### 3. create a STANDARD state machine (callback/.waitForTaskToken is Standard-only)"
SM_ARN=$(aws stepfunctions create-state-machine --name "$SM" \
  --definition file://infra/order.deploy.callback.asl.json --role-arn "$SFN_ROLE" --type STANDARD \
  --query stateMachineArn --output text)
echo "    state machine: $SM_ARN"

echo "### 4. start the execution; it will PAUSE at RequestApproval (.waitForTaskToken)"
EXEC_ARN=$(aws stepfunctions start-execution --state-machine-arn "$SM_ARN" \
  --input '{"customerId":"c-001","items":["sku-42"],"orderId":"o-001","amount":42}' \
  --query executionArn --output text)
echo "    execution: $EXEC_ARN"

echo "### 5. poll history until the callback task is scheduled, then extract its task token"
TOKEN=""
for i in $(seq 1 60); do
  aws stepfunctions get-execution-history --execution-arn "$EXEC_ARN" --max-items 1000 \
    > infra/.cb_history_tmp.json 2>/dev/null || true
  TOKEN=$(node -e '
    const fs=require("fs");
    let ev=[]; try{ ev=(JSON.parse(fs.readFileSync("infra/.cb_history_tmp.json")).events)||[]; }catch(e){}
    for(const e of ev){ const d=e.taskScheduledEventDetails; if(!d||!d.parameters)continue;
      try{ const p=JSON.parse(d.parameters); const t=p&&p.Payload&&p.Payload.TaskToken; if(t){process.stdout.write(t);break;} }catch(_){}}
  ')
  [ -n "$TOKEN" ] && break
  sleep 1
done
if [ -z "$TOKEN" ]; then echo "!! never observed a task token -- aborting"; exit 1; fi
PAUSED=$(aws stepfunctions describe-execution --execution-arn "$EXEC_ARN" --query status --output text)
echo "    callback pending; execution status while waiting = $PAUSED"
echo "    task token (truncated): ${TOKEN:0:24}..."

echo "### 6. EXTERNAL actor resumes the paused workflow via SendTaskSuccess"
aws stepfunctions send-task-success --task-token "$TOKEN" \
  --task-output '{"approved":true,"approver":"external-reviewer","channel":"out-of-band-callback"}'
echo "    SendTaskSuccess accepted"

echo "### 7. poll describe-execution until the execution leaves RUNNING"
STATUS=RUNNING
for i in $(seq 1 60); do
  STATUS=$(aws stepfunctions describe-execution --execution-arn "$EXEC_ARN" --query status --output text)
  [ "$STATUS" != "RUNNING" ] && break
  sleep 1
done
aws stepfunctions describe-execution --execution-arn "$EXEC_ARN" > infra/execution-evidence-callback.json
aws stepfunctions get-execution-history --execution-arn "$EXEC_ARN" --max-items 1000 \
  > infra/execution-history-callback.json
rm -f infra/.cb_history_tmp.json
NEVENTS=$(node -e 'console.log((JSON.parse(require("fs").readFileSync("infra/execution-history-callback.json")).events||[]).length)')

echo
echo "=========================================================="
echo " MODE             : STANDARD + live callback (.waitForTaskToken)"
echo " PAUSED AT        : RequestApproval (status while waiting = $PAUSED)"
echo " RESUMED BY       : external SendTaskSuccess"
echo " FINAL STATUS     : $STATUS"
echo " HISTORY EVENTS   : $NEVENTS"
echo " STATE MACHINE    : $SM_ARN"
echo "=========================================================="
echo "$SM_ARN" > infra/.last_sm_arn_callback
