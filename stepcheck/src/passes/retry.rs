//! Retry-safety analysis (codes `SC3xxx`).
//!
//! A retry is unsafe when a *non-idempotent* task is retried on a **broad**
//! error class (`States.ALL` / `States.TaskFailed`), because such a retry can
//! fire after the task already produced its effect — duplicating a charge,
//! shipment, e-mail, etc. Retries restricted to transient infrastructure errors
//! (throttling, `ServiceException`) are not flagged.

use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink, Severity};
use crate::ir::{State, Workflow};

pub struct RetrySafetyPass;

const BROAD: &[&str] = &["States.ALL", "States.TaskFailed"];

impl Pass for RetrySafetyPass {
    fn id(&self) -> &'static str {
        "retry"
    }
    fn title(&self) -> &'static str {
        "Retry-safety analysis"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", sink);
    }
}

fn run_machine(wf: &Workflow, scope: &str, sink: &mut DiagnosticSink) {
    for (name, st) in &wf.states {
        if st.is_task() && st.anno.idempotent == Some(false) && st.has_retry() {
            // Does any retry rule catch a broad error class?
            let broad: Vec<&str> = st
                .retry
                .iter()
                .flat_map(|r| r.error_equals.iter())
                .filter(|e| BROAD.contains(&e.as_str()))
                .map(|e| e.as_str())
                .collect();
            if !broad.is_empty() {
                let sev = severity(st);
                let qname = qualify(scope, name);
                let effect = st.anno.effect.clone().unwrap_or_else(|| "side effect".into());
                sink.push(
                    Diagnostic::new(
                        sev,
                        "SC3001",
                        &qname,
                        format!(
                            "non-idempotent task '{name}' is retried on {} and may duplicate its effect ({effect})",
                            broad.join(", ")
                        ),
                    )
                    .with_note(confidence_note(st, "restrict Retry to transient errors, make the task idempotent, or annotate it idempotent")),
                );
            }
        }
        recurse(st, scope, sink);
    }
}

fn recurse(st: &State, scope: &str, sink: &mut DiagnosticSink) {
    if let Some(it) = &st.iterator {
        run_machine(it, &qualify(scope, &format!("{}[Map]", st.name)), sink);
    }
    for (i, br) in st.branches.iter().enumerate() {
        run_machine(br, &qualify(scope, &format!("{}[Branch{i}]", st.name)), sink);
    }
}

pub(crate) fn severity(st: &State) -> Severity {
    if st.anno.inferred {
        Severity::Warning
    } else {
        Severity::Error
    }
}

pub(crate) fn confidence_note(st: &State, advice: &str) -> String {
    if st.anno.inferred {
        format!("idempotency inferred from naming; {advice}")
    } else {
        advice.to_string()
    }
}

pub(crate) fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}/{name}")
    }
}
