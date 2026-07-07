//! Semantic annotations: the layer that recovers the information Amazon States
//! Language omits (idempotency, persistence/compensation, business typestates,
//! task I/O schemas).
//!
//! Two sources, in priority order:
//!   1. an explicit **sidecar** file (TOML) — authoritative, declared by a human
//!      or emitted by the typed DSL frontend;
//!   2. **inference** from task names / `Resource` ARNs — a zero-effort default
//!      whose accuracy is measured in the evaluation.
//!
//! Resolution writes results onto each task's [`crate::ir::Anno`]; passes read
//! only the IR. Findings derived from inferred (vs declared) facts are reported
//! at a lower severity by the passes (see `confidence`-based severity).

use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Sidecar {
    #[serde(default)]
    pub workflow: WorkflowAnnot,
    #[serde(default)]
    pub schemas: BTreeMap<String, SchemaDef>,
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskAnnot>,
    /// Allowed business-state transitions, declaring the workflow protocol.
    #[serde(default)]
    pub protocol: Vec<ProtocolEdge>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct WorkflowAnnot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct TaskAnnot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct SchemaDef {
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProtocolEdge {
    pub from: String,
    pub to: String,
}

impl Sidecar {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("reading sidecar {}", path.display()))?;
        toml::from_str(&src).context("parsing sidecar TOML")
    }
}

/// Resolve annotations onto a workflow (recursively into sub-machines).
pub fn resolve(wf: &mut Workflow, sidecar: Option<&Sidecar>, infer: bool, sink: &mut DiagnosticSink) {
    if let Some(sc) = sidecar {
        validate_sidecar(wf, sc, sink);
        if let Some(is) = &sc.workflow.input_schema {
            wf.input_schema = Some(is.clone());
        }
        wf.protocol = sc.protocol.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
    }
    // Resolve the declared workflow-input schema name to its field set so the
    // data-flow analysis can seed the start document as a closed record.
    if wf.input_fields.is_none() {
        if let Some(f) = schema_fields(sidecar, &wf.input_schema) {
            wf.input_fields = Some(f);
        }
    }
    resolve_machine(wf, sidecar, infer);
}

fn validate_sidecar(wf: &Workflow, sc: &Sidecar, sink: &mut DiagnosticSink) {
    let mut states = BTreeSet::new();
    let mut tasks = BTreeSet::new();
    collect_state_names(wf, &mut states, &mut tasks);

    for (name, task) in &sc.tasks {
        if !states.contains(name) {
            sink.push(
                Diagnostic::error(
                    "SC0011",
                    name,
                    format!("sidecar references unknown state/task '{name}'"),
                )
                .with_note("remove the stale [tasks] entry or rename it to a Task state in the workflow"),
            );
        } else if !tasks.contains(name) {
            sink.push(
                Diagnostic::error(
                    "SC0011",
                    name,
                    format!("sidecar task annotation '{name}' names a non-Task state"),
                )
                .with_note("sidecar [tasks] entries only apply to Task states"),
            );
        }

        if let Some(compensation) = &task.compensation {
            if !states.contains(compensation) {
                sink.push(
                    Diagnostic::error(
                        "SC0011",
                        name,
                        format!(
                            "sidecar task '{name}' references unknown compensation state '{compensation}'"
                        ),
                    )
                    .with_note("rename the compensation reference or add the missing state"),
                );
            }
        }

        check_schema_ref(&sc.schemas, name, "input_schema", &task.input_schema, sink);
        check_schema_ref(&sc.schemas, name, "output_schema", &task.output_schema, sink);
    }

    check_schema_ref(
        &sc.schemas,
        "workflow",
        "input_schema",
        &sc.workflow.input_schema,
        sink,
    );
}

fn collect_state_names(wf: &Workflow, states: &mut BTreeSet<String>, tasks: &mut BTreeSet<String>) {
    for (name, st) in &wf.states {
        states.insert(name.clone());
        if st.is_task() {
            tasks.insert(name.clone());
        }
        if let Some(it) = &st.iterator {
            collect_state_names(it, states, tasks);
        }
        for br in &st.branches {
            collect_state_names(br, states, tasks);
        }
    }
}

fn check_schema_ref(
    schemas: &BTreeMap<String, SchemaDef>,
    owner: &str,
    field: &str,
    schema: &Option<String>,
    sink: &mut DiagnosticSink,
) {
    if let Some(name) = schema {
        if !schemas.contains_key(name) {
            sink.push(
                Diagnostic::error(
                    "SC0012",
                    owner,
                    format!("sidecar {owner}.{field} references unknown schema '{name}'"),
                )
                .with_note("define the schema in [schemas] or correct the schema name"),
            );
        }
    }
}

fn resolve_machine(wf: &mut Workflow, sidecar: Option<&Sidecar>, infer: bool) {
    for st in wf.states.values_mut() {
        if st.is_task() {
            resolve_task(st, sidecar, infer);
        }
        if let Some(it) = st.iterator.as_mut() {
            resolve_machine(it, sidecar, infer);
        }
        for br in st.branches.iter_mut() {
            resolve_machine(br, sidecar, infer);
        }
    }
}

fn schema_fields(sidecar: Option<&Sidecar>, name: &Option<String>) -> Option<Vec<String>> {
    let name = name.as_ref()?;
    let sc = sidecar?;
    sc.schemas.get(name).map(|s| s.fields.clone())
}

fn resolve_task(st: &mut State, sidecar: Option<&Sidecar>, infer: bool) {
    // 1. explicit sidecar entry (authoritative)
    let explicit = sidecar.and_then(|sc| sc.tasks.get(&st.name)).cloned();
    if let Some(a) = &explicit {
        if a.idempotent.is_some() {
            st.anno.idempotent = a.idempotent;
        }
        if a.persistent.is_some() {
            st.anno.persistent = a.persistent;
        }
        st.anno.compensation = a.compensation.clone().or(st.anno.compensation.take());
        st.anno.state_in = a.state_in.clone().or(st.anno.state_in.take());
        st.anno.state_out = a.state_out.clone().or(st.anno.state_out.take());
        st.anno.input_schema = a.input_schema.clone().or(st.anno.input_schema.take());
        st.anno.output_schema = a.output_schema.clone().or(st.anno.output_schema.take());
        st.anno.effect = a.effect.clone().or(st.anno.effect.take());
    }
    // Resolve schema names to field sets, but do not clobber fields a frontend
    // already supplied directly (e.g. the CNCF frontend reads inline JSON Schema).
    if let Some(f) = schema_fields(sidecar, &st.anno.input_schema) {
        st.anno.input_fields = Some(f);
    }
    if let Some(f) = schema_fields(sidecar, &st.anno.output_schema) {
        st.anno.output_fields = Some(f);
    }

    // 2. inference fills the gaps left by explicit annotations
    let need_idem = st.anno.idempotent.is_none();
    let need_pers = st.anno.persistent.is_none();
    if infer && (need_idem || need_pers) {
        let sig = task_signal(st);
        let inf = infer_effect(&sig);
        if need_idem && inf.idempotent.is_some() {
            st.anno.idempotent = inf.idempotent;
            st.anno.inferred = true;
        }
        if need_pers && inf.persistent.is_some() {
            st.anno.persistent = inf.persistent;
            st.anno.inferred = true;
        }
        if st.anno.effect.is_none() {
            st.anno.effect = inf.effect;
        }
    }
}

/// Build the lowercased text signal used by inference: state name + Resource +
/// the Lambda `FunctionName` (or `StateMachineArn`) parameter, if present.
fn task_signal(st: &State) -> String {
    let mut parts = vec![st.name.clone()];
    if let Some(r) = &st.resource {
        parts.push(r.clone());
    }
    if let Some(Value::Object(p)) = &st.parameters {
        for key in ["FunctionName", "StateMachineArn", "QueueUrl", "TopicArn", "TableName"] {
            if let Some(Value::String(s)) = p.get(key) {
                parts.push(s.clone());
            }
        }
    }
    parts.join(" ").to_lowercase()
}

pub struct Inferred {
    pub idempotent: Option<bool>,
    pub persistent: Option<bool>,
    pub effect: Option<String>,
}

/// Classify a task by the naming/resource heuristic (used by the mutator to
/// find applicable injection sites without a full resolve pass).
pub fn classify(st: &State) -> Inferred {
    infer_effect(&task_signal(st))
}

// Read-only / safe-to-repeat operations.
const READ_KW: &[&str] = &[
    "get", "read", "list", "describe", "fetch", "query", "scan", "lookup", "search",
    "check", "validate", "verify", "poll", "status", "detect", "analyz", "classif",
    "count", "head", "exists", "head", "getitem", "batchget",
];
// Side-effecting / not-safe-to-blindly-repeat operations.
const WRITE_KW: &[&str] = &[
    "create", "put", "post", "charge", "pay", "bill", "send", "email", "publish",
    "notify", "submit", "reserve", "book", "provision", "launch", "insert", "register",
    "order", "ship", "transfer", "withdraw", "deposit", "allocate", "upload", "write",
    "refund", "cancel", "delete", "remove", "terminate", "startexecution", "sendmessage",
    "sendemail", "putevents", "putitem", "updateitem", "deleteitem", "invokemodel",
];
// Resource-acquiring effects that require a compensating action (Saga).
const RESERVE_KW: &[&str] = &[
    "reserve", "book", "charge", "pay", "payment", "provision", "allocate", "create",
    "register", "putitem", "insert", "transfer", "withdraw", "deposit", "launch",
    "startexecution", "acquire", "lock", "ship",
];
// Fire-and-forget notifications: side-effecting (non-idempotent) but NOT a
// durable resource acquisition, so they do not require compensation. This
// override prevents "Notify …" states from being mislabelled persistent.
const NOTIFY_KW: &[&str] = &[
    "notify", "alert", "publish", "putevents", ":sns:", ":ses:", "sendemail", "emit",
];

// Undo / compensating actions: what a Saga runs on an error path to roll back a
// prior persistent effect. Used by the effect-aware compensation check (SC4010)
// to decide whether a Catch path actually compensates rather than just logging or
// failing. Evaluated only on error-handler-reachable tasks, so the broad verbs
// (delete/terminate) read as cleanup there.
const UNDO_KW: &[&str] = &[
    "refund", "cancel", "release", "rollback", "compensat", "undo", "revert",
    "restore", "deprovision", "deregister", "terminate", "cleanup", "delete",
    "remove", "abort", "void", "reverse",
];

/// Whether a task looks like a compensating/undo action (name/resource heuristic).
pub fn is_compensator(st: &State) -> bool {
    let sig = task_signal(st);
    UNDO_KW.iter().any(|k| sig.contains(k))
}

fn matches(sig: &str, kws: &[&str]) -> Option<String> {
    kws.iter().find(|k| sig.contains(**k)).map(|k| k.to_string())
}

/// The naming/resource heuristic. Documented and configurable; its accuracy is
/// measured against a hand-labeled gold set in the evaluation.
pub fn infer_effect(sig: &str) -> Inferred {
    let read = matches(sig, READ_KW);
    let write = matches(sig, WRITE_KW);
    let reserve = matches(sig, RESERVE_KW);
    let notify = matches(sig, NOTIFY_KW);

    // Idempotency: a write/notify keyword dominates (non-idempotent); else a
    // read keyword implies idempotent; otherwise unknown.
    let idempotent = if write.is_some() || notify.is_some() {
        Some(false)
    } else if read.is_some() {
        Some(true)
    } else {
        None
    };
    // Persistence: notifications never need compensation; otherwise only
    // resource-acquiring effects do.
    let persistent = if notify.is_some() {
        Some(false)
    } else if reserve.is_some() {
        Some(true)
    } else if write.is_some() || read.is_some() {
        Some(false)
    } else {
        None
    };
    let effect = if notify.is_some() {
        notify
    } else {
        reserve.or(write).or(read)
    };
    Inferred { idempotent, persistent, effect }
}
