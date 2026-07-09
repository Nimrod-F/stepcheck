//! Concurrency-interference analysis (codes `SC5xxx`).
//!
//! ASL's `Parallel` runs its branches concurrently and `Map` runs its iterations
//! concurrently (`MaxConcurrency` != 1). Each branch/iteration receives its own
//! *copy* of the state document, so the document itself cannot race — but the
//! *external* resources the tasks touch can. Two branches that both write the
//! same DynamoDB table, S3 object, SQS queue, SNS topic, statically named child
//! execution, or other recognized AWS target, or concurrent Map iterations that
//! all write one shared resource, produce an order-dependent or duplicated
//! effect. No prior ASL tool reasons about this; the interference
//! literature does so for actors and distributed objects but not for serverless
//! fan-out.
//!
//! These are heuristic *warnings* (a shared write may be intentional and
//! correctly keyed), in line with the tool's confidence model.
//!
//! **Composition across `startExecution` (SC5003).** Interference reasoning
//! flattens a branch's *nested* Parallel/Map bodies (they are in the same
//! definition). When a branch invokes a *child* state machine via
//! `states:startExecution`, the child's ARN is a sound composition key: two
//! branches invoking the same child run its effects concurrently (SC5003).
//! If the child's definition is in scope (for example, another state machine in
//! the same CloudFormation/SAM template), the child's own write set is
//! recursively flattened into the parent's SC5001 footprint. When no definition
//! is available, composition remains scoped to the invocation level.

use super::retry::qualify;
use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{State, StateKind, Workflow};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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

    if let Some(arn) = static_param(p, "StateMachineArn") {
        if let Some(name) = static_param(p, "Name") {
            return Some(("sfn-execution".to_string(), format!("{arn}#{name}")));
        }
        return Some(("sfn".to_string(), arn.to_string()));
    }
    if let Some(bus) = static_param(p, "EventBusName").or_else(|| static_entry_param(p, "EventBusName")) {
        return Some(("events".to_string(), bus.to_string()));
    }
    if let Some(cluster) = static_param(p, "Cluster") {
        let key = match static_param(p, "TaskDefinition") {
            Some(task) => format!("{cluster}#{task}"),
            None => cluster.to_string(),
        };
        return Some(("ecs".to_string(), key));
    }
    if let Some(model) = static_param(p, "ModelId") {
        return Some(("bedrock".to_string(), model.to_string()));
    }
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

fn static_param<'a>(p: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    match p.get(key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

fn static_entry_param<'a>(p: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let entries = p.get("Entries")?.as_array()?;
    entries.iter().find_map(|entry| match entry {
        Value::Object(entry) => static_param(entry, key),
        _ => None,
    })
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
    let r = st.resource.as_deref().unwrap_or("").to_lowercase();
    a.contains("putitem")
        || a.contains("putobject")
        || (r.contains(":states:startexecution") && has_static_execution_name(st))
}

fn has_static_execution_name(st: &State) -> bool {
    match &st.parameters {
        Some(Value::Object(p)) => static_param(p, "Name").is_some(),
        _ => false,
    }
}

/// If `st` is a `states:startExecution` that names a *statically resolvable*
/// child state machine, return that child's `StateMachineArn`. Deployment
/// templates leave the ARN as a CloudFormation / JSONata reference
/// (`${ChildStateMachine}`, `{% 'child1' %}`) rather than a literal ARN, but the
/// reference string still identifies the child uniquely within the workflow, so
/// it is a sound composition key: two concurrent invocations of the *same*
/// reference invoke the *same* child.
fn child_execution_arn(st: &State) -> Option<String> {
    let r = st.resource.as_deref().unwrap_or("").to_lowercase();
    if !r.contains(":states:startexecution") {
        return None;
    }
    match &st.parameters {
        Some(Value::Object(p)) => static_param(p, "StateMachineArn").map(|s| s.to_string()),
        _ => None,
    }
}

/// The child state machines a (sub-)machine may invoke via `startExecution`,
/// flattened across its nested Parallel/Map bodies. This is the "external-write
/// footprint" a branch contributes through composition: even without the child's
/// definition, invoking it concurrently runs its effects concurrently.
fn child_invocations_in(wf: &Workflow) -> Vec<(String, String)> {
    let mut out = Vec::new();
    wf.walk_machines(&mut |m| {
        for (name, st) in &m.states {
            if let Some(arn) = child_execution_arn(st) {
                out.push((name.clone(), arn));
            }
        }
    });
    out
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

fn writes_in_with_registry(wf: &Workflow, registry: &BTreeMap<String, Workflow>) -> Vec<(String, (String, String))> {
    let mut visiting = BTreeSet::new();
    writes_in_resolved(wf, registry, &mut visiting)
}

fn writes_in_resolved(
    wf: &Workflow,
    registry: &BTreeMap<String, Workflow>,
    visiting: &mut BTreeSet<String>,
) -> Vec<(String, (String, String))> {
    let mut out = Vec::new();
    wf.walk_machines(&mut |m| {
        for (name, st) in &m.states {
            if is_overwriting_write(st) {
                if let Some(rk) = data_resource(st) {
                    out.push((name.clone(), rk));
                }
            }
            if let Some(arn) = child_execution_arn(st) {
                if !visiting.insert(arn.clone()) {
                    continue;
                }
                if let Some(child) = registry.get(&arn) {
                    for (child_name, rk) in writes_in_resolved(child, registry, visiting) {
                        out.push((format!("{name}->{child_name}"), rk));
                    }
                }
                visiting.remove(&arn);
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
                    for (sname, rk) in writes_in_with_registry(br, wf.linked_children.as_ref()) {
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

                // SC5003: composition across `startExecution`. Two branches that
                // invoke the *same* child state machine run its external effects
                // concurrently. We resolve statically-named child executions and
                // union their invocation footprint across each branch's nested
                // bodies; the child's own writes are then a scoped, flattened
                // side effect (see module note on the intra-child assumption).
                let mut by_child: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
                for (i, br) in st.branches.iter().enumerate() {
                    for (sname, arn) in child_invocations_in(br) {
                        by_child.entry(arn).or_default().push((i, sname));
                    }
                }
                for (arn, sites) in &by_child {
                    let branches: std::collections::BTreeSet<usize> =
                        sites.iter().map(|(b, _)| *b).collect();
                    if branches.len() >= 2 {
                        let states: Vec<String> = sites.iter().map(|(b, s)| format!("branch{b}:{s}")).collect();
                        sink.push(
                            Diagnostic::warning(
                                "SC5003",
                                &qualify(scope, name),
                                format!(
                                    "parallel branches concurrently invoke the same child workflow '{arn}' ({}) — its external effects run twice concurrently",
                                    states.join(", ")
                                ),
                            )
                            .with_note(
                                "startExecution composes: a non-idempotent child run concurrently may duplicate or interleave its writes",
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
