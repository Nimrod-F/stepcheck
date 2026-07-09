//! Error injection (mutation testing). Each mutation introduces exactly one
//! defect of a known class into a real workflow, so the evaluation can measure
//! whether the corresponding check detects it (recall) without the checker ever
//! being told where the defect is.
//!
//! Mutations operate on top-level states and re-emit ASL, so the output is a
//! genuine, deployable-shaped workflow that differs from the original by one
//! localized edit.

use crate::annot::classify;
use crate::ir::*;
use clap::ValueEnum;
use serde_json::Value;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum MutationKind {
    /// Break a JSONPath data binding (detected by the contract pass, SC1003).
    Contract,
    /// Redirect a `.$` reference to a field no state produces (data-flow, SC1101).
    Dataflow,
    /// Add a broad retry to a non-idempotent task (SC3001).
    Retry,
    /// Remove the error handling from a persistent task (SC4001).
    Compensation,
    /// Retarget a transition to a non-existent state (SC0002).
    Structural,
    /// Make two Parallel branches write the same resource (SC5001).
    Concurrency,
    /// Set HeartbeatSeconds >= TimeoutSeconds on a task (SC6003).
    Temporal,
}

impl MutationKind {
    /// The diagnostic code that should catch this defect class.
    pub fn expected_code(&self) -> &'static str {
        match self {
            MutationKind::Contract => "SC1003",
            MutationKind::Dataflow => "SC1101",
            MutationKind::Retry => "SC3001",
            MutationKind::Compensation => "SC4001",
            MutationKind::Structural => "SC0002",
            MutationKind::Concurrency => "SC5001",
            MutationKind::Temporal => "SC6003",
        }
    }
    /// The defect classes injected over the raw corpus (the in-the-wild mutation
    /// study). `Dataflow` is excluded here because its detection depends on a
    /// declared/constructed record shape (the typed tier); it is evaluated
    /// separately on the typed demonstrators.
    pub fn all() -> [MutationKind; 6] {
        [
            MutationKind::Contract,
            MutationKind::Retry,
            MutationKind::Compensation,
            MutationKind::Structural,
            MutationKind::Concurrency,
            MutationKind::Temporal,
        ]
    }
}

/// Apply one mutation, returning the mutated workflow IR (or `None` if the
/// workflow has no applicable injection site for this class).
pub fn mutate(wf: &Workflow, kind: MutationKind, seed: u64) -> Option<Workflow> {
    let mut w = wf.clone();
    let ok = match kind {
        MutationKind::Contract => mutate_contract(&mut w, seed),
        MutationKind::Dataflow => mutate_dataflow(&mut w, seed),
        MutationKind::Retry => mutate_retry(&mut w, seed),
        MutationKind::Compensation => mutate_compensation(&mut w, seed),
        MutationKind::Structural => mutate_structural(&mut w, seed),
        MutationKind::Concurrency => mutate_concurrency(&mut w, seed),
        MutationKind::Temporal => mutate_temporal(&mut w, seed),
    };
    if ok {
        Some(w)
    } else {
        None
    }
}

fn pick(sites: &[String], seed: u64) -> Option<String> {
    if sites.is_empty() {
        None
    } else {
        Some(sites[(seed as usize) % sites.len()].clone())
    }
}

/// Add `Retry: [{ErrorEquals: [States.ALL], ...}]` to a non-idempotent task
/// that does not already retry broadly.
fn mutate_retry(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| {
            s.is_task()
                && classify(s).idempotent == Some(false)
                && !s
                    .retry
                    .iter()
                    .flat_map(|r| &r.error_equals)
                    .any(|e| e == "States.ALL" || e == "States.TaskFailed")
        })
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    st.retry.push(RetryRule {
        error_equals: vec!["States.ALL".into()],
        max_attempts: Some(3),
        interval_seconds: Some(1.0),
        backoff_rate: Some(2.0),
        extra: serde_json::Map::new(),
    });
    true
}

/// Strip the error handling / compensation from a persistent task.
fn mutate_compensation(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task() && classify(s).persistent == Some(true) && !s.catch.is_empty())
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    st.catch.clear();
    st.anno.compensation = None;
    true
}

/// Retarget a state's `Next` to a non-existent state.
fn mutate_structural(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.next.is_some())
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    st.next = Some("__StepCheckMissingState__".into());
    true
}

/// Break a JSONPath payload (`"x.$": "$.foo"` -> `"x.$": "foo"`).
fn mutate_contract(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.parameters.as_ref().map(has_breakable_path).unwrap_or(false))
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    if let Some(p) = st.parameters.as_mut() {
        break_first_path(p);
    }
    true
}

/// Redirect a `.$` reference to a field no state produces (`$.x` -> a fresh
/// absent field). Unlike `mutate_contract` this keeps the value a *valid* path,
/// so it escapes the syntactic check and is caught only by the provenance
/// analysis (SC1101) when the document shape is known (typed tier).
fn mutate_dataflow(w: &mut Workflow, seed: u64) -> bool {
    let mut sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.parameters.as_ref().map(has_simple_path).unwrap_or(false))
        .map(|(n, _)| n.clone())
        .collect();
    // Prefer the start state: its document is the (typed) input record, so the
    // injected miss falls within the analysis's remit — the same discipline the
    // other mutation classes follow.
    if let Some(pos) = sites.iter().position(|n| n == &w.start_at) {
        sites.swap(0, pos);
    }
    let Some(name) = pick(&sites, if sites.first().map(|n| n == &w.start_at).unwrap_or(false) { 0 } else { seed }) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    if let Some(p) = st.parameters.as_mut() {
        redirect_first_path(p);
    }
    true
}

/// Inject a shared write into two Parallel branches so they contend on one
/// resource (caught by the concurrency-interference pass, SC5001).
fn mutate_concurrency(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.kind == StateKind::Parallel && s.branches.len() >= 2)
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    prepend_write(&mut st.branches[0], "__SCWriteA", "StepCheckSharedTable");
    prepend_write(&mut st.branches[1], "__SCWriteB", "StepCheckSharedTable");
    true
}

fn prepend_write(branch: &mut Workflow, sname: &str, table: &str) {
    let mut st = State::new(sname, StateKind::Task);
    st.resource = Some("arn:aws:states:::dynamodb:putItem".into());
    // Static item key (no `.$`) so the injected state introduces *only* the
    // concurrency defect, not an incidental missing-field (SC1101) read.
    st.parameters = Some(serde_json::json!({ "TableName": table, "Item": { "id": "fixed" } }));
    // Mark it an overwriting write but explicitly non-persistent, so it isolates
    // SC5001 without also triggering the uncompensated-persistent check (SC4001) ---
    // keeping the mutation-study confusion matrix diagonal.
    st.anno.idempotent = Some(false);
    st.anno.persistent = Some(false);
    st.next = Some(branch.start_at.clone());
    branch.states.insert(sname.to_string(), st);
    branch.start_at = sname.to_string();
}

/// Set `HeartbeatSeconds >= TimeoutSeconds` on a task (caught by SC6003).
fn mutate_temporal(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task())
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    st.timeout_seconds = Some(10.0);
    st.heartbeat_seconds = Some(20.0);
    true
}

fn has_simple_path(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, val)| {
            (k.ends_with(".$")
                && val.as_str().map(|s| s.starts_with('$') && s.len() > 1).unwrap_or(false))
                || has_simple_path(val)
        }),
        Value::Array(a) => a.iter().any(has_simple_path),
        _ => false,
    }
}

fn redirect_first_path(v: &mut Value) -> bool {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                if k.ends_with(".$") {
                    if let Value::String(s) = val {
                        if s.starts_with('$') && s.len() > 1 {
                            *s = "$.__stepcheck_absent".to_string();
                            return true;
                        }
                    }
                }
                if redirect_first_path(val) {
                    return true;
                }
            }
            false
        }
        Value::Array(a) => {
            for val in a.iter_mut() {
                if redirect_first_path(val) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

fn has_breakable_path(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, val)| {
            (k.ends_with(".$") && val.as_str().map(|s| s.starts_with('$')).unwrap_or(false))
                || has_breakable_path(val)
        }),
        Value::Array(a) => a.iter().any(has_breakable_path),
        _ => false,
    }
}

fn break_first_path(v: &mut Value) -> bool {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                if k.ends_with(".$") {
                    if let Value::String(s) = val {
                        if let Some(stripped) = s.strip_prefix('$') {
                            *s = format!("BROKEN{stripped}");
                            return true;
                        }
                    }
                }
                if break_first_path(val) {
                    return true;
                }
            }
            false
        }
        Value::Array(a) => {
            for val in a.iter_mut() {
                if break_first_path(val) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}
