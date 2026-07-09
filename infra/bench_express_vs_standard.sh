#!/usr/bin/env bash
# E7d -- deployment runtime study. Runs the StepCheck-verified order workflow N
# times each as an Express (synchronous) and a Standard (asynchronous, durable)
# state machine, and reports the per-mode execution-duration distribution plus one
# runtime log per mode. This is NOT an AWS performance benchmark: it substantiates
# that a verified artifact completes successfully and with stable latency across
# both AWS execution models at scale. Reuses two pre-existing IAM roles. Cleans up
# all created resources at the end.
set -euo pipefail
cd "$(dirname "$0")/.."

REGION=eu-central-1
ACCT=683003725669
LAMBDA_ROLE="arn:aws:iam::${ACCT}:role/lambda-basic-role"
SFN_ROLE="arn:aws:iam::${ACCT}:role/TesseraBenchmark-StepFunctions-Role"
FN=stepcheck-bench-echo
SM_E=stepcheck-bench-express
SM_S=stepcheck-bench-standard
N=${1:-100}
INPUT='{"customerId":"c-001","items":["sku-42"],"orderId":"o-001","amount":42}'
export AWS_DEFAULT_REGION=$REGION

# CreateStateMachine is rejected while a same-named machine is still DELETING
# (deletion is asynchronous); retry until the name frees up.
create_sm() {
  local name="$1" type="$2" arn
  for _ in $(seq 1 60); do
    if arn=$(aws stepfunctions create-state-machine --name "$name" \
        --definition file://infra/order.deploy.bench.asl.json --role-arn "$SFN_ROLE" --type "$type" \
        --query stateMachineArn --output text 2>/tmp/sm_err); then
      echo "$arn"; return 0
    fi
    grep -q "StateMachineDeleting\|StateMachineAlreadyExists" /tmp/sm_err || { cat /tmp/sm_err >&2; return 1; }
    sleep 2
  done
  echo "create_sm timed out waiting for $name" >&2; return 1
}

echo "### 0. clean leftovers"
for SM in "$SM_E" "$SM_S"; do
  A=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
  [ "$A" != "None" ] && [ -n "$A" ] && aws stepfunctions delete-state-machine --state-machine-arn "$A" || true
done
aws lambda delete-function --function-name "$FN" 2>/dev/null || true

echo "### 1. echo Lambda"
mkdir -p infra/lambda
printf 'exports.handler = async (event) => ({ ...event, processedBy: "stepcheck-demo" });\n' > infra/lambda/index.js
( cd infra/lambda && zip -q -j ../echo.zip index.js )
LAMBDA_ARN=$(aws lambda create-function --function-name "$FN" \
  --runtime nodejs20.x --role "$LAMBDA_ROLE" --handler index.handler \
  --zip-file fileb://infra/echo.zip --timeout 10 --query FunctionArn --output text)
aws lambda wait function-active-v2 --function-name "$FN"
echo "    lambda: $LAMBDA_ARN"

echo "### 2. bind the verified order.asl.json to the lambda"
node -e '
const fs=require("fs"); const arn=process.argv[1];
const wf=JSON.parse(fs.readFileSync("corpus/dsl/order.asl.json"));
function walk(states){ for(const s of Object.values(states||{})){ if(!s||typeof s!=="object")continue;
  if(s.Type==="Task" && s.Parameters && "FunctionName" in s.Parameters) s.Parameters.FunctionName=arn;
  if(s.Iterator)walk(s.Iterator.States); if(s.ItemProcessor)walk(s.ItemProcessor.States);
  if(Array.isArray(s.Branches))s.Branches.forEach(b=>walk(b.States)); } }
walk(wf.States);
fs.writeFileSync("infra/order.deploy.bench.asl.json", JSON.stringify(wf,null,2));
' "$LAMBDA_ARN"

echo "### 3. create both state machines from the SAME verified definition"
E_ARN=$(create_sm "$SM_E" EXPRESS)
S_ARN=$(create_sm "$SM_S" STANDARD)
echo "    express : $E_ARN"
echo "    standard: $S_ARN"

echo "### 4. Express: $N synchronous executions"
: > infra/.bench_express.tsv
for i in $(seq 1 "$N"); do
  # command substitution strips the Windows CR/trailing newline; echo re-adds a clean LF
  L=$(aws stepfunctions start-sync-execution --state-machine-arn "$E_ARN" --input "$INPUT" \
    --query '[status,startDate,stopDate,billingDetails.billedDurationInMilliseconds]' --output text)
  echo "$L" >> infra/.bench_express.tsv
done
echo "    done ($(wc -l < infra/.bench_express.tsv) runs)"

echo "### 5. Standard: start $N async executions, then poll each to completion"
: > infra/.bench_std_arns.txt
for i in $(seq 1 "$N"); do
  EX=$(aws stepfunctions start-execution --state-machine-arn "$S_ARN" --input "$INPUT" \
    --query executionArn --output text)
  echo "$EX" >> infra/.bench_std_arns.txt
done
: > infra/.bench_standard.tsv
while read -r EX; do
  EX=$(echo "$EX" | tr -d '\r')
  for t in $(seq 1 120); do
    ST=$(aws stepfunctions describe-execution --execution-arn "$EX" --query status --output text)
    [ "$ST" != "RUNNING" ] && break
    sleep 0.5
  done
  L=$(aws stepfunctions describe-execution --execution-arn "$EX" \
    --query '[status,startDate,stopDate]' --output text)
  echo "$L" >> infra/.bench_standard.tsv
done < infra/.bench_std_arns.txt
echo "    done ($(wc -l < infra/.bench_standard.tsv) runs)"

echo "### 6. capture one runtime log per mode + state-transition count"
FIRST_STD=$(head -1 infra/.bench_std_arns.txt)
aws stepfunctions get-execution-history --execution-arn "$FIRST_STD" --max-items 1000 \
  > infra/bench-standard-history.json
aws stepfunctions start-sync-execution --state-machine-arn "$E_ARN" --input "$INPUT" \
  > infra/bench-express-sample.json

echo "### 7. aggregate"
node -e '
const fs=require("fs");
function pct(a,p){ if(!a.length) return null; const s=[...a].sort((x,y)=>x-y); const i=Math.min(s.length-1,Math.ceil(p/100*s.length)-1); return s[Math.max(0,i)]; }
function stat(a){ const n=a.length, sum=a.reduce((x,y)=>x+y,0);
  return {n, mean:+(sum/n).toFixed(1), min:Math.min(...a), p50:pct(a,50), p95:pct(a,95), p99:pct(a,99), max:Math.max(...a)}; }
function readTsv(f){ return fs.readFileSync(f,"utf8").trim().split(/\r?\n/).filter(Boolean).map(l=>l.split("\t")); }
const ex=readTsv("infra/.bench_express.tsv");   // status,start,stop,billed
const st=readTsv("infra/.bench_standard.tsv");  // status,start,stop
const exDur=ex.map(r=>Date.parse(r[2])-Date.parse(r[1]));
const exBilled=ex.map(r=>parseInt(r[3],10)).filter(x=>!isNaN(x));
const stDur=st.map(r=>Date.parse(r[2])-Date.parse(r[1]));
const exOk=ex.filter(r=>r[0]==="SUCCEEDED").length;
const stOk=st.filter(r=>r[0]==="SUCCEEDED").length;
const hist=JSON.parse(fs.readFileSync("infra/bench-standard-history.json")).events||[];
const transitions=hist.filter(e=>/StateEntered/.test(e.type)).length;
const report={
  generated_by:"infra/bench_express_vs_standard.sh",
  workflow:"corpus/dsl/order.asl.json (StepCheck-verified, deployed unmodified)",
  region:"eu-central-1", executions_per_mode:ex.length,
  express:{ succeeded:exOk+"/"+ex.length, exec_duration_ms:stat(exDur), billed_duration_ms:stat(exBilled) },
  standard:{ succeeded:stOk+"/"+st.length, exec_duration_ms:stat(stDur), state_transitions_per_execution:transitions,
             durable_history_events_sample:hist.length },
  note:"Execution-duration distributions over N runs of one verified artifact deployed unmodified in each AWS execution model; reported as evidence of successful, stable completion across modes, not as a cross-mode AWS performance benchmark. Standard retains a durable audited history per execution; Express retains none."
};
fs.writeFileSync("eval/deploy-runtime-bench.json", JSON.stringify(report,null,2)+"\n");
console.log(JSON.stringify(report,null,2));
'

echo "### 8. teardown"
for A in "$E_ARN" "$S_ARN"; do aws stepfunctions delete-state-machine --state-machine-arn "$A" && echo "    deleted $A"; done
aws lambda delete-function --function-name "$FN" && echo "    deleted $FN"
rm -f infra/.bench_express.tsv infra/.bench_standard.tsv infra/.bench_std_arns.txt
echo "### done. report: eval/deploy-runtime-bench.json"
