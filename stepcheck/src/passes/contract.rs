//! Contract / data-flow checking (codes `SC1xxx`).
//!
//! Two layers:
//!   * **Native** (no annotations): sound, ASL-semantic data-binding checks —
//!     a `waitForTaskToken` task that never references the task token (SC1001),
//!     misuse of the `$$.Map` context outside a `Map` (SC1002), and syntactically
//!     invalid JSONPath payloads (SC1003).
//!   * **Typed** (with declared/DSL schemas): a successor task must not require
//!     an input field the producer does not output (SC1010) — the classic
//!     `amount` vs `total` contract mismatch.

use super::retry::qualify;
use super::{forward_tasks, Pass};
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{collect_jsonpath_refs, State, StateKind, Workflow};

pub struct ContractPass;

impl Pass for ContractPass {
    fn id(&self) -> &'static str {
        "contract"
    }
    fn title(&self) -> &'static str {
        "Contract / data-flow checking"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", false, sink);
    }
}

fn refs_of(st: &State) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = &st.parameters {
        collect_jsonpath_refs(p, &mut out);
    }
    for c in &st.choices {
        collect_jsonpath_refs(&c.condition, &mut out);
    }
    out
}

fn run_machine(wf: &Workflow, scope: &str, in_map: bool, sink: &mut DiagnosticSink) {
    for (name, st) in &wf.states {
        let qname = qualify(scope, name);
        let refs = refs_of(st);

        // SC1001: waitForTaskToken task that never passes the task token.
        if let Some(r) = &st.resource {
            if r.contains(".waitForTaskToken") {
                let params_str = st
                    .parameters
                    .as_ref()
                    .map(|p| p.to_string())
                    .unwrap_or_default();
                if !params_str.contains("Task.Token") {
                    sink.push(
                        Diagnostic::error(
                            "SC1001",
                            &qname,
                            format!("task '{name}' uses .waitForTaskToken but never passes $$.Task.Token"),
                        )
                        .with_note("the callback can never be delivered; the execution blocks until timeout"),
                    );
                }
            }
        }

        // SC1002: $$.Map.* context used outside a Map iterator. A Map state's
        // own ItemSelector/Parameters legitimately use the item context, so a
        // Map state is exempt.
        if !in_map && st.kind != StateKind::Map {
            for r in &refs {
                if r.contains("$$.Map.") {
                    sink.push(Diagnostic::warning(
                        "SC1002",
                        &qname,
                        format!("reference '{r}' uses the $$.Map context outside a Map state"),
                    ));
                    break;
                }
            }
        }

        // SC1003: syntactically invalid JSONPath payload.
        for r in &refs {
            if !is_valid_path(r) {
                sink.push(
                    Diagnostic::warning(
                        "SC1003",
                        &qname,
                        format!("'{r}' is not a valid JSONPath or intrinsic-function payload"),
                    )
                    .with_note("a `.$` value must start with `$` or be a States.* intrinsic"),
                );
            }
        }

        recurse(st, scope, sink);
    }

    // SC1010: typed contract compatibility between adjacent tasks.
    for (name, st) in &wf.states {
        if !st.is_task() {
            continue;
        }
        let Some(out_fields) = &st.anno.output_fields else { continue };
        let succs = forward_tasks(wf, name);
        for sn in succs {
            let succ = &wf.states[&sn];
            if let Some(in_fields) = &succ.anno.input_fields {
                for need in in_fields {
                    if !out_fields.contains(need) {
                        sink.push(
                            Diagnostic::error(
                                "SC1010",
                                &qualify(scope, &sn),
                                format!(
                                    "contract mismatch: '{sn}' requires field '{need}' not produced by predecessor '{name}'"
                                ),
                            )
                            .with_note(format!("'{name}' outputs {{{}}}", out_fields.join(", "))),
                        );
                    }
                }
            }
        }
    }
}

fn is_valid_path(r: &str) -> bool {
    r.starts_with('$') || r.starts_with("States.")
}

fn recurse(st: &State, scope: &str, sink: &mut DiagnosticSink) {
    if let Some(it) = &st.iterator {
        // inside a Map iterator: $$.Map context is valid here
        run_machine(it, &qualify(scope, &format!("{}[Map]", st.name)), true, sink);
    }
    for (i, br) in st.branches.iter().enumerate() {
        // Parallel branches are not Map iterators
        run_machine(br, &qualify(scope, &format!("{}[Branch{i}]", st.name)), false, sink);
    }
}
