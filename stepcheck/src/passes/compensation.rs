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
use crate::ir::{State, StateKind, Workflow};

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

            // SC4010 (effect-aware): the task DOES have a Catch, but no error path
            // reaches a compensating action---it merely logs, notifies, or fails, so
            // the durable effect is never undone. This is the over-acceptance gap in
            // a plain "has a Catch" rule: a Catch to a logger is not a compensation.
            if !has_explicit && has_catch
                && !st.catch.iter().any(|c| reaches_compensator(wf, &c.next))
            {
                sink.push(
                    Diagnostic::warning(
                        "SC4010",
                        &qname,
                        format!(
                            "persistent task '{name}' has a Catch, but no error path reaches a compensating action (it only logs/notifies/fails)"
                        ),
                    )
                    .with_note(
                        "route the failure to a state that undoes the effect (refund/cancel/release/rollback), or annotate a compensation",
                    ),
                );
            }

            // SC4011 (declared tier, sound + complete): downstream-aware Saga
            // completeness. A persistent task with a *declared* compensator `C`
            // is Saga-complete only if every failure that can occur *after it
            // commits* routes into the compensation chain that reaches `C`. The
            // task's own Catch (SC4001/SC4010) is necessary but not sufficient: a
            // later step (e.g.\ a shipping or approval step) can fail with this
            // task's effect already committed, and unless that step's failure also
            // reaches `C` the effect leaks. We decide this as control-flow
            // reachability over the post-commit region; inference never supplies a
            // compensator, so this fires only on declared annotations.
            if let Some(comp) = &st.anno.compensation {
                if wf.states.contains_key(comp) {
                    if let Some(escape) = downstream_escape(wf, name, comp) {
                        sink.push(
                            Diagnostic::error(
                                "SC4011",
                                &qname,
                                format!(
                                    "persistent task '{name}' declares compensation '{comp}', but a failure at '{escape}' after it commits never reaches '{comp}' (its effect is left uncompensated)"
                                ),
                            )
                            .with_note(
                                "route that downstream failure into the reverse-order compensation chain that reaches the declared compensator",
                            ),
                        );
                    }
                }
            }
        }
        recurse(st, scope, sink);
    }
}

/// Whether any task reachable (via normal control flow) from `start` looks like a
/// compensating/undo action. Used to decide if a Catch path actually compensates.
fn reaches_compensator(wf: &Workflow, start: &str) -> bool {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        let Some(st) = wf.states.get(&cur) else { continue };
        if st.is_task() && crate::annot::is_compensator(st) {
            return true;
        }
        for s in st.normal_successors() {
            stack.push(s.to_string());
        }
    }
    false
}

/// The states that execute *after* `start` commits, reached via *normal* control
/// flow (the post-commit region). These are the states whose failure must be
/// compensated for `start`. The compensator `comp` is never on a normal path from
/// `start`, but we exclude it defensively so the region is purely forward work.
fn post_commit_region(wf: &Workflow, start: &str, comp: &str) -> Vec<String> {
    use std::collections::{HashSet, VecDeque};
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut q: VecDeque<String> = wf
        .states
        .get(start)
        .map(|s| s.normal_successors().iter().map(|x| x.to_string()).collect())
        .unwrap_or_default();
    while let Some(cur) = q.pop_front() {
        if cur == comp || !seen.insert(cur.clone()) {
            continue;
        }
        let Some(st) = wf.states.get(&cur) else { continue };
        out.push(cur.clone());
        for s in st.normal_successors() {
            q.push_back(s.to_string());
        }
    }
    out
}

/// Whether `state` can raise an error at run time (and so needs its failure
/// routed to the compensator). Only task-like states invoke external work;
/// `Choice`/`Pass`/`Wait`/`Succeed` cannot fail a forward operation.
fn is_fallible(st: &State) -> bool {
    matches!(st.kind, StateKind::Task | StateKind::Map | StateKind::Parallel)
}

/// Whether `target` is reachable from `from` via *normal* successors. Used to
/// test whether a Catch path eventually runs the compensator (the compensation
/// chain is wired with ordinary `Next` edges).
fn reaches_state(wf: &Workflow, from: &str, target: &str) -> bool {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut stack = vec![from.to_string()];
    while let Some(cur) = stack.pop() {
        if cur == target {
            return true;
        }
        if !seen.insert(cur.clone()) {
            continue;
        }
        let Some(st) = wf.states.get(&cur) else { continue };
        for s in st.normal_successors() {
            stack.push(s.to_string());
        }
    }
    false
}

/// Whether `q`'s *generic* failure is caught and routed to `comp`: it has a Catch
/// covering a broad error class (`States.ALL`/`States.TaskFailed`, i.e. the class
/// any task failure falls into) whose target reaches `comp`. A narrow Catch (a
/// specific custom error only) leaves the generic failure uncaught, so it does
/// not count.
fn failure_reaches(wf: &Workflow, q: &State, comp: &str) -> bool {
    q.catch.iter().any(|c| {
        c.error_equals.iter().any(|e| e == "States.ALL" || e == "States.TaskFailed")
            && reaches_state(wf, &c.next, comp)
    })
}

/// The first state in `start`'s post-commit region whose failure (or a normal
/// path to a `Fail` terminal) escapes the compensator `comp`, or `None` if every
/// post-commit failure is compensated (the workflow is Saga-complete for
/// `start`). This is the decision procedure behind SC4011.
fn downstream_escape(wf: &Workflow, start: &str, comp: &str) -> Option<String> {
    for q in post_commit_region(wf, start, comp) {
        let Some(st) = wf.states.get(&q) else { continue };
        // A normal-flow path to a Fail terminal aborts the execution with the
        // effect already committed and never having reached `comp`.
        if matches!(st.kind, StateKind::Fail) {
            return Some(q);
        }
        if is_fallible(st) && !failure_reaches(wf, st, comp) {
            return Some(q);
        }
    }
    None
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
