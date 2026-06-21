# Label signals for the 13 expansion workflows

Fill `idempotent` and `persistent` (`true` / `false` / `null`) for each task in
`gold-labels-human-A.json` and `gold-labels-human-B.json`, using these signals and
the rubric in HUMAN-GOLD-LABELLING.md. `null` = abstain (cannot decide).


## sfn-collection__sfn-rekognition-video-catalog-workflow__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Create/Update VideoContentCatalog | Task | startcrawler | arn:aws:states:::aws-sdk:glue:startCrawler |  |  |
| StartContentModeration | Task | startcontentmoderation | arn:aws:states:::aws-sdk:rekognition:startContentModeration |  |  |
| Wait for ContentModeration Callback | Task | waitfortasktoken | TableName=rekognition-job-tracker |  |  |
| GetContentModeration | Task | getcontentmoderation | arn:aws:states:::aws-sdk:rekognition:getContentModeration |  |  |
| Update ContentModeration Job status | Task | updateitem | TableName=rekognition-job-tracker |  |  |
| Write Rekognition Content Moderation Results to file | Task | invoke | FunctionName=arn:aws:lambda:{REGION}:{ACCOUNT-NUMBER}:functi |  |  |
| StartLabelDetection | Task | startlabeldetection | arn:aws:states:::aws-sdk:rekognition:startLabelDetection |  |  |
| Wait for LabelDetection Callback | Task | waitfortasktoken | TableName=rekognition-job-tracker |  |  |
| GetLabelDetection | Task | getlabeldetection | arn:aws:states:::aws-sdk:rekognition:getLabelDetection |  |  |
| Update Label Detection Status | Task | updateitem | TableName=rekognition-job-tracker |  |  |
| Write Rekognition Label Detection Results to File | Task | invoke | FunctionName=arn:aws:lambda:{REGION}:{ACCOUNT-NUMBER}:functi |  |  |
| StartSegmentDetection | Task | startsegmentdetection | arn:aws:states:::aws-sdk:rekognition:startSegmentDetection |  |  |
| Wait for SegmentDetection Callback | Task | waitfortasktoken | TableName=rekognition-job-tracker |  |  |
| GetSegmentDetection | Task | getsegmentdetection | arn:aws:states:::aws-sdk:rekognition:getSegmentDetection |  |  |
| Update Segment Detection Status | Task | updateitem | TableName=rekognition-job-tracker |  |  |
| Write Rekognition Segmet Results to File | Task | invoke | FunctionName=arn:aws:lambda:{REGION}:{ACCOUNT-NUMBER}:functi |  |  |
| MoveProcessedFiles | Task | invoke | FunctionName=arn:aws:lambda:{REGION}:{ACCOUNT-NUMBER}:functi |  |  |

## sfn-collection__ec2-instance-isolation-sam__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Get EC2 Instance Info | Task | describeinstances | arn:aws:states:::aws-sdk:ec2:describeInstances |  |  |
| Disable Instance Termination | Task | modifyinstanceattribute | arn:aws:states:::aws-sdk:ec2:modifyInstanceAttribute |  |  |
| Get AutoScalingGroup Info | Task | describeautoscalinginstances | arn:aws:states:::aws-sdk:autoscaling:describeAutoScalingInst |  |  |
| Detach Instance from ASG | Task | detachinstances | arn:aws:states:::aws-sdk:autoscaling:detachInstances |  |  |
| Create Forensic Instance | Task | runinstances | arn:aws:states:::aws-sdk:ec2:runInstances |  |  |
| Create Snapshot from Isolated Instance | Task | createsnapshot | arn:aws:states:::aws-sdk:ec2:createSnapshot |  |  |
| Get Snapshot Status | Task | describesnapshots | arn:aws:states:::aws-sdk:ec2:describeSnapshots |  |  |
| Create EBS Volume from Snapshot | Task | createvolume | arn:aws:states:::aws-sdk:ec2:createVolume |  |  |
| Get EBS Volume Status | Task | describevolumes | arn:aws:states:::aws-sdk:ec2:describeVolumes |  |  |
| Attach Volume | Task | attachvolume | arn:aws:states:::aws-sdk:ec2:attachVolume |  |  |
| Allow Forensic Instance Ingress | Task | authorizesecuritygroupingress | arn:aws:states:::aws-sdk:ec2:authorizeSecurityGroupIngress |  |  |
| Tag Instance as Quarantine | Task | createtags | arn:aws:states:::aws-sdk:ec2:createTags |  |  |

## sfn-collection__checkout-processing-workflow__statemachine__checkout_workflow.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Set Order, Payment and Shipping Status | Task | putitem | TableName=${DynamoName} |  |  |
| Third Party Payment Service HTTPS Endpoint | Task | waitfortasktoken | ApiEndpoint=${ApiGateWayEndpoint} |  |  |
| Set Payment Status: Unsuccessful | Task | updateitem | TableName=${DynamoName} |  |  |
| Send PaymentFailed Event to Notification Bus | Task | putevents | arn:aws:states:::events:putEvents |  |  |
| Set Payment Status: Successful | Task | updateitem | TableName=${DynamoName} |  |  |
| Third Party Shipping Service HTTPS Endpoint | Task | waitfortasktoken | ApiEndpoint=${ApiGateWayEndpoint} |  |  |
| Set Shipping Status: Successful | Task | updateitem | TableName=${DynamoName} |  |  |
| Set Shipping Status: Unsuccessful | Task | updateitem | TableName=${DynamoName} |  |  |
| Send ShippingFailed Event to Notification Bus | Task | putevents | arn:aws:states:::events:putEvents |  |  |
| Send ShippingSuccessful Event to Notification Bus | Task | putevents | arn:aws:states:::events:putEvents |  |  |

## sfn-collection__saga-pattern-tf__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| ReserveFlight | Task | invoke | FunctionName=${reserveFlightFunction} |  |  |
| ReserveCarRental | Task | invoke | FunctionName=${reserveCarRentalFunction} |  |  |
| ProcessPayment | Task | invoke | FunctionName=${processPaymentFunction} |  |  |
| ConfirmFlight | Task | invoke | FunctionName=${confirmFlightFunction} |  |  |
| ConfirmCarRental | Task | invoke | FunctionName=${confirmCarRentalFunction} |  |  |
| SendingSMSSuccess | Task | publish | TopicArn=${snsTopicArn} |  |  |
| RefundPayment | Task | invoke | FunctionName=${refundPaymentFunction} |  |  |
| CancelRentalReservation | Task | invoke | FunctionName=${cancelCarRentalFunction} |  |  |
| CancelFlightReservation | Task | invoke | FunctionName=${cancelFlightFunction} |  |  |
| SendingSMSFailure | Task | publish | TopicArn=${snsTopicArn} |  |  |

## sfn-examples__sam__demo-fis-stepfunctions__stepfunction__fis.ec2.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| EC2CPUStressExperimentTemplate | Task | createexperimenttemplate | arn:aws:states:::aws-sdk:fis:createExperimentTemplate |  |  |
| CPUStressInstances | Task | startexperiment | arn:aws:states:::aws-sdk:fis:startExperiment |  |  |
| GetExperiment | Task | getexperiment | arn:aws:states:::aws-sdk:fis:getExperiment |  |  |
| EC2StopExperimentTemplate | Task | createexperimenttemplate | arn:aws:states:::aws-sdk:fis:createExperimentTemplate |  |  |
| StopInstances | Task | startexperiment | arn:aws:states:::aws-sdk:fis:startExperiment |  |  |
| GetExperiment (1) | Task | getexperiment | arn:aws:states:::aws-sdk:fis:getExperiment |  |  |
| EC2TerminateExperimentTemplate | Task | createexperimenttemplate | arn:aws:states:::aws-sdk:fis:createExperimentTemplate |  |  |
| TerminateInstances | Task | startexperiment | arn:aws:states:::aws-sdk:fis:startExperiment |  |  |
| GetExperiment (2) | Task | getexperiment | arn:aws:states:::aws-sdk:fis:getExperiment |  |  |

## sfn-collection__sfn-rds-to-aurora-migrate-postgres-cdk__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Describe DB Instances | Task | describedbinstances | arn:aws:states:::aws-sdk:rds:describeDBInstances |  |  |
| Create Final Snapshot | Task | createdbsnapshot | arn:aws:states:::aws-sdk:rds:createDBSnapshot |  |  |
| Check DBSnapshot Status | Task | describedbsnapshots | arn:aws:states:::aws-sdk:rds:describeDBSnapshots |  |  |
| Restore DBCluster from Snapshot | Task | restoredbclusterfromsnapshot | arn:aws:states:::aws-sdk:rds:restoreDBClusterFromSnapshot |  |  |
| DescribeDBClusters | Task | describedbclusters | arn:aws:states:::aws-sdk:rds:describeDBClusters |  |  |
| Create Aurora Instance | Task | createdbinstance | arn:aws:states:::aws-sdk:rds:createDBInstance |  |  |
| DescribeDBInstances | Task | describedbinstances | arn:aws:states:::aws-sdk:rds:describeDBInstances |  |  |
| Modify Aurora Cluster | Task | modifydbcluster | arn:aws:states:::aws-sdk:rds:modifyDBCluster |  |  |

## sfn-examples__usecases__genai-prompt-chaining-hitl__src__workflow.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| StartTranscriptionJob | Task | starttranscriptionjob | arn:aws:states:::aws-sdk:transcribe:startTranscriptionJob |  |  |
| GetTranscriptionJob | Task | gettranscriptionjob | arn:aws:states:::aws-sdk:transcribe:getTranscriptionJob |  |  |
| Read Transcript | Task | getobject | Bucket=${bucket} |  |  |
| Bedrock InvokeModel | Task | invokemodel | arn:aws:states:::bedrock:invokeModel |  |  |
| Call third-party API | Task | invoke | ApiEndpoint=${public_inference_endpoint} |  |  |
| Wait for user feedback | Task | waitfortasktoken | FunctionName=${send_response_lambda} |  |  |
| Generate Avatar | Task | invokemodel | arn:aws:states:::bedrock:invokeModel |  |  |
| send custom avatar to user | Task | invoke | FunctionName=${send_response_lambda} |  |  |

## sfn-collection__sfn-cfn-stacksets-workflow-cdk__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| ListRoots | Task | listroots | arn:aws:states:::aws-sdk:organizations:listRoots |  |  |
| CreateStackSet | Task | createstackset | arn:aws:states:::aws-sdk:cloudformation:createStackSet |  |  |
| CreateStackInstances | Task | createstackinstances | arn:aws:states:::aws-sdk:cloudformation:createStackInstances |  |  |
| CheckCreatingStackSetStatus | Task | describestacksetoperation | arn:aws:states:::aws-sdk:cloudformation:describeStackSetOper |  |  |
| CheckDeletingStackSetStatus | Task | describestacksetoperation | arn:aws:states:::aws-sdk:cloudformation:describeStackSetOper |  |  |
| DeleteStackInstances | Task | deletestackinstances | arn:aws:states:::aws-sdk:cloudformation:deleteStackInstances |  |  |
| DeleteStackSet | Task | deletestackset | arn:aws:states:::aws-sdk:cloudformation:deleteStackSet |  |  |

## sfn-collection__uml-statemachine__statemachine__BlogBuySellSM.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Check Stock Price | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Generate Buy/Sell recommendation | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Route For Approval (Callback) | Task | waitfortasktoken | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Buy Stock | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Sell Stock | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Report Result | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |
| Log Reject | Task | invoke | FunctionName=${BlogDummyUMLHandlerLambdaArn} |  |  |

## sfn-collection__cqrs__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Invoke ItemSalesReport Query | Task | invoke | FunctionName=${QueryItemSalesReportFunctionArn} |  |  |
| Send ItemSalesReport Query Results | Task | putevents | arn:aws:states:::events:putEvents |  |  |
| Put Order to DynamoDB | Task | putitem | TableName=${DynamoDBTableName} |  |  |
| Send Response to EventBridge | Task | putevents | arn:aws:states:::events:putEvents |  |  |
| Invoke MonthlySalesByItem Query | Task | invoke | FunctionName=${QueryMonthlySalesByItemFunctionArn} |  |  |
| Send MonthlySalesByItem Query Results | Task | putevents | arn:aws:states:::events:putEvents |  |  |

## sfn-collection__distributed-data-stream-aggregator__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Get Third-party locations | Task | query | TableName=locations |  |  |
| Iterate Containers[Map]/Get location Summary | ? |  |  |  |  |
| Combine Part Files | Task | startjobrun | arn:aws:states:::glue:startJobRun |  |  |
| Has Job Finish | Task | getjobrun | arn:aws:states:::aws-sdk:glue:getJobRun |  |  |
| Update DynamoDb | Task | updateitem | TableName={% 'task_table' %} |  |  |

## sfn-collection__idempotent-workflow-sam__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Create idempotency settings (key and ttl) | Task | ${idempotencyconfigfunctionarn} | ${IdempotencyConfigFunctionArn} |  |  |
| Create and lock idempotency record | Task | ${ddbtransactwriteitems} | ${DDBTransactWriteItems} |  |  |
| Get idempotency record from DynamoDB | Task | ${ddbgetitem} | TableName=${DDBTable} |  |  |
| Save execution results | Task | ${ddbupdateitem} | TableName=${DDBTable} |  |  |
| Save failure | Task | ${ddbupdateitem} | TableName=${DDBTable} |  |  |

## sfn-collection__bedrock-evaluations-sam__statemachine__statemachine.asl.json

| task | type | action | target | idempotent? | persistent? |
|------|------|--------|--------|-------------|-------------|
| Create Evaluation Pipelines[Branch0]/CreateKnowledgeBase | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch0]/Create Data Source | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch0]/Start Ingestion Job | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch0]/Get Ingestion Job | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch0]/Create Evaluation Job | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch0]/Get Evaluation Job Status | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Create Knowledge Base 2 | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Create Data Source 2 | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Start Ingestion Job 2 | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Get Ingestion Job 2 | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Create Evaluation Job 2 | ? |  |  |  |  |
| Create Evaluation Pipelines[Branch1]/Get Evaluation Job Status 2 | ? |  |  |  |  |

_Total: 13 files, 116 tasks._
