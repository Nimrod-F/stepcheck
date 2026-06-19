#!/usr/bin/env bash
# Delete every resource created by deploy_run.sh. Pre-existing IAM roles are left
# untouched (we only reused them).
set -uo pipefail
cd "$(dirname "$0")/.."
export AWS_DEFAULT_REGION=eu-central-1
SM=stepcheck-demo-order
FN=stepcheck-demo-echo

SM_ARN=$(aws stepfunctions list-state-machines --query "stateMachines[?name=='${SM}'].stateMachineArn | [0]" --output text 2>/dev/null || echo None)
if [ "$SM_ARN" != "None" ] && [ -n "$SM_ARN" ]; then
  aws stepfunctions delete-state-machine --state-machine-arn "$SM_ARN" && echo "deleted state machine $SM_ARN"
fi
aws lambda delete-function --function-name "$FN" 2>/dev/null && echo "deleted lambda $FN" || echo "lambda already gone"
echo "teardown complete (IAM roles lambda-basic-role / TesseraBenchmark-StepFunctions-Role left intact)"
