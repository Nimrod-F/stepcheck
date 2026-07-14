//! Automated tests: the DSL frontend (both example workflows, valid and
//! reordered), the ASL emitter round-trip, and the ASL / CNCF frontends.
//! Run with `cargo test`.

use crate::{annot, asl, cfn, cncf, diag, dsl, ir, passes};

/// Resolve annotations from a sidecar (no inference) and run the full pipeline.
fn verify(mut wf: ir::Workflow, sidecar: &annot::Sidecar) -> diag::DiagnosticSink {
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, Some(sidecar), false, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

#[test]
fn dsl_order_valid_is_clean() {
    let (wf, sc) = dsl::order_example(false);
    let sink = verify(wf, &sc);
    assert_eq!(sink.errors(), 0, "valid order should be clean: {:#?}", sink.diagnostics);
}

#[test]
fn dsl_order_reordered_is_rejected() {
    let (wf, sc) = dsl::order_example(true);
    let sink = verify(wf, &sc);
    assert!(sink.has_code("SC2001"), "expected a typestate error");
    assert!(sink.has_code("SC1010"), "expected a contract error");
}

#[test]
fn dsl_travel_valid_is_clean() {
    let (wf, sc) = dsl::travel_example(false);
    let sink = verify(wf, &sc);
    assert_eq!(sink.errors(), 0, "valid travel should be clean: {:#?}", sink.diagnostics);
}

#[test]
fn dsl_travel_reordered_is_rejected() {
    let (wf, sc) = dsl::travel_example(true);
    let sink = verify(wf, &sc);
    assert!(sink.has_code("SC2001"), "expected a typestate error");
    assert!(sink.has_code("SC1010"), "expected a contract error");
}

#[test]
fn dsl_emits_reparsable_asl() {
    // DSL -> ASL JSON -> reparse: the emitter produces valid, loadable ASL.
    let (wf, _) = dsl::order_example(false);
    let json = serde_json::to_string(&asl::emit(&wf)).unwrap();
    let round = asl::parse_str(&json, "roundtrip").unwrap();
    assert_eq!(round.start_at, "CreateOrder");
    assert!(round.states.contains_key("ChargeCard"));
    // Succeed/Fail states must not carry an explicit End (AWS rejects it).
    assert!(!json.contains("\"OrderCompleted\":{\"Type\":\"Succeed\",\"End\""));
}

#[test]
fn asl_emit_preserves_passthrough_fields() {
    let src = r#"{"Comment":"top","QueryLanguage":"JSONata","TimeoutSeconds":3600,"StartAt":"MapIt","States":{
        "MapIt":{"Type":"Map","Label":"jobs","ItemReader":{"Resource":"arn:aws:states:::s3:getObject"},
            "ItemProcessor":{"ProcessorConfig":{"Mode":"DISTRIBUTED"},"StartAt":"Work","States":{
                "Work":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke","Catch":[{"ErrorEquals":["States.ALL"],"Assign":{"err":"{% $states.errorOutput %}"},"Next":"Done"}],"Next":"Done"},
                "Done":{"Type":"Succeed"}}},"End":true}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let emitted = asl::emit(&wf);
    assert_eq!(emitted.get("TimeoutSeconds").and_then(|v| v.as_i64()), Some(3600));
    assert_eq!(emitted.get("QueryLanguage").and_then(|v| v.as_str()), Some("JSONata"));
    let map = emitted.pointer("/States/MapIt").unwrap();
    assert!(map.get("ItemProcessor").is_some(), "ItemProcessor spelling should be preserved");
    assert!(map.get("Iterator").is_none(), "Distributed Map should not be rewritten to Iterator");
    assert!(map.get("ItemReader").is_some(), "uninterpreted Map fields should pass through");
    assert_eq!(map.get("Label").and_then(|v| v.as_str()), Some("jobs"));
    assert!(emitted.pointer("/States/MapIt/ItemProcessor/ProcessorConfig").is_some());
    assert!(emitted.pointer("/States/MapIt/ItemProcessor/States/Work/Catch/0/Assign").is_some());
}

#[test]
fn asl_frontend_parses() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","Next":"B"},
        "B":{"Type":"Succeed"}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    assert_eq!(wf.start_at, "A");
    assert_eq!(wf.total_states(), 2);
}

#[test]
fn cncf_frontend_parses() {
    let src = "document:\n  name: t\ndo:\n  - first:\n      call: http\n      then: exit\n";
    let wf = cncf::parse_str(src, "t").unwrap();
    assert!(wf.states.contains_key("first"));
}

#[test]
fn cfn_yaml_template_extracts_state_machine() {
    // A SAM/CloudFormation YAML template with a short-form !Sub DefinitionString:
    // the CFN frontend must find the AWS::StepFunctions::StateMachine, resolve the
    // ${...} intrinsics (keeping service ARNs intact), and yield runnable ASL.
    let src = concat!(
        "Resources:\n",
        "  ProcessBooking:\n",
        "    Type: AWS::StepFunctions::StateMachine\n",
        "    Properties:\n",
        "      StateMachineName: !Sub ${AWS::StackName}-ProcessBooking\n",
        "      DefinitionString: !Sub |\n",
        "        {\n",
        "          \"StartAt\": \"Charge\",\n",
        "          \"States\": {\n",
        "            \"Charge\": {\n",
        "              \"Type\": \"Task\",\n",
        "              \"Resource\": \"${CollectPayment.Arn}\",\n",
        "              \"Next\": \"Confirm\"\n",
        "            },\n",
        "            \"Confirm\": { \"Type\": \"Succeed\" }\n",
        "          }\n",
        "        }\n",
    );
    let machines = cfn::extract(src, "template.yaml", None).unwrap();
    assert_eq!(machines.len(), 1);
    let (id, wf) = &machines[0];
    assert_eq!(id, "ProcessBooking");
    assert!(wf.states.contains_key("Charge") && wf.states.contains_key("Confirm"));
}

#[test]
fn cfn_json_template_with_dangling_transition_flags_sc0002() {
    // A CDK-synth-style JSON template (Fn::Sub long form) whose extracted ASL has a
    // dangling Next -> the structural pass fires on the CFN-extracted machine.
    let src = r#"{"Resources":{"Machine":{"Type":"AWS::StepFunctions::StateMachine",
      "Properties":{"DefinitionString":{"Fn::Sub":"{ \"StartAt\": \"A\", \"States\": { \"A\": { \"Type\": \"Task\", \"Resource\": \"${Fn.Arn}\", \"Next\": \"Missing\" } } }"}}}}}"#;
    let machines = cfn::extract(src, "cdk.out", None).unwrap();
    assert_eq!(machines.len(), 1);
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&passes::default_pipeline(), &machines[0].1, &mut sink);
    assert!(sink.has_code("SC0002"), "expected SC0002 on dangling transition: {:#?}", sink.diagnostics);
}

#[test]
fn cfn_template_links_child_state_machine_aliases() {
    let src = r#"{"Resources":{
      "Parent":{"Type":"AWS::StepFunctions::StateMachine","Properties":{"DefinitionString":{"Fn::Sub":"{ \"StartAt\": \"Run\", \"States\": { \"Run\": { \"Type\": \"Task\", \"Resource\": \"arn:aws:states:::states:startExecution\", \"Parameters\": { \"StateMachineArn\": \"${Child.Arn}\" }, \"End\": true } } }"}}},
      "Child":{"Type":"AWS::StepFunctions::StateMachine","Properties":{"DefinitionString":"{ \"StartAt\": \"Write\", \"States\": { \"Write\": { \"Type\": \"Task\", \"Resource\": \"arn:aws:states:::dynamodb:putItem\", \"Parameters\": { \"TableName\": \"Orders\", \"Item\": { \"id\": { \"S\": \"1\" } } }, \"End\": true } } }"}}
    }}"#;
    let machines = cfn::extract(src, "template.json", None).unwrap();
    let parent = machines.iter().find(|(id, _)| id == "Parent").unwrap();
    let run = &parent.1.states["Run"];
    let arn = match &run.parameters {
        Some(serde_json::Value::Object(p)) => p.get("StateMachineArn").and_then(|v| v.as_str()),
        _ => None,
    };
    assert_eq!(arn, Some("arn:aws:states:us-east-1:000000000000:stateMachine:Child"));
    assert!(parent.1.linked_children.contains_key(arn.unwrap()));
}

#[test]
fn cfn_template_links_state_machine_name_alias_for_child_flattening() {
        let src = r#"{"Resources":{
            "Parent":{"Type":"AWS::StepFunctions::StateMachine","Properties":{"StateMachineName":"parent-sm","DefinitionString":{"Fn::Sub":"{ \"StartAt\": \"P\", \"States\": { \"P\": { \"Type\": \"Parallel\", \"End\": true, \"Branches\": [ { \"StartAt\": \"C1\", \"States\": { \"C1\": { \"Type\": \"Task\", \"Resource\": \"arn:aws:states:::states:startExecution\", \"Parameters\": { \"StateMachineArn\": \"arn:aws:states:${AWS::Region}:${AWS::AccountId}:stateMachine:child-sm\" }, \"End\": true } } }, { \"StartAt\": \"C2\", \"States\": { \"C2\": { \"Type\": \"Task\", \"Resource\": \"arn:aws:states:::states:startExecution\", \"Parameters\": { \"StateMachineArn\": \"arn:aws:states:${AWS::Region}:${AWS::AccountId}:stateMachine:child-sm\" }, \"End\": true } } } ] } } }"}}},
            "Child":{"Type":"AWS::StepFunctions::StateMachine","Properties":{"StateMachineName":"child-sm","DefinitionString":"{ \"StartAt\": \"Write\", \"States\": { \"Write\": { \"Type\": \"Task\", \"Resource\": \"arn:aws:states:::dynamodb:putItem\", \"Parameters\": { \"TableName\": \"Orders\", \"Item\": { \"id\": { \"S\": \"1\" } } }, \"End\": true } } }"}}
        }}"#;
        let machines = cfn::extract(src, "template.json", None).unwrap();
        let (_, mut parent) = machines.into_iter().find(|(id, _)| id == "Parent").unwrap();
        let mut sink = diag::DiagnosticSink::new();
        annot::resolve(&mut parent, None, true, &mut sink);
        passes::run_pipeline(&passes::default_pipeline(), &parent, &mut sink);
        assert!(sink.has_code("SC5003"), "expected child invocation composition warning: {:#?}", sink.diagnostics);
        assert!(sink.has_code("SC5001"), "expected flattened child write collision through StateMachineName alias: {:#?}", sink.diagnostics);
}

fn check_cncf(src: &str) -> diag::DiagnosticSink {
    let mut wf = cncf::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, None, false, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

#[test]
fn cncf_jq_read_of_missing_field_flags_sc1101() {
    // A jq reference `${ .customerId }` in `with:` reads a field absent from the
    // task's declared input schema {orderId, amount} -> SC1101, at the same
    // extension point as ASL (no pass changed to support CNCF).
    let src = "document:\n  name: t\ndo:\n  - chargeCard:\n      call: http\n      input:\n        schema:\n          document:\n            properties:\n              orderId: { type: string }\n              amount: { type: number }\n      with:\n        order: ${ .orderId }\n        customer: ${ .customerId }\n";
    let sink = check_cncf(src);
    assert!(sink.has_code("SC1101"), "expected SC1101 on CNCF jq read: {:#?}", sink.diagnostics);
}

#[test]
fn cncf_jq_read_of_present_field_is_clean() {
    // Reading only declared fields via jq must not flag (no false positive), and
    // a composite jq expression must stay silent (projects to Top).
    let src = "document:\n  name: t\ndo:\n  - chargeCard:\n      call: http\n      input:\n        schema:\n          document:\n            properties:\n              orderId: { type: string }\n              amount: { type: number }\n      with:\n        order: ${ .orderId }\n        doubled: ${ .amount * 2 }\n";
    let sink = check_cncf(src);
    assert!(!sink.has_code("SC1101"), "must not flag present/opaque jq: {:#?}", sink.diagnostics);
}

/// Lower a single `switch` task and return its first case's Choice condition.
fn cncf_first_guard(when: &str) -> serde_json::Value {
    let src = format!(
        "document:\n  name: t\ndo:\n  - route:\n      switch:\n        - hit:\n            when: {when}\n            then: done\n        - miss:\n            then: done\n  - done:\n      set:\n        ok: true\n"
    );
    let wf = cncf::parse_str(&src, "t").unwrap();
    wf.states.get("route").unwrap().choices[0].condition.clone()
}

#[test]
fn cncf_when_comparison_operators_read_the_field() {
    // Non-`==` comparison guards must expose the document field as a Choice read
    // (so SC1101/SC1110 apply), and preserve the precise numeric comparator.
    let lt = cncf_first_guard("${ .customer.age < 18 }");
    assert_eq!(lt["Variable"], "$.customer.age");
    assert_eq!(lt["NumericLessThan"], 18.0);

    let gt = cncf_first_guard("${ .temperature > 38 }");
    assert_eq!(gt["Variable"], "$.temperature");
    assert_eq!(gt["NumericGreaterThan"], 38.0);

    // `!=` (and any non-literal / context right side) falls back to a sound
    // presence read: the field is still definitely read to evaluate the guard.
    let ne = cncf_first_guard("${ .vet != null }");
    assert_eq!(ne["Variable"], "$.vet");
    assert_eq!(ne["IsPresent"], true);

    // Existing equality behaviour is preserved.
    let eq = cncf_first_guard("${ .status == \"OPEN\" }");
    assert_eq!(eq["Variable"], "$.status");
    assert_eq!(eq["StringEquals"], "OPEN");
}

#[test]
fn cncf_when_boolean_models_only_leading_operand() {
    // jq `and`/`or` short-circuit, so only the leading operand's field is
    // guaranteed to be read; we model exactly that one (sound under-approximation).
    let cond = cncf_first_guard("${ .bpm < 60 or .bpm > 100 }");
    assert_eq!(cond["Variable"], "$.bpm");
    assert_eq!(cond["NumericLessThan"], 60.0);
}

#[test]
fn cncf_when_composite_lhs_stays_opaque() {
    // A guard whose left side is not a pure document field (arithmetic, context)
    // must not manufacture a read: it stays opaque so no unsound SC1101 fires.
    let arith = cncf_first_guard("${ .a + .b == 5 }");
    assert!(arith.get("Variable").is_none(), "arithmetic LHS must stay opaque: {arith:#?}");
    let ctx = cncf_first_guard("${ $context.userId == 7 }");
    assert!(ctx.get("Variable").is_none(), "context LHS must stay opaque: {ctx:#?}");
}

// ----- data-flow / provenance analysis (SC1101) -----------------------------

fn check_asl(src: &str, sidecar: Option<&annot::Sidecar>) -> diag::DiagnosticSink {
    let mut wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, sidecar, false, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

fn check_asl_with_result_shapes(src: &str, sidecar: Option<&annot::Sidecar>) -> diag::DiagnosticSink {
    let mut wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, sidecar, false, &mut sink);
    passes::run_pipeline(&passes::pipeline_with_result_shapes(), &wf, &mut sink);
    sink
}

#[test]
fn sidecar_unknown_task_is_diagnostic() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","End":true}}}"#;
    let mut sc = annot::Sidecar::default();
    sc.tasks.insert("MissingTask".into(), annot::TaskAnnot::default());
    let sink = check_asl_with_result_shapes(src, Some(&sc));
    assert!(sink.has_code("SC0011"), "expected stale sidecar task diagnostic: {:#?}", sink.diagnostics);
}

#[test]
fn sidecar_unknown_compensation_is_diagnostic() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","End":true}}}"#;
    let mut sc = annot::Sidecar::default();
    sc.tasks.insert(
        "A".into(),
        annot::TaskAnnot { compensation: Some("UndoA".into()), ..Default::default() },
    );
    let sink = check_asl(src, Some(&sc));
    assert!(sink.has_code("SC0011"), "expected stale compensation diagnostic: {:#?}", sink.diagnostics);
}

#[test]
fn sidecar_unknown_schema_is_diagnostic() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","End":true}}}"#;
    let mut sc = annot::Sidecar::default();
    sc.workflow.input_schema = Some("MissingSchema".into());
    let sink = check_asl(src, Some(&sc));
    assert!(sink.has_code("SC0012"), "expected stale sidecar schema diagnostic: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_native_flags_constructed_missing_field() {
    // A Pass builds {order:{orderId,amount}}; a later task reads $.order.total.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"order":{"orderId":"o","amount":1}},"ResultPath":"$","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"total.$":"$.order.total"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "expected a native data-flow error: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_native_clean_when_field_exists() {
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"order":{"orderId":"o","amount":1}},"ResultPath":"$","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"total.$":"$.order.amount"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "produced field must not be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_typed_tracks_through_resultpath_merge() {
    // amount is in the schema; reservation.reservationId is produced by the merge.
    let mut sc = annot::Sidecar::default();
    sc.workflow.input_schema = Some("In".into());
    sc.schemas.insert("In".into(), annot::SchemaDef { fields: vec!["orderId".into(), "amount".into()] });
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"r","Payload":{"orderId.$":"$.orderId"}},
                   "ResultSelector":{"reservationId.$":"$.Payload.id"},"ResultPath":"$.reservation","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"a.$":"$.amount","r.$":"$.reservation.reservationId"}},"End":true}}}"#;
    let sink = check_asl_with_result_shapes(src, Some(&sc));
    assert!(!sink.has_code("SC1101"), "valid typed flow must be clean: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_typed_flags_schema_and_merge_misses() {
    let mut sc = annot::Sidecar::default();
    sc.workflow.input_schema = Some("In".into());
    sc.schemas.insert("In".into(), annot::SchemaDef { fields: vec!["orderId".into(), "amount".into()] });
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"r","Payload":{"orderId.$":"$.orderId"}},
                   "ResultSelector":{"reservationId.$":"$.Payload.id"},"ResultPath":"$.reservation","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"a.$":"$.total","r.$":"$.reservation.reservationCode"}},"End":true}}}"#;
    let sink = check_asl(src, Some(&sc));
    let n = sink.diagnostics.iter().filter(|d| d.code == "SC1101").count();
    assert!(n >= 2, "expected two SC1101 (schema miss + merge miss), got {n}: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_tracks_declared_task_output_contract() {
    let mut sc = annot::Sidecar::default();
    sc.schemas.insert("Out".into(), annot::SchemaDef { fields: vec!["known".into()] });
    sc.tasks.insert(
        "Produce".into(),
        annot::TaskAnnot { output_schema: Some("Out".into()), ..Default::default() },
    );
    let src = r#"{"StartAt":"Produce","States":{
        "Produce":{"Type":"Task","Resource":"${ProducerArn}","Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.missing"}},"End":true}}}"#;
    let sink = check_asl_with_result_shapes(src, Some(&sc));
    assert!(sink.has_code("SC1101"), "closed task output contracts should expose absent fields: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_tracks_known_lambda_result_envelope() {
    let src = r#"{"StartAt":"Invoke","States":{
        "Invoke":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"p","Payload":{"id":"1"}},"Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.__stepcheck_absent"}},"End":true}}}"#;
    let sink = check_asl_with_result_shapes(src, None);
    assert!(sink.has_code("SC1101"), "known service result envelopes should be closed records: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_accepts_quoted_bracket_member_paths() {
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"order":{"total":7}},"ResultPath":"$","Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$['order'][\"id\"]"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "quoted bracket member paths should be resolved precisely: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_choice_is_present() {
    // A Choice IsPresent test against a (constructed, known) document reads a
    // possibly-absent field BY DESIGN; it returns false, it does not fail. Must
    // NOT be flagged SC1101.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"a":1},"ResultPath":"$","Next":"Pick"},
        "Pick":{"Type":"Choice","Choices":[{"Variable":"$.b","IsPresent":true,"Next":"Done"}],"Default":"Done"},
        "Done":{"Type":"Succeed"}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "Choice IsPresent on an absent field must not be SC1101: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_literal_variable_field() {
    // A field literally named "Variable" inside a Parameters payload is verbatim
    // data, not a JSONPath read; it must not be checked.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"cfg":{"x":1}},"ResultPath":"$","Next":"Send"},
        "Send":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                "Parameters":{"FunctionName":"s","Payload":{"Variable":"$.cfg.missing","real.$":"$.cfg.x"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "a literal 'Variable' data field must not be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_opaque_input() {
    // Reading arbitrary fields of the (opaque) execution input must NOT be flagged.
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
             "Parameters":{"FunctionName":"a","Payload":{"x.$":"$.anything.at.all"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "opaque input must not be flagged (soundness): {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_jsonata_produced_field() {
    // SOUNDNESS REGRESSION: a JSONata state reshapes the document with an `Output`
    // expression we do not model; a later JSONPath task reads a field that
    // expression produces. Modelling the JSONata Pass as identity would make
    // `$.computed` look definitely-absent — a FALSE POSITIVE on valid, deployable
    // ASL. Treating the JSONata state opaquely (output Top) keeps the read `Maybe`.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"raw":{"x":1}},"ResultPath":"$","Next":"Shape"},
        "Shape":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Output":"{% { 'computed': $states.input.raw.x } %}","Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"v.$":"$.computed"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "a JSONata-produced field must not be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_flags_jsonpath_assign_missing_field() {
    // JSONPath Assign is a hard-failing read site too. In a Pass state with no
    // explicit Result/Parameters, `$` is the effective input, so this missing
    // field is a native SC1101.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"order":{"total":7}},"ResultPath":"$","Next":"Store"},
        "Store":{"Type":"Pass","Assign":{"saved.$":"$.order.id"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "Assign read of a definitely-absent field must be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_pass_assign_reads_pass_result_shape() {
    // For a JSONPath Pass state, Assign reads from the Pass result, not from the
    // pre-Parameters input. Checking this against the input would be a false
    // positive on $.order.id.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"raw":{"id":1}},"ResultPath":"$","Next":"Store"},
        "Store":{"Type":"Pass","Parameters":{"order":{"id.$":"$.raw.id"}},
                 "Assign":{"saved.$":"$.order.id"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "Assign should read from the Pass result shape: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_jsonpath_workflow_variable_reference() {
    // Workflow-variable references (`$customerName`) are not document paths
    // (`$.customerName`). The current document lattice treats them as a
    // conservative non-SC1101 read, so a field can travel through a variable
    // without being misreported as absent from the document.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"customer":{"name":"Ada"}},"ResultPath":"$","Next":"Store"},
        "Store":{"Type":"Pass","Assign":{"customerName.$":"$.customer.name"},"Next":"Use"},
        "Use":{"Type":"Pass","Parameters":{"name.$":"$customerName"},"ResultPath":"$","Next":"Send"},
        "Send":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                "Parameters":{"FunctionName":"u","Payload":{"name.$":"$.name"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "workflow-variable reads must not be treated as missing document fields: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_whole_jsonata_machine() {
    // A machine in JSONata mode uses `{% … %}` Arguments, never JSONPath `.$`
    // reads, so the provenance check must stay silent (and not crash).
    let src = r#"{"StartAt":"A","QueryLanguage":"JSONata","States":{
        "A":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
             "Arguments":{"x":"{% $states.input.nope %}"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "JSONata machine must not be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_jsonata_output_shape_is_precise_for_object_literal() {
    // A JSONata Output object literal closes the successor document. The later
    // JSONPath read of $.other is therefore definitely absent and should fire;
    // treating all JSONata as Top would miss this.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"raw":{"x":1}},"ResultPath":"$","Next":"Shape"},
        "Shape":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Output":"{% { 'computed': $states.input.raw.x } %}","Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"v.$":"$.other"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "JSONata Output object should close the document shape: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_jsonata_object_constructor_absent_field() {
    // SOUNDNESS REGRESSION: a JSONata object constructor `{% { 'k': $ref } %}`
    // OMITS a key whose value is undefined (JSONata 2.0.6, which ASL implements),
    // yielding `{}` rather than a `States.QueryEvaluationError`. Only a *bare*
    // top-level expression that evaluates to undefined hard-fails. So a
    // definitely-absent reference nested INSIDE a constructor must NOT be flagged
    // SC1101, even though `Build` closes `$states.input` to `{a}`.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"a":1},"ResultPath":"$","Next":"Shape"},
        "Shape":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Output":"{% { 'k': $states.input.missing } %}","End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "a ref inside a JSONata object constructor must not hard-fail: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_flags_direct_jsonata_missing_input_read() {
    // Direct `$states.input` reads in modeled JSONata value positions fail when
    // the referenced field is definitely absent.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"order":{"total":7}},"ResultPath":"$","Next":"Shape"},
        "Shape":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Output":{"id":"{% $states.input.order.id %}"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "direct JSONata read of a definitely-absent field must be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_keeps_complex_jsonata_predicates_conservative() {
    // `$exists(...)` is a valid way to tolerate absent JSONata paths. Complex
    // expressions are therefore not treated as hard-failing direct reads.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"order":{"total":7}},"ResultPath":"$","Next":"Shape"},
        "Shape":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Output":{"ok":"{% $exists($states.input.order.id) %}"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "complex JSONata predicates must stay conservative: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_jsonata_variable_carries_shape_to_jsonpath() {
    // A field can move through a workflow variable and then re-enter the JSON
    // document through JSONata Output; the later JSONPath read should see it.
    let src = r#"{"StartAt":"Build","QueryLanguage":"JSONPath","States":{
        "Build":{"Type":"Pass","Result":{"customer":{"name":"Ada"}},"ResultPath":"$","Next":"Store"},
        "Store":{"Type":"Pass","QueryLanguage":"JSONata",
                 "Assign":{"person":"{% $states.input.customer %}"},"Next":"Use"},
        "Use":{"Type":"Pass","QueryLanguage":"JSONata",
               "Output":{"name":"{% $person.name %}"},"Next":"Send"},
        "Send":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                "Parameters":{"FunctionName":"u","Payload":{"name.$":"$.name"}},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"), "JSONata variable transport should preserve the produced field: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_flags_direct_jsonata_undefined_variable_read() {
    let src = r#"{"StartAt":"Use","QueryLanguage":"JSONata","States":{
        "Use":{"Type":"Pass","Output":{"name":"{% $person.name %}"},"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1101"), "direct read of an unassigned workflow variable must be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_sound_on_parallel_parameters_seed() {
    // SOUNDNESS REGRESSION: a Parallel `Parameters` constructs the payload each
    // branch receives; a branch reads a field that Parameters produces. Seeding the
    // branch from the pre-Parameters effective input alone would make `$.b` look
    // definitely-absent — a FALSE POSITIVE. The branch seed must apply Parameters.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"a":1},"ResultPath":"$","Next":"Fan"},
        "Fan":{"Type":"Parallel","Parameters":{"b.$":"$.a"},"End":true,"Branches":[
            {"StartAt":"Use","States":{
                "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                       "Parameters":{"FunctionName":"u","Payload":{"v.$":"$.b"}},"End":true}}}
        ]}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1101"),
        "a Parallel branch field constructed by Parameters must not be flagged: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_flags_dead_choice_guard() {
    // A Pass builds {a:1}; a Choice tests IsPresent on $.b (never produced) -> the
    // branch can never be taken: a dead guard (SC1110). The IsPresent:true on the
    // present field $.a must NOT be flagged.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"a":1},"ResultPath":"$","Next":"Pick"},
        "Pick":{"Type":"Choice","Choices":[
            {"Variable":"$.b","IsPresent":true,"Next":"Done"},
            {"Variable":"$.a","IsPresent":true,"Next":"Done"}],"Default":"Done"},
        "Done":{"Type":"Succeed"}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC1110"), "expected a dead-guard warning: {:#?}", sink.diagnostics);
    assert_eq!(sink.diagnostics.iter().filter(|d| d.code == "SC1110").count(), 1,
        "only the $.b guard is dead: {:#?}", sink.diagnostics);
}

#[test]
fn dataflow_no_dead_guard_on_opaque_input() {
    // Against the opaque execution input, no guard is provably dead (soundness).
    let src = r#"{"StartAt":"Pick","States":{
        "Pick":{"Type":"Choice","Choices":[{"Variable":"$.b","IsPresent":true,"Next":"Done"}],"Default":"Done"},
        "Done":{"Type":"Succeed"}}}"#;
    let sink = check_asl(src, None);
    assert!(!sink.has_code("SC1110"), "must not flag a guard against opaque input: {:#?}", sink.diagnostics);
}

// ----- retry safety (SC3001) ------------------------------------------------

#[test]
fn retry_flags_broad_retry_on_non_idempotent_task() {
    let src = r#"{"StartAt":"Charge","States":{
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
          "Retry":[{"ErrorEquals":["States.ALL"]}],"End":true}}}"#;
    let mut sc = annot::Sidecar::default();
    sc.tasks.insert(
        "Charge".into(),
        annot::TaskAnnot { idempotent: Some(false), effect: Some("charge".into()), ..Default::default() },
    );
    let sink = check_asl(src, Some(&sc));
    assert!(sink.has_code("SC3001"), "expected unsafe-retry diagnostic: {:#?}", sink.diagnostics);
}

// ----- execution oracle: independent witness of the soundness theorem -------

#[test]
fn oracle_confirms_native_miss_and_clears_hit() {
    use crate::concrete::{check_ref, Verdict};
    // Build {order:{orderId,amount}}; Charge reads $.order.total (absent) and would
    // also (clean variant) read $.order.amount (present). The oracle, by independent
    // per-path enumeration, must confirm the miss absent and the hit present.
    let src = r#"{"StartAt":"Build","States":{
        "Build":{"Type":"Pass","Result":{"order":{"orderId":"o","amount":1}},"ResultPath":"$","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"total.$":"$.order.total"}},"End":true}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    assert_eq!(check_ref(&wf, "Charge", "$.order.total"), Verdict::ConfirmedAbsent);
    assert_eq!(check_ref(&wf, "Charge", "$.order.amount"), Verdict::Present);
    // and never a false counterexample on the opaque execution input
    assert_eq!(check_ref(&wf, "Build", "$.anything"), Verdict::Unverifiable);
}

#[test]
fn oracle_confirms_typed_merge_miss() {
    use crate::concrete::{check_ref, Verdict};
    // Reserve produces reservationId via ResultSelector/ResultPath; Charge reads
    // reservationCode (a flow-sensitive miss). With the input schema seeded, the
    // oracle confirms it absent on all paths.
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"r","Payload":{"orderId.$":"$.orderId"}},
                   "ResultSelector":{"reservationId.$":"$.Payload.id"},"ResultPath":"$.reservation","Next":"Charge"},
        "Charge":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                  "Parameters":{"FunctionName":"c","Payload":{"r.$":"$.reservation.reservationCode"}},"End":true}}}"#;
    let mut wf = asl::parse_str(src, "t").unwrap();
    wf.input_fields = Some(vec!["orderId".into(), "amount".into()]);
    assert_eq!(check_ref(&wf, "Charge", "$.reservation.reservationCode"), Verdict::ConfirmedAbsent);
    assert_eq!(check_ref(&wf, "Charge", "$.reservation.reservationId"), Verdict::Present);
}

#[test]
fn oracle_unrolls_loop_revisits() {
    use crate::concrete::{check_ref, Verdict};
    // The first visit to Check lacks $.done, but the Wait branch writes it and
    // loops back. The oracle must keep exploring after recording the first target
    // entry, otherwise it would incorrectly confirm $.done absent.
    let src = r#"{"StartAt":"Seed","States":{
        "Seed":{"Type":"Pass","Result":{"order":{"amount":1}},"ResultPath":"$","Next":"Check"},
        "Check":{"Type":"Choice","Choices":[{"Variable":"$.done","IsPresent":true,"Next":"Use"}],"Default":"Wait"},
        "Wait":{"Type":"Pass","Result":true,"ResultPath":"$.done","Next":"Check"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.order.total"}},"End":true}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    assert_eq!(check_ref(&wf, "Check", "$.done"), Verdict::Present);
    assert_eq!(check_ref(&wf, "Use", "$.order.total"), Verdict::ConfirmedAbsent);
}

#[test]
fn dataflow_waitfortasktoken_result_is_opaque() {
    // Surfaced on aws-solutions/media2cloud: a `.waitForTaskToken` callback's
    // result is the arbitrary SendTaskSuccess payload, not the base service
    // envelope, so under --result-shapes a downstream read must not be flagged a
    // definite absence (that was a false positive on real callback workflows).
    let src = r#"{"StartAt":"Wait","States":{
        "Wait":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke.waitForTaskToken",
            "Parameters":{"FunctionName":"f","Payload":{"token.$":"$$.Task.Token"}},"ResultPath":"$","Next":"Use"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
            "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.uuid"}},"End":true}}}"#;
    let sink = check_asl_with_result_shapes(src, None);
    assert!(!sink.has_code("SC1101"), "callback result is opaque, must not flag: {:#?}", sink.diagnostics);
}

#[test]
fn issue_sns_envelope_missing_field_sc1101() {
    // Regression for aws issue-tracker defect campus-compute#32: a task reads a
    // job field from the SNS Publish response envelope {MessageId,SequenceNumber}
    // that never contains it -> the runtime "JSONPath could not be found" error,
    // caught statically as SC1101 under service-result-shape modeling.
    let src = r#"{"StartAt":"Notify","States":{
        "Notify":{"Type":"Task","Resource":"arn:aws:states:::sns:publish",
            "Parameters":{"TopicArn":"arn:aws:sns:us-east-1:1:alerts","Message":"failed"},
            "ResultPath":"$","Next":"Audit"},
        "Audit":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
            "Parameters":{"FunctionName":"audit","Payload":{"execution_id.$":"$.execution_id"}},"End":true}}}"#;
    let sink = check_asl_with_result_shapes(src, None);
    assert!(sink.has_code("SC1101"), "expected SC1101 on SNS-envelope read: {:#?}", sink.diagnostics);
}

// ----- concurrency interference (SC5001) ------------------------------------

fn concurrency_sink(src: &str) -> diag::DiagnosticSink {
    let mut wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, None, true, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

#[test]
fn concurrency_flags_parallel_overwriting_write() {
    // Two branches both PutItem (overwrite) the same table -> genuine race.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"W1","States":{"W1":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:putItem",
            "Parameters":{"TableName":"Orders","Item":{"id.$":"$.id"}},"End":true}}},
        {"StartAt":"W2","States":{"W2":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:putItem",
            "Parameters":{"TableName":"Orders","Item":{"id.$":"$.id"}},"End":true}}}]}}}"#;
    assert!(concurrency_sink(src).has_code("SC5001"), "expected a parallel-interference warning");
}

#[test]
fn concurrency_clean_on_merging_writes() {
    // Two branches UpdateItem the same table (atomic merge, commutes) -> NOT a race.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"W1","States":{"W1":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:updateItem",
            "Parameters":{"TableName":"Orders","Key":{"id.$":"$.id"}},"End":true}}},
        {"StartAt":"W2","States":{"W2":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:updateItem",
            "Parameters":{"TableName":"Orders","Key":{"id.$":"$.id"}},"End":true}}}]}}}"#;
    assert!(!concurrency_sink(src).has_code("SC5001"), "merging writes must not be flagged");
}

#[test]
fn concurrency_flags_named_child_execution_collision() {
    // Two branches start the same child state machine with the same static execution name.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child","Name":"order-42"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child","Name":"order-42"},"End":true}}}]}}}"#;
    assert!(concurrency_sink(src).has_code("SC5001"), "expected a child-execution collision warning");
}

#[test]
fn concurrency_flags_named_child_execution_sync_collision() {
    // The same static child execution name also collides through the synchronous service integration.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution.sync:2",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child","Name":"order-42"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution.sync:2",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child","Name":"order-42"},"End":true}}}]}}}"#;
    assert!(concurrency_sink(src).has_code("SC5001"), "expected a synchronous child-execution collision warning");
}

#[test]
fn concurrency_composes_across_startexecution_sc5003() {
    // Two parallel branches invoke the same child workflow (a CloudFormation ARN
    // reference, no static Name). No SC5001 name-collision, but the child's
    // effects run twice concurrently -> SC5003 composition warning.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildStateMachine}"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildStateMachine}"},"End":true}}}]}}}"#;
    let sink = concurrency_sink(src);
    assert!(sink.has_code("SC5003"), "expected startExecution composition warning: {:#?}", sink.diagnostics);
    assert!(!sink.has_code("SC5001"), "no name collision here");
}

#[test]
fn concurrency_flattens_resolved_child_writes_for_sc5001() {
    let parent_src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildA}"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildB}"},"End":true}}}]}}}"#;
    let child_a_src = r#"{"StartAt":"AWrite","States":{"AWrite":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:putItem",
        "Parameters":{"TableName":"Orders","Item":{"id":{"S":"1"}}},"End":true}}}"#;
    let child_b_src = r#"{"StartAt":"BWrite","States":{"BWrite":{"Type":"Task","Resource":"arn:aws:states:::dynamodb:putItem",
        "Parameters":{"TableName":"Orders","Item":{"id":{"S":"1"}}},"End":true}}}"#;

    let mut parent = asl::parse_str(parent_src, "parent").unwrap();
    let child_a = asl::parse_str(child_a_src, "child-a").unwrap();
    let child_b = asl::parse_str(child_b_src, "child-b").unwrap();
    parent.linked_children = std::rc::Rc::new(std::collections::BTreeMap::from([
        ("${ChildA}".to_string(), child_a),
        ("${ChildB}".to_string(), child_b),
    ]));
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut parent, None, true, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &parent, &mut sink);
    assert!(sink.has_code("SC5001"), "expected flattened child write collision: {:#?}", sink.diagnostics);
}

#[test]
fn concurrency_clean_on_distinct_child_executions() {
    // Two branches invoke *different* child workflows: no composition interference.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildA}"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"${ChildB}"},"End":true}}}]}}}"#;
    assert!(!concurrency_sink(src).has_code("SC5003"), "distinct children must not be flagged");
}

#[test]
fn concurrency_clean_on_anonymous_child_execution() {
    // Without a static Name, Step Functions generates distinct child execution names.
    let src = r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"C1","States":{"C1":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child"},"End":true}}},
        {"StartAt":"C2","States":{"C2":{"Type":"Task","Resource":"arn:aws:states:::states:startExecution",
            "Parameters":{"StateMachineArn":"arn:aws:states:us-east-1:111122223333:stateMachine:Child"},"End":true}}}]}}}"#;
    assert!(!concurrency_sink(src).has_code("SC5001"), "anonymous child executions must not be flagged");
}

#[test]
fn concurrency_clean_on_non_overwriting_service_integrations() {
    let cases = [
        (
            "eventbridge",
            r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"E1","States":{"E1":{"Type":"Task","Resource":"arn:aws:states:::events:putEvents",
            "Parameters":{"Entries":[{"EventBusName":"Orders","Source":"checkout","DetailType":"created","Detail":"{}"}]},"End":true}}},
        {"StartAt":"E2","States":{"E2":{"Type":"Task","Resource":"arn:aws:states:::events:putEvents",
            "Parameters":{"Entries":[{"EventBusName":"Orders","Source":"checkout","DetailType":"created","Detail":"{}"}]},"End":true}}}]}}}"#,
        ),
        (
            "ecs",
            r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"R1","States":{"R1":{"Type":"Task","Resource":"arn:aws:states:::ecs:runTask",
            "Parameters":{"Cluster":"orders","TaskDefinition":"worker"},"End":true}}},
        {"StartAt":"R2","States":{"R2":{"Type":"Task","Resource":"arn:aws:states:::ecs:runTask",
            "Parameters":{"Cluster":"orders","TaskDefinition":"worker"},"End":true}}}]}}}"#,
        ),
        (
            "bedrock",
            r#"{"StartAt":"P","States":{"P":{"Type":"Parallel","End":true,"Branches":[
        {"StartAt":"B1","States":{"B1":{"Type":"Task","Resource":"arn:aws:states:::bedrock:invokeModel",
            "Parameters":{"ModelId":"anthropic.claude-3-haiku-20240307-v1:0"},"End":true}}},
        {"StartAt":"B2","States":{"B2":{"Type":"Task","Resource":"arn:aws:states:::bedrock:invokeModel",
            "Parameters":{"ModelId":"anthropic.claude-3-haiku-20240307-v1:0"},"End":true}}}]}}}"#,
        ),
    ];

    for (case_name, source) in cases {
        assert!(
            !concurrency_sink(source).has_code("SC5001"),
            "{case_name} should not be treated as a last-writer-wins overwrite"
        );
    }
}

// ----- effect-aware compensation (SC4010) -----------------------------------

#[test]
fn compensation_flags_noncompensating_catch() {
    // A persistent task whose only Catch goes to a Fail never undoes its effect.
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"ReserveInventory"},
                   "Catch":[{"ErrorEquals":["States.ALL"],"Next":"Fail"}],"Next":"Done"},
        "Fail":{"Type":"Fail"},
        "Done":{"Type":"Succeed"}}}"#;
    let sink = concurrency_sink(src); // resolves with inference, runs full pipeline
    assert!(sink.has_code("SC4010"), "expected non-compensating-catch warning: {:#?}", sink.diagnostics);
    assert!(!sink.has_code("SC4001"), "SC4001 must not fire when a Catch exists");
}

#[test]
fn compensation_clean_when_catch_compensates() {
    // The Catch reaches a Release action -> the effect is compensated, no SC4010.
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"ReserveInventory"},
                   "Catch":[{"ErrorEquals":["States.ALL"],"Next":"ReleaseInventory"}],"Next":"Done"},
        "ReleaseInventory":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"ReleaseInventory"},"Next":"Fail"},
        "Fail":{"Type":"Fail"},
        "Done":{"Type":"Succeed"}}}"#;
    let sink = concurrency_sink(src);
    assert!(!sink.has_code("SC4010"), "a compensating catch must not be flagged: {:#?}", sink.diagnostics);
}

// ----- downstream-aware compensation (SC4011, declared tier) ----------------

#[test]
fn compensation_flags_downstream_escape() {
    // Two persistent steps, each with its OWN declared compensator going straight
    // to the Fail terminal (no reverse-order chaining). If `Charge` fails after
    // `Reserve` has committed, `Reserve`'s `Release` is never reached, so the
    // reservation leaks. SC4001/SC4010 cannot see this (Reserve's own Catch does
    // reach a compensator); the downstream-aware SC4011 does.
    let (wf, sc) = dsl::Builder::new("saga", "Reserve")
        .schema("S0", &["id"])
        .schema("S1", &["id"])
        .schema("S2", &["id"])
        .protocol("S0", "S1")
        .protocol("S1", "S2")
        .task("Reserve", "S0", "S1", false, true, Some("Release"), Some("Charge"), Some("Release"))
        .task("Charge", "S1", "S2", false, true, Some("Refund"), None, Some("Refund"))
        .compensator("Release", "Failed") // straight to Fail, not chained
        .compensator("Refund", "Failed")
        .fail("Failed")
        .build();
    let sink = verify(wf, &sc);
    assert!(sink.has_code("SC4011"), "expected a downstream-compensation escape: {:#?}", sink.diagnostics);
    assert!(!sink.has_code("SC4001"), "SC4001 must not fire when compensation is declared");
}

#[test]
fn compensation_clean_on_chained_saga() {
    // The order saga wires its compensators as a reverse-order chain, so every
    // post-commit failure unwinds all prior effects: no SC4011.
    let (wf, sc) = dsl::order_example(false);
    let sink = verify(wf, &sc);
    assert!(!sink.has_code("SC4011"), "a complete reverse-order Saga must not be flagged: {:#?}", sink.diagnostics);
}

// ----- temporal analysis (SC6001/SC6003) ------------------------------------

#[test]
fn temporal_flags_unbounded_callback_and_bad_heartbeat() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke.waitForTaskToken",
             "Parameters":{"FunctionName":"a","Payload":{"t.$":"$$.Task.Token"}},"Next":"B"},
        "B":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke","TimeoutSeconds":10,"HeartbeatSeconds":20,"End":true}}}"#;
    let sink = check_asl(src, None);
    assert!(sink.has_code("SC6001"), "expected unbounded-callback warning");
    assert!(sink.has_code("SC6003"), "expected heartbeat>=timeout error");
}

#[test]
fn structural_pass_flags_dangling_transition() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","Next":"Missing"}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    assert!(sink.has_code("SC0002"), "expected a dangling-transition error");
}

// ----- data-flow fixpoint telemetry (fixpoint-stats command) ----------------

#[test]
fn fixpoint_recorder_captures_converged_rounds() {
    // A back-edge (CheckStock -> WaitRestock -> CheckStock) forces the forward
    // data-flow fixpoint to take more than one round. The opt-in recorder must
    // capture each analyzed machine, every one must converge within its bound,
    // and no observed round count may exceed the finite-height bound.
    let src = r#"{"StartAt":"CheckStock","States":{
        "CheckStock":{"Type":"Choice","Choices":[{"Variable":"$.ok","IsPresent":true,"Next":"Done"}],"Default":"WaitRestock"},
        "WaitRestock":{"Type":"Wait","Seconds":1,"Next":"CheckStock"},
        "Done":{"Type":"Succeed"}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    passes::dataflow::fixpoint_record_start();
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    let rec = passes::dataflow::fixpoint_record_take();
    assert!(!rec.is_empty(), "recorder should capture at least one machine");
    assert!(rec.iter().all(|r| r.converged), "every machine must converge within its bound");
    assert!(rec.iter().all(|r| r.rounds <= r.bound), "rounds must not exceed the bound");
}

#[test]
fn dataflow_reports_after_cyclic_reembedding_widens() {
    // This loop re-embeds the whole document under $.wrap on each iteration. A
    // cap-and-suppress fixpoint can lose all SC1101 reporting for the machine;
    // depth-k widening collapses the deep tail to Top while preserving the
    // shallow fact that $.missing is never produced.
    let mut sc = annot::Sidecar::default();
    sc.workflow.input_schema = Some("In".into());
    sc.schemas.insert("In".into(), annot::SchemaDef { fields: vec!["base".into(), "stop".into()] });
    let src = r#"{"StartAt":"Grow","States":{
        "Grow":{"Type":"Pass","ResultPath":"$.wrap","Next":"Check"},
        "Check":{"Type":"Choice","Choices":[{"Variable":"$.stop","IsPresent":true,"Next":"Use"}],"Default":"Grow"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.missing"}},"End":true}}}"#;
    let sink = check_asl(src, Some(&sc));
    assert!(
        sink.has_code("SC1101"),
        "widened cyclic re-embedding should still report shallow missing fields: {:#?}",
        sink.diagnostics
    );
}

#[test]
fn dataflow_certificate_checks_cyclic_report() {
    let mut sc = annot::Sidecar::default();
    sc.workflow.input_schema = Some("In".into());
    sc.schemas.insert("In".into(), annot::SchemaDef { fields: vec!["base".into(), "stop".into()] });
    let src = r#"{"StartAt":"Grow","States":{
        "Grow":{"Type":"Pass","ResultPath":"$.wrap","Next":"Check"},
        "Check":{"Type":"Choice","Choices":[{"Variable":"$.stop","IsPresent":true,"Next":"Use"}],"Default":"Grow"},
        "Use":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
               "Parameters":{"FunctionName":"u","Payload":{"x.$":"$.missing"}},"End":true}}}"#;
    let mut wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, Some(&sc), false, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    let sc1101 = sink.diagnostics.iter().filter(|d| d.code == "SC1101").count();
    let cert = passes::dataflow::certify_sc1101(&wf, false);
    assert!(cert.ok(), "certificate obligations should hold");
    assert_eq!(cert.sc1101_reports, sc1101, "certificate and diagnostics should agree");
    assert!(cert.postconditions > 0, "certificate should check finite edge obligations");
}

// ---------------------------------------------------------------------------
// Hard mutants (W1): each boundary variant is a genuine, reparsable defect, and
// each behaves at its analysis's ⊤ / coverage / family boundary as designed.
// ---------------------------------------------------------------------------

use crate::mutate::{mutate, mutate_hard, MutationKind};

/// Full pipeline with naming inference (the tier the mutation study uses).
fn verify_infer(mut wf: ir::Workflow) -> diag::DiagnosticSink {
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, None, true, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

#[test]
fn hard_concurrency_dynamic_name_escapes_sc5001() {
    let src = r#"{"StartAt":"P","States":{
        "P":{"Type":"Parallel","End":true,"Branches":[
            {"StartAt":"A","States":{"A":{"Type":"Pass","End":true}}},
            {"StartAt":"B","States":{"B":{"Type":"Pass","End":true}}}]}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    // Easy: static shared table is a statically-resolvable resource -> SC5001.
    let easy = mutate(&wf, MutationKind::Concurrency, 0).unwrap();
    assert!(verify_infer(easy).has_code("SC5001"), "easy concurrency mutant should raise SC5001");
    // Hard: dynamically-named table -> resource identity lifts to ⊤ -> no SC5001.
    let hard = mutate_hard(&wf, MutationKind::Concurrency, 0).unwrap();
    assert!(!verify_infer(hard).has_code("SC5001"), "hard concurrency mutant must escape SC5001 (dynamic resource)");
}

#[test]
fn hard_temporal_reference_paths_escape_sc6003() {
    let src = r#"{"StartAt":"T","States":{
        "T":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke","Parameters":{"FunctionName":"f"},"End":true}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let easy = mutate(&wf, MutationKind::Temporal, 0).unwrap();
    assert!(verify_infer(easy).has_code("SC6003"), "easy temporal mutant should raise SC6003");
    let hard = mutate_hard(&wf, MutationKind::Temporal, 0).unwrap();
    assert!(!verify_infer(hard.clone()).has_code("SC6003"), "hard temporal mutant must escape SC6003 (reference-path values)");
    let json = serde_json::to_string(&asl::emit(&hard)).unwrap();
    assert!(json.contains("HeartbeatSecondsPath") && json.contains("TimeoutSecondsPath"),
            "hard temporal mutant must emit reference-path fields: {json}");
}

#[test]
fn hard_compensation_non_compensating_catch_caught_by_sibling() {
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke","Parameters":{"FunctionName":"reserveInventory"},"End":true}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let hard = mutate_hard(&wf, MutationKind::Compensation, 0).unwrap();
    let sink = verify_infer(hard);
    // The presence-only SC4001 does NOT fire (there is a Catch); the effect-aware
    // sibling SC4010 does -- the family catches it, the operator's code does not.
    assert!(!sink.has_code("SC4001"), "hard compensation mutant should not raise the presence-only SC4001");
    assert!(sink.has_code("SC4010"), "hard compensation mutant should be caught by the sibling SC4010");
}

#[test]
fn hard_structural_nested_dangling_still_caught() {
    let src = r#"{"StartAt":"P","States":{
        "P":{"Type":"Parallel","End":true,"Branches":[
            {"StartAt":"A","States":{"A":{"Type":"Pass","Next":"B"},"B":{"Type":"Pass","End":true}}}]}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let hard = mutate_hard(&wf, MutationKind::Structural, 0).unwrap();
    assert!(verify_infer(hard).has_code("SC0002"),
            "structural reachability is exact and recurses: a nested dangling edge must still be caught");
}

#[test]
fn hard_mutants_emit_reparsable_asl() {
    // Every hard mutant that applies must round-trip through the ASL emitter.
    let src = r#"{"StartAt":"Reserve","States":{
        "Reserve":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke",
                   "Parameters":{"FunctionName":"reserveInventory","Payload":{"id.$":"$.orderId"}},"Next":"P"},
        "P":{"Type":"Parallel","End":true,"Branches":[
            {"StartAt":"A","States":{"A":{"Type":"Pass","End":true}}},
            {"StartAt":"B","States":{"B":{"Type":"Pass","End":true}}}]}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    for kind in MutationKind::all_hard() {
        if let Some(m) = mutate_hard(&wf, kind, 0) {
            let json = serde_json::to_string(&asl::emit(&m)).unwrap();
            asl::parse_str(&json, "roundtrip")
                .unwrap_or_else(|e| panic!("hard {kind:?} mutant must emit reparsable ASL: {e}\n{json}"));
        }
    }
}
