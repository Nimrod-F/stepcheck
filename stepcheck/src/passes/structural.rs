//! Structural validation (codes `SC0xxx`).
//!
//! Native — needs no annotations. Catches well-formedness defects that AWS only
//! reports at deploy/run time: dangling transitions, unreachable states,
//! dead-ends, non-exhaustive `Choice`, and missing `Map`/`Parallel` sub-machines.
//! Runs recursively over nested `Map`/`Parallel` machines.

use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{StateKind, Workflow};
use std::collections::HashSet;

pub struct StructuralPass;

impl Pass for StructuralPass {
    fn id(&self) -> &'static str {
        "structural"
    }
    fn title(&self) -> &'static str {
        "Structural validation"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        check_machine(wf, "", sink);
    }
}

fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}/{name}")
    }
}

fn check_machine(wf: &Workflow, scope: &str, sink: &mut DiagnosticSink) {
    // SC0001: StartAt must name an existing state.
    if !wf.states.contains_key(&wf.start_at) {
        sink.push(Diagnostic::error(
            "SC0001",
            &qualify(scope, &wf.start_at),
            format!("StartAt refers to undefined state '{}'", wf.start_at),
        ));
    }

    // SC0010: a state value that is not a JSON object is malformed.
    let mut malformed: HashSet<&str> = HashSet::new();
    for (name, st) in &wf.states {
        if st.kind == StateKind::Unknown("<non-object>".to_string()) {
            malformed.insert(name.as_str());
            sink.push(
                Diagnostic::error(
                    "SC0010",
                    &qualify(scope, name),
                    format!("state '{name}' is not a valid state object"),
                )
                .with_note("a top-level directive (e.g. QueryLanguage) may be misplaced inside States"),
            );
        }
    }

    // SC0002: every transition target must exist.
    for (name, st) in &wf.states {
        if malformed.contains(name.as_str()) {
            continue;
        }
        for tgt in st.all_successors() {
            if !wf.states.contains_key(tgt) {
                sink.push(
                    Diagnostic::error(
                        "SC0002",
                        &qualify(scope, name),
                        format!("transition targets undefined state '{tgt}'"),
                    )
                    .with_note("a Next/Default/Choice/Catch target has no matching state"),
                );
            }
        }

        // SC0004: a non-terminal state must offer a normal continuation.
        if !st.is_terminal() && st.normal_successors().is_empty() {
            sink.push(
                Diagnostic::error(
                    "SC0004",
                    &qualify(scope, name),
                    format!(
                        "non-terminal {} state has no Next/End or successor",
                        st.kind.as_str()
                    ),
                )
                .with_note("the state can never hand off control on success"),
            );
        }

        // SC0007: a Choice without Default is non-exhaustive.
        if st.kind == StateKind::Choice && st.default.is_none() {
            sink.push(
                Diagnostic::warning(
                    "SC0007",
                    &qualify(scope, name),
                    "Choice state has no Default branch",
                )
                .with_note("an input matching no rule fails at runtime with States.NoChoiceMatched"),
            );
        }

        // SC0008/SC0009: structural completeness of compound states.
        if st.kind == StateKind::Map && st.iterator.is_none() {
            sink.push(Diagnostic::error(
                "SC0008",
                &qualify(scope, name),
                "Map state has no Iterator/ItemProcessor",
            ));
        }
        if st.kind == StateKind::Parallel && st.branches.is_empty() {
            sink.push(Diagnostic::warning(
                "SC0009",
                &qualify(scope, name),
                "Parallel state has no Branches",
            ));
        }
    }

    // Reachability from StartAt (normal + catch transitions).
    let mut reachable: HashSet<&str> = HashSet::new();
    if wf.states.contains_key(&wf.start_at) {
        let mut stack = vec![wf.start_at.as_str()];
        while let Some(cur) = stack.pop() {
            if !reachable.insert(cur) {
                continue;
            }
            if let Some(st) = wf.states.get(cur) {
                for s in st.all_successors() {
                    if wf.states.contains_key(s) && !reachable.contains(s) {
                        stack.push(s);
                    }
                }
            }
        }
    }
    // SC0003: unreachable states.
    for name in wf.states.keys() {
        if malformed.contains(name.as_str()) {
            continue;
        }
        if !reachable.contains(name.as_str()) && name != &wf.start_at {
            sink.push(Diagnostic::warning(
                "SC0003",
                &qualify(scope, name),
                format!("state '{name}' is unreachable from StartAt"),
            ));
        }
    }
    // SC0005: no reachable terminal => possible non-termination.
    let has_terminal = reachable
        .iter()
        .filter_map(|n| wf.states.get(*n))
        .any(|s| s.is_terminal());
    if !reachable.is_empty() && !has_terminal {
        sink.push(
            Diagnostic::warning(
                "SC0005",
                &qualify(scope, &wf.start_at),
                "no terminal state (Succeed/Fail/End) reachable from StartAt",
            )
            .with_note("the workflow may never complete"),
        );
    }

    // Recurse into nested machines.
    for (name, st) in &wf.states {
        if let Some(it) = &st.iterator {
            check_machine(it, &qualify(scope, &format!("{name}[Map]")), sink);
        }
        for (i, br) in st.branches.iter().enumerate() {
            check_machine(br, &qualify(scope, &format!("{name}[Branch{i}]")), sink);
        }
    }
}
