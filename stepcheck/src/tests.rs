//! Automated tests: the DSL frontend (both example workflows, valid and
//! reordered), the ASL emitter round-trip, and the ASL / CNCF frontends.
//! Run with `cargo test`.

use crate::{annot, asl, cncf, diag, dsl, ir, passes};

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

// ----- data-flow / provenance analysis (SC1101) -----------------------------

fn check_asl(src: &str, sidecar: Option<&annot::Sidecar>) -> diag::DiagnosticSink {
    let mut wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, sidecar, false, &mut sink);
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
}

#[test]
fn sidecar_unknown_task_is_diagnostic() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","End":true}}}"#;
    let mut sc = annot::Sidecar::default();
    sc.tasks.insert("MissingTask".into(), annot::TaskAnnot::default());
    let sink = check_asl(src, Some(&sc));
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
    let sink = check_asl(src, Some(&sc));
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
