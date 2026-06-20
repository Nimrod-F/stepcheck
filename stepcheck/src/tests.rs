//! Automated tests: the DSL frontend (both example workflows, valid and
//! reordered), the ASL emitter round-trip, and the ASL / CNCF frontends.
//! Run with `cargo test`.

use crate::{annot, asl, cncf, diag, dsl, ir, passes};

/// Resolve annotations from a sidecar (no inference) and run the full pipeline.
fn verify(mut wf: ir::Workflow, sidecar: &annot::Sidecar) -> diag::DiagnosticSink {
    annot::resolve(&mut wf, Some(sidecar), false);
    let mut sink = diag::DiagnosticSink::new();
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
    annot::resolve(&mut wf, sidecar, false);
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    sink
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

// ----- concurrency interference (SC5001) ------------------------------------

fn concurrency_sink(src: &str) -> diag::DiagnosticSink {
    let mut wf = asl::parse_str(src, "t").unwrap();
    annot::resolve(&mut wf, None, true);
    let mut sink = diag::DiagnosticSink::new();
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
