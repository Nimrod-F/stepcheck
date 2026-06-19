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

#[test]
fn structural_pass_flags_dangling_transition() {
    let src = r#"{"StartAt":"A","States":{
        "A":{"Type":"Task","Resource":"r","Next":"Missing"}}}"#;
    let wf = asl::parse_str(src, "t").unwrap();
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&passes::default_pipeline(), &wf, &mut sink);
    assert!(sink.has_code("SC0002"), "expected a dangling-transition error");
}
