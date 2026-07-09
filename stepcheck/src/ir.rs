//! The format-agnostic workflow intermediate representation (IR).
//!
//! Every frontend (ASL JSON, the typed DSL, …) lowers to a [`Workflow`].
//! Every analysis [`crate::passes::Pass`] reads this IR and nothing else, so
//! checks are independent of the surface syntax a workflow was authored in.

use indexmap::IndexMap;
use serde_json::Value;
use std::collections::BTreeMap;
use std::rc::Rc;

/// The kind of an ASL state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateKind {
    Task,
    Choice,
    Parallel,
    Map,
    Wait,
    Pass,
    Succeed,
    Fail,
    /// An unrecognised `Type`; kept so the frontend never rejects real ASL.
    Unknown(String),
}

impl StateKind {
    pub fn as_str(&self) -> &str {
        match self {
            StateKind::Task => "Task",
            StateKind::Choice => "Choice",
            StateKind::Parallel => "Parallel",
            StateKind::Map => "Map",
            StateKind::Wait => "Wait",
            StateKind::Pass => "Pass",
            StateKind::Succeed => "Succeed",
            StateKind::Fail => "Fail",
            StateKind::Unknown(s) => s,
        }
    }
    pub fn is_terminal_kind(&self) -> bool {
        matches!(self, StateKind::Succeed | StateKind::Fail)
    }
}

/// The expression language a state (or whole machine) uses to read and reshape
/// its document. ASL defaults to JSONPath; a `QueryLanguage: "JSONata"` directive
/// (at the machine or state level) switches to JSONata, whose `Arguments`/
/// `Output`/`Assign` constructs reshape the document with arbitrary expressions
/// the data-flow analysis does not model. We track the mode so the analysis can
/// treat JSONata states *opaquely* (output `Top`, no reference checks), which is
/// what keeps the may-present analysis sound on JSONata workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLang {
    JsonPath,
    JsonAta,
}

/// How a state writes its result back into the state document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultPath {
    /// `ResultPath` absent → defaults to `$` (result replaces the document).
    Default,
    /// `ResultPath: null` → the result is discarded, document passes through.
    Discard,
    /// `ResultPath: "$.x"` → the result is written under `$.x`.
    Path(String),
}

#[derive(Debug, Clone)]
pub struct RetryRule {
    pub error_equals: Vec<String>,
    pub max_attempts: Option<i64>,
    pub interval_seconds: Option<f64>,
    pub backoff_rate: Option<f64>,
    /// Retry fields not interpreted by StepCheck, preserved for ASL re-emission.
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct CatchRule {
    pub error_equals: Vec<String>,
    pub next: String,
    pub result_path: ResultPath,
    pub assign: Option<Value>,
    /// Catch fields not interpreted by StepCheck, preserved for ASL re-emission.
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct ChoiceRule {
    pub next: String,
    /// The raw comparison object (`Variable`/`And`/`Or`/`Not`/…); kept verbatim
    /// so the contract pass can mine its JSONPath references.
    pub condition: Value,
}

/// Semantic annotations recovered for a task — either declared explicitly
/// (DSL / sidecar) or inferred from naming heuristics (see [`crate::annot`]).
#[derive(Debug, Clone, Default)]
pub struct Anno {
    /// `Some(true)` idempotent, `Some(false)` non-idempotent, `None` unknown.
    pub idempotent: Option<bool>,
    /// Whether the task commits a durable/external effect that needs undoing.
    pub persistent: Option<bool>,
    /// Name of the compensating state, if declared.
    pub compensation: Option<String>,
    /// A short effect label (e.g. `charge`, `reserve`, `read`) from inference.
    pub effect: Option<String>,
    /// Business typestate consumed / produced (typestate pass).
    pub state_in: Option<String>,
    pub state_out: Option<String>,
    /// Declared input / output schema names (typed contract pass).
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    /// Resolved field sets for the declared schemas (typed contract pass).
    pub input_fields: Option<Vec<String>>,
    pub output_fields: Option<Vec<String>>,
    /// True when idempotency/persistence were inferred rather than declared.
    pub inferred: bool,
}

#[derive(Debug, Clone)]
pub struct State {
    pub name: String,
    pub kind: StateKind,
    pub comment: Option<String>,
    pub resource: Option<String>,
    pub next: Option<String>,
    pub end: bool,
    pub input_path: Option<Value>,
    pub output_path: Option<Value>,
    pub result_path: ResultPath,
    pub parameters: Option<Value>,
    /// JSONata action arguments (`Arguments`).
    pub arguments: Option<Value>,
    /// Workflow-variable assignments (`Assign`). JSONPath states use payload-template
    /// `.$` bindings; JSONata states use `{% ... %}` expressions. The data-flow
    /// pass models JSONPath assignments conservatively and treats JSONata ones as opaque.
    pub assign: Option<Value>,
    /// JSONata state output (`Output`).
    pub output: Option<Value>,
    /// JSONata Map item source (`Items`).
    pub items: Option<Value>,
    pub result_selector: Option<Value>,
    pub result: Option<Value>,
    pub retry: Vec<RetryRule>,
    pub catch: Vec<CatchRule>,
    pub choices: Vec<ChoiceRule>,
    pub default: Option<String>,
    /// Sub-machine of a `Map` state (`Iterator` or `ItemProcessor`).
    pub iterator: Option<Box<Workflow>>,
    pub items_path: Option<String>,
    /// Sub-machines of a `Parallel` state.
    pub branches: Vec<Workflow>,
    /// Per-attempt task `TimeoutSeconds` (temporal analysis).
    pub timeout_seconds: Option<f64>,
    /// `HeartbeatSeconds` for activity / `waitForTaskToken` tasks.
    pub heartbeat_seconds: Option<f64>,
    /// `MaxConcurrency` of a `Map` state (`None` = unbounded).
    pub max_concurrency: Option<i64>,
    /// Whether this is a Distributed Map (`ItemProcessor` in distributed mode,
    /// or distributed-only fields such as `ItemReader`).
    pub distributed_map: bool,
    /// `ItemSelector`/`Parameters` of a `Map` describing the per-item document.
    pub item_selector: Option<Value>,
    /// Effective query language of this state (explicit `QueryLanguage`, else the
    /// machine default, resolved by the frontend). When `JsonAta`, the data-flow
    /// analysis treats the state opaquely for soundness.
    pub query_language: QueryLang,
    pub anno: Anno,
    /// State fields not interpreted by StepCheck, preserved for ASL re-emission.
    pub extra: serde_json::Map<String, Value>,
    /// Whether the original Map sub-machine was spelled as `ItemProcessor`
    /// rather than classic `Iterator`; preserves Distributed Map syntax.
    pub map_uses_item_processor: bool,
}

impl State {
    pub fn new(name: impl Into<String>, kind: StateKind) -> Self {
        State {
            name: name.into(),
            kind,
            comment: None,
            resource: None,
            next: None,
            end: false,
            input_path: None,
            output_path: None,
            result_path: ResultPath::Default,
            parameters: None,
            arguments: None,
            assign: None,
            output: None,
            items: None,
            result_selector: None,
            result: None,
            retry: Vec::new(),
            catch: Vec::new(),
            choices: Vec::new(),
            default: None,
            iterator: None,
            items_path: None,
            branches: Vec::new(),
            timeout_seconds: None,
            heartbeat_seconds: None,
            max_concurrency: None,
            distributed_map: false,
            item_selector: None,
            query_language: QueryLang::JsonPath,
            anno: Anno::default(),
            extra: serde_json::Map::new(),
            map_uses_item_processor: false,
        }
    }

    /// Whether the analysis must treat this state as opaque (JSONata mode), i.e.
    /// its document reshaping is not modelled, so its output is `Top` and its
    /// references are not checked. This is the soundness fallback for JSONata.
    pub fn is_opaque_query(&self) -> bool {
        matches!(self.query_language, QueryLang::JsonAta)
    }


    pub fn is_terminal(&self) -> bool {
        self.kind.is_terminal_kind() || self.end
    }

    /// Successor state names within this machine via *normal* control flow
    /// (`Next`, `Default`, `Choices[].Next`) — excludes error transitions.
    pub fn normal_successors(&self) -> Vec<&str> {
        let mut out = Vec::new();
        if let Some(n) = &self.next {
            out.push(n.as_str());
        }
        if let Some(d) = &self.default {
            out.push(d.as_str());
        }
        for c in &self.choices {
            out.push(c.next.as_str());
        }
        out
    }

    /// Error-handling successors (`Catch[].Next`).
    pub fn catch_successors(&self) -> Vec<&str> {
        self.catch.iter().map(|c| c.next.as_str()).collect()
    }

    /// All in-machine successors (normal + catch).
    pub fn all_successors(&self) -> Vec<&str> {
        let mut v = self.normal_successors();
        v.extend(self.catch_successors());
        v
    }

    pub fn is_task(&self) -> bool {
        matches!(self.kind, StateKind::Task)
    }

    /// Whether this task carries a retry policy (`Retry[]`).
    pub fn has_retry(&self) -> bool {
        !self.retry.is_empty()
    }
}

/// A workflow (or a `Map`/`Parallel` sub-machine).
#[derive(Debug, Clone)]
pub struct Workflow {
    pub name: String,
    pub comment: Option<String>,
    pub start_at: String,
    pub states: IndexMap<String, State>,
    /// Sibling/child state-machine definitions available for static composition.
    /// Keys are the static `StateMachineArn` strings produced by a frontend.
    pub linked_children: Rc<BTreeMap<String, Workflow>>,
    /// Optional declared schema of the whole-workflow input (typed contract).
    pub input_schema: Option<String>,
    /// Resolved field set of the declared workflow input schema. When present
    /// the data-flow analysis seeds the start document as a *closed* record of
    /// these fields (the typed tier); otherwise the start document is `Top`.
    pub input_fields: Option<Vec<String>>,
    /// Machine-level `TimeoutSeconds` (temporal analysis); `None` = unbounded.
    pub timeout_seconds: Option<f64>,
    /// Allowed business-state transitions `(from -> to)`; empty if no protocol
    /// was declared (typestate pass is then a no-op for ordering checks).
    pub protocol: Vec<(String, String)>,
    /// Machine-level fields not interpreted by StepCheck, preserved for ASL re-emission.
    pub extra: serde_json::Map<String, Value>,
}

impl Workflow {
    pub fn new(name: impl Into<String>, start_at: impl Into<String>) -> Self {
        Workflow {
            name: name.into(),
            comment: None,
            start_at: start_at.into(),
            states: IndexMap::new(),
            linked_children: Rc::new(BTreeMap::new()),
            input_schema: None,
            input_fields: None,
            timeout_seconds: None,
            protocol: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    /// Number of states counting nested `Map`/`Parallel` sub-machines.
    pub fn total_states(&self) -> usize {
        let mut n = self.states.len();
        for s in self.states.values() {
            if let Some(it) = &s.iterator {
                n += it.total_states();
            }
            for b in &s.branches {
                n += b.total_states();
            }
        }
        n
    }

    /// Iterate this machine and every nested sub-machine.
    pub fn walk_machines<'a>(&'a self, f: &mut dyn FnMut(&'a Workflow)) {
        f(self);
        for s in self.states.values() {
            if let Some(it) = &s.iterator {
                it.walk_machines(f);
            }
            for b in &s.branches {
                b.walk_machines(f);
            }
        }
    }
}

/// Collect every JSONPath reference syntactically reachable inside a JSON value:
/// keys suffixed with `.$` (intrinsic/path payloads) and `Variable` operands of
/// Choice rules. Used by the contract / data-flow analysis.
pub fn collect_jsonpath_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k.ends_with(".$") {
                    if let Value::String(s) = val {
                        out.push(s.clone());
                    }
                } else if k == "Variable" {
                    if let Value::String(s) = val {
                        out.push(s.clone());
                    }
                }
                collect_jsonpath_refs(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_jsonpath_refs(val, out);
            }
        }
        _ => {}
    }
}
