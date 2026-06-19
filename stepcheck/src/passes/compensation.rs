//! Compensation-completeness checking (codes `SC4xxx`).
//!
//! In a Saga, every *persistent* (resource-acquiring) forward operation must
//! have a way to be undone if a later step fails. Statically, that means a
//! persistent task should either declare a compensation handler (DSL/sidecar)
//! or route its failures through a `Catch` to a compensating path. A persistent
//! task with neither leaves its effect uncompensated.

use super::retry::{qualify, severity};
use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{State, Workflow};

pub struct CompensationPass;

impl Pass for CompensationPass {
    fn id(&self) -> &'static str {
        "compensation"
    }
    fn title(&self) -> &'static str {
        "Compensation-completeness checking"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", sink);
    }
}

fn run_machine(wf: &Workflow, scope: &str, sink: &mut DiagnosticSink) {
    for (name, st) in &wf.states {
        if st.is_task() && st.anno.persistent == Some(true) {
            let has_explicit = st.anno.compensation.is_some();
            let has_catch = !st.catch.is_empty();
            let qname = qualify(scope, name);

            // SC4002: declared compensation must name a real state.
            if let Some(comp) = &st.anno.compensation {
                if !wf.states.contains_key(comp) {
                    sink.push(Diagnostic::error(
                        "SC4002",
                        &qname,
                        format!("declared compensation '{comp}' is not a state in the workflow"),
                    ));
                }
            }

            // SC4001: persistent op with no compensation and no error handling.
            if !has_explicit && !has_catch {
                let sev = severity(st);
                let effect = st.anno.effect.clone().unwrap_or_else(|| "durable effect".into());
                sink.push(
                    Diagnostic::new(
                        sev,
                        "SC4001",
                        &qname,
                        format!(
                            "persistent task '{name}' ({effect}) has no compensating action or failure handling"
                        ),
                    )
                    .with_note(note(st)),
                );
            }
        }
        recurse(st, scope, sink);
    }
}

fn note(st: &State) -> String {
    if st.anno.inferred {
        "persistence inferred from naming; add a Catch to a compensating state or annotate the task idempotent/non-persistent".into()
    } else {
        "add a Catch to a compensating state or a `compensate(...)` handler".into()
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
