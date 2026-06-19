//! Typestate / business-protocol checking (codes `SC2xxx`).
//!
//! Tasks may be annotated with the business state they consume (`state_in`) and
//! produce (`state_out`). Two checks:
//!   * **SC2001 composition** — for adjacent tasks, the producer's `state_out`
//!     must equal the consumer's `state_in`; this catches re-orderings such as
//!     "ship before pay".
//!   * **SC2002 protocol conformance** — when a protocol (set of allowed
//!     business transitions) is declared, each task's own `state_in -> state_out`
//!     must be an allowed edge.
//!
//! With no annotations this pass is a no-op (reported honestly as not applicable).

use super::retry::qualify;
use super::{forward_tasks, Pass};
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{State, Workflow};

pub struct TypestatePass;

impl Pass for TypestatePass {
    fn id(&self) -> &'static str {
        "typestate"
    }
    fn title(&self) -> &'static str {
        "Typestate / business-protocol checking"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", sink);
    }
}

fn run_machine(wf: &Workflow, scope: &str, sink: &mut DiagnosticSink) {
    let has_protocol = !wf.protocol.is_empty();
    for (name, st) in &wf.states {
        if st.is_task() {
            // SC2002: protocol conformance of this task's own transition.
            if has_protocol {
                if let (Some(si), Some(so)) = (&st.anno.state_in, &st.anno.state_out) {
                    let allowed = wf.protocol.iter().any(|(f, t)| f == si && t == so);
                    if !allowed {
                        sink.push(
                            Diagnostic::error(
                                "SC2002",
                                &qualify(scope, name),
                                format!("task '{name}' performs an undeclared business transition {si} -> {so}"),
                            )
                            .with_note("this transition is not in the declared workflow protocol"),
                        );
                    }
                }
            }
            // SC2001: composition with the successor task(s).
            if let Some(so) = &st.anno.state_out {
                for sn in forward_tasks(wf, name) {
                    let succ = &wf.states[&sn];
                    if let Some(si) = &succ.anno.state_in {
                        if si != so {
                            sink.push(
                                Diagnostic::error(
                                    "SC2001",
                                    &qualify(scope, &sn),
                                    format!(
                                        "typestate mismatch: '{sn}' consumes {si} but predecessor '{name}' produces {so}"
                                    ),
                                )
                                .with_note("a task is composed out of business-protocol order"),
                            );
                        }
                    }
                }
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
