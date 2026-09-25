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

    /// Diagnostic codes that legitimately catch this defect class, *including
    /// sibling codes in the same analysis family*. Used by the hard-mutant study:
    /// a defect caught by a sibling check (e.g. a non-compensating `Catch` caught
    /// by the effect-aware `SC4010` rather than the presence-only `SC4001`) is
    /// still a detection, and crediting only the single expected code would
    /// understate the tool while overstating operator--check coupling.
    pub fn expected_family(&self) -> &'static [&'static str] {
        match self {
            MutationKind::Contract => &["SC1003"],
            MutationKind::Dataflow => &["SC1101"],
            MutationKind::Retry => &["SC3001"],
            MutationKind::Compensation => &["SC4001", "SC4010", "SC4011"],
            MutationKind::Structural => &["SC0002"],
            MutationKind::Concurrency => &["SC5001", "SC5002", "SC5003"],
            MutationKind::Temporal => &["SC6003"],
        }
    }

    /// All seven classes, for the hard-mutant study. Unlike [`all`], this includes
    /// `Dataflow` (its hard variant is run in the typed tier, where the class is
    /// meaningful) so every check with an over-approximating (⊤-lifting) or
    /// coverage boundary is stressed by a mutant sitting just past that boundary.
    pub fn all_hard() -> [MutationKind; 7] {
        [
            MutationKind::Contract,
            MutationKind::Dataflow,
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

/// Apply one *hard* mutation: the same semantic defect class as [`mutate`], but
/// deliberately placed just past the boundary of what the corresponding analysis
/// can prove — behind a construct the analysis over-approximates to `⊤`
/// (dynamically-named resources, reference-path timeouts, filter-expression
/// re-roots) or outside a check's syntactic reach (a `ResultSelector`). Each hard
/// mutant is still a genuine, deployable defect of its class. Whether StepCheck
/// catches it — via the expected code, via a sibling in [`expected_family`], or
/// not at all because it soundly lifts to `⊤` — is exactly what the hard-mutant
/// study measures. Returns `None` when the workflow has no applicable site.
pub fn mutate_hard(wf: &Workflow, kind: MutationKind, seed: u64) -> Option<Workflow> {
    let mut w = wf.clone();
    let ok = match kind {
        MutationKind::Contract => mutate_contract_hard(&mut w, seed),
        MutationKind::Dataflow => mutate_dataflow_hard(&mut w, seed),
        MutationKind::Retry => mutate_retry_hard(&mut w, seed),
        MutationKind::Compensation => mutate_compensation_hard(&mut w, seed),
        MutationKind::Structural => mutate_structural_hard(&mut w, seed),
        MutationKind::Concurrency => mutate_concurrency_hard(&mut w, seed),
        MutationKind::Temporal => mutate_temporal_hard(&mut w, seed),
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

// ===========================================================================
// Hard mutants: genuine defects placed at each analysis's boundary.
// ===========================================================================

/// **Concurrency (SC5001), ⊤ via dynamic resource identity.** Two parallel
/// branches overwrite one table, but the table is named by an input field
/// (`"TableName.$": "$.sharedTable"`) rather than a literal. The interference
/// analysis resolves shared resources only through statically-known names, so it
/// cannot prove the two branches contend and soundly stays silent — yet at run
/// time both branches read the *same* field and write the *same* table: a real
/// last-writer-wins race.
fn mutate_concurrency_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.kind == StateKind::Parallel && s.branches.len() >= 2)
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    prepend_write_dynamic(&mut st.branches[0], "__SCWriteA", "$.sharedTable");
    prepend_write_dynamic(&mut st.branches[1], "__SCWriteB", "$.sharedTable");
    true
}

fn prepend_write_dynamic(branch: &mut Workflow, sname: &str, table_ref: &str) {
    let mut st = State::new(sname, StateKind::Task);
    st.resource = Some("arn:aws:states:::dynamodb:putItem".into());
    // Dynamically-named target: the resource identity is not statically
    // resolvable (`data_resource` returns `None`), so the branches' contention is
    // invisible to SC5001 even though both resolve `$.sharedTable` to one table.
    st.parameters = Some(serde_json::json!({ "TableName.$": table_ref, "Item": { "id": "fixed" } }));
    st.anno.idempotent = Some(false);
    st.anno.persistent = Some(false);
    st.next = Some(branch.start_at.clone());
    branch.states.insert(sname.to_string(), st);
    branch.start_at = sname.to_string();
}

/// **Temporal (SC6003), ⊤ via reference-path values.** The same defect as the
/// easy temporal mutant (heartbeat not shorter than the task timeout), but the
/// two durations are supplied through `HeartbeatSecondsPath` / `TimeoutSecondsPath`
/// reference paths instead of integer literals, so the constant comparison
/// SC6003 performs is unavailable and the check lifts to ⊤. The misconfiguration
/// is real at run time.
fn mutate_temporal_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task())
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    st.timeout_seconds = None;
    st.heartbeat_seconds = None;
    st.extra.insert("HeartbeatSecondsPath".into(), Value::String("$.heartbeat".into()));
    st.extra.insert("TimeoutSecondsPath".into(), Value::String("$.timeout".into()));
    true
}

/// **Contract (SC1003), generalization control via `ResultSelector`.** The same
/// syntactically invalid `.$` payload as the easy contract mutant, but placed in a
/// `ResultSelector` instead of `Parameters`. The scan reads every payload template
/// (`Parameters`, `ResultSelector`, a Distributed `Map`'s `ItemSelector`, `Assign`)
/// and `Choice` guards, so this should still be caught — the point is to confirm the
/// check is not tuned to where the easy operator puts the break.
fn mutate_contract_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task())
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    if let Some(rs) = st.result_selector.as_mut() {
        if break_first_path(rs) {
            return true;
        }
    }
    st.result_selector = Some(serde_json::json!({ "picked.$": "BROKEN.notapath" }));
    true
}

/// **Structural (SC0002), generalization control.** Retarget a `Next` *inside a
/// nested Parallel branch or Map iterator* to a missing state. A non-recursive
/// reachability check would miss a dangling edge in a sub-machine scope; our
/// structural pass recurses, so this hard mutant should still be caught — the
/// point is to confirm the check is not tuned to the top-level-only easy operator.
fn mutate_structural_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| matches!(s.kind, StateKind::Parallel | StateKind::Map))
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let st = w.states.get_mut(&name).unwrap();
    let mut machines: Vec<&mut Workflow> = st.branches.iter_mut().collect();
    if let Some(it) = st.iterator.as_deref_mut() {
        machines.push(it);
    }
    for m in machines {
        let inner = m.states.iter().find(|(_, s)| s.next.is_some()).map(|(n, _)| n.clone());
        if let Some(iname) = inner {
            m.states.get_mut(&iname).unwrap().next = Some("__StepCheckMissingState__".into());
            return true;
        }
    }
    false
}

/// **Compensation (SC4001 family), sibling detection via SC4010.** A persistent
/// task that *does* have error handling, but whose `Catch` routes to a state that
/// only logs rather than compensating. A presence-only rule (SC4001) accepts it;
/// the effect-aware sibling SC4010 is designed for exactly this, so the family is
/// expected to catch it even though the *expected* code SC4001 does not fire.
fn mutate_compensation_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task() && classify(s).persistent == Some(true))
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    let handler = unique_name(w, "SCLogOnly");
    let mut log = State::new(&handler, StateKind::Pass);
    log.end = true;
    w.states.insert(handler.clone(), log);
    let st = w.states.get_mut(&name).unwrap();
    st.catch = vec![CatchRule {
        error_equals: vec!["States.ALL".into()],
        next: handler,
        result_path: ResultPath::Default,
        assign: None,
        extra: serde_json::Map::new(),
    }];
    st.anno.compensation = None;
    true
}

/// **Retry (SC3001), inference-tier boundary.** A genuinely non-idempotent task
/// (selected by the same classifier the easy operator uses) given a broad retry,
/// but with the state name and `FunctionName` genericized to uninformative
/// labels — the realistic case of a side-effecting Lambda with an opaque name.
/// The naming heuristic can then no longer prove non-idempotence, so SC3001
/// soundly stays silent; the duplicate-effect defect is nonetheless real. (When
/// the task carries a direct SDK integration whose action still signals a write,
/// e.g. `dynamodb:putItem`, the class is caught anyway — a generalization case.)
fn mutate_retry_hard(w: &mut Workflow, seed: u64) -> bool {
    let sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.is_task() && classify(s).idempotent == Some(false))
        .map(|(n, _)| n.clone())
        .collect();
    let Some(name) = pick(&sites, seed) else { return false };
    {
        let st = w.states.get_mut(&name).unwrap();
        st.retry.push(RetryRule {
            error_equals: vec!["States.ALL".into()],
            max_attempts: Some(3),
            interval_seconds: Some(1.0),
            backoff_rate: Some(2.0),
            extra: serde_json::Map::new(),
        });
        if let Some(Value::Object(p)) = st.parameters.as_mut() {
            for key in ["FunctionName", "StateMachineArn", "QueueUrl", "TopicArn", "TableName"] {
                if p.contains_key(key) {
                    p.insert(key.to_string(), Value::String("SCOpaqueTarget".into()));
                }
            }
        }
        st.anno = Anno::default();
    }
    let neutral = unique_name(w, "SCOpaqueTask");
    rename_state(w, &name, &neutral);
    true
}

/// **Data-flow (SC1101), ⊤ via a filter-expression re-root.** Redirect a `.$`
/// read to an absent field (as the easy operator does), but additionally place an
/// `InputPath` filter expression on the state. The provenance analysis parses only
/// the dotted-path fragment and lifts a filter/recursive selection to ⊤ (`narrow`
/// returns `Top`), so the read is evaluated against an opaque document and comes
/// back `Maybe` — a sound non-report. The field is still absent at run time.
fn mutate_dataflow_hard(w: &mut Workflow, seed: u64) -> bool {
    let mut sites: Vec<String> = w
        .states
        .iter()
        .filter(|(_, s)| s.parameters.as_ref().map(has_simple_path).unwrap_or(false))
        .map(|(n, _)| n.clone())
        .collect();
    if let Some(pos) = sites.iter().position(|n| n == &w.start_at) {
        sites.swap(0, pos);
    }
    let Some(name) = pick(&sites, if sites.first().map(|n| n == &w.start_at).unwrap_or(false) { 0 } else { seed }) else {
        return false;
    };
    let st = w.states.get_mut(&name).unwrap();
    if let Some(p) = st.parameters.as_mut() {
        redirect_first_path(p);
    }
    // A filter-expression InputPath the dotted-fragment parser cannot model: the
    // state's input document lifts to ⊤, masking the injected missing read.
    st.input_path = Some(Value::String("$.stepcheckMask[?(@.present)]".into()));
    true
}

/// Rename a top-level state (map key + `name` field + every intra-machine
/// reference: `StartAt`, `Next`, `Default`, `Choices[].Next`, `Catch[].Next`) so
/// the change survives a round trip through the ASL emitter.
fn rename_state(w: &mut Workflow, old: &str, new: &str) {
    if w.start_at == old {
        w.start_at = new.to_string();
    }
    let entry = w.states.shift_remove(old);
    for st in w.states.values_mut() {
        retarget(st, old, new);
    }
    if let Some(mut st) = entry {
        st.name = new.to_string();
        retarget(&mut st, old, new);
        w.states.insert(new.to_string(), st);
    }
}

fn retarget(st: &mut State, old: &str, new: &str) {
    if st.next.as_deref() == Some(old) {
        st.next = Some(new.to_string());
    }
    if st.default.as_deref() == Some(old) {
        st.default = Some(new.to_string());
    }
    for c in &mut st.choices {
        if c.next == old {
            c.next = new.to_string();
        }
    }
    for c in &mut st.catch {
        if c.next == old {
            c.next = new.to_string();
        }
    }
}

/// A state name not already present in `w` (append a numeric suffix if needed).
fn unique_name(w: &Workflow, base: &str) -> String {
    if !w.states.contains_key(base) {
        return base.to_string();
    }
    (0..).map(|i| format!("{base}{i}")).find(|n| !w.states.contains_key(n)).unwrap()
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
