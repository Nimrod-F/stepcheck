//! The format-agnostic workflow intermediate representation (IR).
//!
//! Every frontend (ASL JSON, the typed DSL, …) lowers to a [`Workflow`].
//! Every analysis [`crate::passes::Pass`] reads this IR and nothing else, so
//! checks are independent of the surface syntax a workflow was authored in.

use indexmap::IndexMap;
use serde_json::Value;

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
}

#[derive(Debug, Clone)]
pub struct CatchRule {
    pub error_equals: Vec<String>,
    pub next: String,
    pub result_path: ResultPath,
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
    pub anno: Anno,
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
            result_selector: None,
            result: None,
            retry: Vec::new(),
            catch: Vec::new(),
            choices: Vec::new(),
            default: None,
            iterator: None,
            items_path: None,
            branches: Vec::new(),
            anno: Anno::default(),
        }
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
    /// Optional declared schema of the whole-workflow input (typed contract).
    pub input_schema: Option<String>,
    /// Allowed business-state transitions `(from -> to)`; empty if no protocol
    /// was declared (typestate pass is then a no-op for ordering checks).
    pub protocol: Vec<(String, String)>,
}

impl Workflow {
    pub fn new(name: impl Into<String>, start_at: impl Into<String>) -> Self {
        Workflow {
            name: name.into(),
            comment: None,
            start_at: start_at.into(),
            states: IndexMap::new(),
            input_schema: None,
            protocol: Vec::new(),
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
