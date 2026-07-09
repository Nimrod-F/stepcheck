#!/usr/bin/env bash
# Delete every resource created by deploy_run.sh, deploy_run_standard.sh, and
# deploy_run_callback.sh. Pre-existing IAM roles are left untouched (we only
# reused them).
set -uo pipefail
cd "$(dirname "$0")/.."
export AWS_DEFAULT_REGION=eu-central-1

SMS="stepcheck-demo-order stepcheck-demo-order-standard stepcheck-demo-order-callback"
FNS="stepcheck-demo-echo stepcheck-demo-echo-std stepcheck-demo-echo-cb"

for SM in $SMS; do
  SM_ARN=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
  if [ "$SM_ARN" != "None" ] && [ -n "$SM_ARN" ]; then
    aws stepfunctions delete-state-machine --state-machine-arn "$SM_ARN" && echo "deleted state machine $SM_ARN"
  fi
done

for FN in $FNS; do
  aws lambda delete-function --function-name "$FN" 2>/dev/null && echo "deleted lambda $FN" || echo "lambda $FN already gone"
done

echo "teardown complete (IAM roles lambda-basic-role / TesseraBenchmark-StepFunctions-Role left intact)"
