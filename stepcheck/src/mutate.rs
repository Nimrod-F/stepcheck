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
    /// Add a broad retry to a non-idempotent task (SC3001).
    Retry,
    /// Remove the error handling from a persistent task (SC4001).
    Compensation,
    /// Retarget a transition to a non-existent state (SC0002).
    Structural,
}

impl MutationKind {
    /// The diagnostic code that should catch this defect class.
    pub fn expected_code(&self) -> &'static str {
        match self {
            MutationKind::Contract => "SC1003",
            MutationKind::Retry => "SC3001",
            MutationKind::Compensation => "SC4001",
            MutationKind::Structural => "SC0002",
        }
    }
    pub fn all() -> [MutationKind; 4] {
        [
            MutationKind::Contract,
            MutationKind::Retry,
            MutationKind::Compensation,
            MutationKind::Structural,
        ]
    }
}

/// Apply one mutation, returning the mutated workflow IR (or `None` if the
/// workflow has no applicable injection site for this class).
pub fn mutate(wf: &Workflow, kind: MutationKind, seed: u64) -> Option<Workflow> {
    let mut w = wf.clone();
    let ok = match kind {
        MutationKind::Contract => mutate_contract(&mut w, seed),
        MutationKind::Retry => mutate_retry(&mut w, seed),
        MutationKind::Compensation => mutate_compensation(&mut w, seed),
        MutationKind::Structural => mutate_structural(&mut w, seed),
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
