//! Concurrency-interference analysis (codes `SC5xxx`).
//!
//! ASL's `Parallel` runs its branches concurrently and `Map` runs its iterations
//! concurrently (`MaxConcurrency` != 1). Each branch/iteration receives its own
//! *copy* of the state document, so the document itself cannot race — but the
//! *external* resources the tasks touch can. Two branches that both write the
//! same DynamoDB table, S3 object, SQS queue or SNS topic, or concurrent Map
//! iterations that all write one shared resource, produce an order-dependent or
//! duplicated effect. No prior ASL tool reasons about this; the interference
//! literature does so for actors and distributed objects but not for serverless
//! fan-out.
//!
//! These are heuristic *warnings* (a shared write may be intentional and
//! correctly keyed), in line with the tool's confidence model.

use super::retry::qualify;
use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{State, StateKind, Workflow};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct ConcurrencyPass;

impl Pass for ConcurrencyPass {
    fn id(&self) -> &'static str {
        "concurrency"
    }
    fn title(&self) -> &'static str {
        "Concurrency-interference analysis"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", sink);
    }
}

/// A *stateful data resource* a task mutates, keyed by the target parameter that
/// names it. Reads and pure compute do not contend; only persistent / non-
/// idempotent writes to a shared store interfere, so the caller gates on that.
fn data_resource(st: &State) -> Option<(String, String)> {
    let p = match &st.parameters {
        Some(Value::Object(p)) => p,
        _ => return None,
    };
    for (svc, key) in [
        ("dynamodb", "TableName"),
        ("sqs", "QueueUrl"),
        ("sns", "TopicArn"),
        ("s3", "Bucket"),
    ] {
        if let Some(Value::String(s)) = p.get(key) {
            return Some((svc.to_string(), s.clone()));
        }
    }
    None
}

fn is_write(st: &State) -> bool {
    st.anno.idempotent == Some(false) || st.anno.persistent == Some(true)
}

/// Lowercased service action of an integration ARN
/// (`arn:aws:states:::dynamodb:putItem` -> `putitem`).
fn action(st: &State) -> String {
    st.resource
        .as_ref()
        .map(|r| {
            r.to_lowercase()
                .split(['.', ':'])
                .filter(|t| !t.is_empty())
                .last()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}

/// An \emph{overwriting} write replaces a whole item/object (last-writer-wins),
/// so two of them to the same resource genuinely race. A merging write
/// (`updateItem`) is atomic and commutes for disjoint attributes, and a
/// fire-and-forget publish/send delivers independent messages---neither is a
/// lost-update race, so we exclude them to avoid false positives on idiomatic
/// keyed/commuting patterns.
fn is_overwriting_write(st: &State) -> bool {
    if !st.is_task() || !is_write(st) {
        return false;
    }
    let a = action(st);
    a.contains("putitem") || a.contains("putobject")
}

/// Whether a write threads per-item / per-record data into its payload (any `.$`
/// reference), i.e. successive invocations are distinguished and unlikely to
/// clobber one another.
fn threads_dynamic_data(st: &State) -> bool {
    fn scan(v: &Value) -> bool {
        match v {
            Value::Object(m) => m.iter().any(|(k, val)| k.ends_with(".$") || scan(val)),
            Value::Array(a) => a.iter().any(scan),
            _ => false,
        }
    }
    st.parameters.as_ref().map(scan).unwrap_or(false)
}

/// Collect the overwriting writes performed anywhere inside a (sub-)machine.
fn writes_in(wf: &Workflow) -> Vec<(String, (String, String))> {
    let mut out = Vec::new();
    wf.walk_machines(&mut |m| {
        for (name, st) in &m.states {
            if is_overwriting_write(st) {
                if let Some(rk) = data_resource(st) {
                    out.push((name.clone(), rk));
                }
            }
        }
    });
    out
}

fn run_machine(wf: &Workflow, scope: &str, sink: &mut DiagnosticSink) {
    for (name, st) in &wf.states {
        match st.kind {
            // SC5001: two Parallel branches write the same stateful resource.
            StateKind::Parallel => {
                // resource -> list of (branch index, state)
                let mut by_res: BTreeMap<(String, String), Vec<(usize, String)>> = BTreeMap::new();
                for (i, br) in st.branches.iter().enumerate() {
                    for (sname, rk) in writes_in(br) {
                        by_res.entry(rk).or_default().push((i, sname));
                    }
                }
                for (rk, sites) in &by_res {
                    let branches: std::collections::BTreeSet<usize> =
                        sites.iter().map(|(b, _)| *b).collect();
                    if branches.len() >= 2 {
                        let states: Vec<String> = sites.iter().map(|(b, s)| format!("branch{b}:{s}")).collect();
                        sink.push(
                            Diagnostic::warning(
                                "SC5001",
                                &qualify(scope, name),
                                format!(
                                    "parallel branches overwrite the same {} resource '{}' ({}) — concurrent last-writer-wins may interfere",
                                    rk.0, rk.1, states.join(", ")
                                ),
                            )
                            .with_note(
                                "Parallel branches run concurrently; ensure the writes target disjoint keys",
                            ),
                        );
                    }
                }
            }
            // SC5002: concurrent Map iterations perform an overwriting write to
            // one shared resource without threading any per-item data (so every
            // iteration clobbers the same item).
            StateKind::Map => {
                let concurrent = st.max_concurrency != Some(1);
                if concurrent {
                    if let Some(it) = &st.iterator {
                        for (sname, st2) in collect_writes(it) {
                            if !threads_dynamic_data(&st2) {
                                if let Some(rk) = data_resource(&st2) {
                                    sink.push(
                                        Diagnostic::warning(
                                            "SC5002",
                                            &qualify(scope, &format!("{name}[Map]/{sname}")),
                                            format!(
                                                "concurrent Map iterations overwrite shared {} resource '{}' without an item-specific key",
                                                rk.0, rk.1
                                            ),
                                        )
                                        .with_note(
                                            "set MaxConcurrency: 1 or key each write by the item to avoid interference",
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        // recurse so nested Parallel/Map are covered
        recurse(st, scope, sink);
    }
}

fn collect_writes(wf: &Workflow) -> Vec<(String, State)> {
    let mut out = Vec::new();
    wf.walk_machines(&mut |m| {
        for (name, st) in &m.states {
            if is_overwriting_write(st) {
                out.push((name.clone(), st.clone()));
            }
        }
    });
    out
}

fn recurse(st: &State, scope: &str, sink: &mut DiagnosticSink) {
    if let Some(it) = &st.iterator {
        run_machine(it, &qualify(scope, &format!("{}[Map]", st.name)), sink);
    }
    for (i, br) in st.branches.iter().enumerate() {
        run_machine(br, &qualify(scope, &format!("{}[Branch{i}]", st.name)), sink);
    }
}
