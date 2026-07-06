//! Flow-sensitive data-flow / field-provenance analysis (code `SC1101`).
//!
//! This is the analysis that no existing Amazon States Language tool performs.
//! `statelint`, `asl-validator` and AWS's own `ValidateStateMachineDefinition`
//! check JSONPath *syntax* ("does this look like a path?"); none reason about
//! *provenance* ("can the field this path names ever have been produced?").
//!
//! We model the ASL I/O-processing pipeline as an abstract interpretation over
//! a *may-present* lattice of document shapes:
//!
//! ```text
//!   Shape ::= Top                       -- opaque: any field may be present
//!           | Obj { k1: Shape, ... }    -- a record whose keys are EXACTLY k1..
//! ```
//!
//! `Top` is the lattice top; `Obj` records over-approximate the set of paths
//! that *may* be present. The join is the pointwise union of keys (`Top`
//! absorbs), so a path is reported missing only when it is absent on *every*
//! execution that reaches the state. The transfer functions follow the ASL
//! contract exactly — `InputPath` (re-root), `Parameters`/`ItemSelector`
//! (construct a closed record; each `k.$:"$.p"` *reads* `$.p`), the opaque task
//! result (`Top` unless a `ResultSelector` pins keys), `ResultPath` (merge into
//! the *raw* state input), and `OutputPath` (re-root). The result is **sound**:
//! every `SC1101` is a reference that is guaranteed to fail at run time, so the
//! analysis has no false positives by construction.
//!
//! Two tiers, matching the rest of the tool:
//!   * **native** — the start document is `Top` (the execution input is
//!     arbitrary JSON); findings arise wherever the workflow *constructs* the
//!     data it later reads (a `Pass` `Result`, a `ResultSelector`, a static
//!     `Parameters` object, an `OutputPath` that drops a field);
//!   * **typed** — when an input schema is declared (DSL / sidecar / CNCF JSON
//!     Schema) the start document is a *closed* record, which recovers the full
//!     "field never produced" class (the `amount` vs `total` defect) flow-
//!     sensitively, across `ResultPath`/`OutputPath` re-shaping.

use super::retry::qualify;
use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{ResultPath, State, StateKind, Workflow};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct DataFlowPass;

/// Fixpoint telemetry for one analyzed machine: how many rounds the forward
/// data-flow iteration took, the finite-height round bound it was allowed
/// (`|states| * (|keys| + 2) + 2`), whether it converged within that bound, and
/// the machine's state count. Used by the `fixpoint-stats` eval command to show
/// how far the iteration stays below its termination bound on a corpus.
#[derive(Clone, Copy)]
pub struct FixpointRound {
    pub rounds: usize,
    pub bound: usize,
    pub converged: bool,
    pub states: usize,
}

/// Opt-in recorder for [`FixpointRound`]s. Recording is off by default, so normal
/// verification pays only a single relaxed atomic load per machine; the
/// `fixpoint-stats` eval command switches it on to measure the round/bound
/// utilisation behind the termination argument.
static FP_ON: AtomicBool = AtomicBool::new(false);
static FP_LOG: Mutex<Vec<FixpointRound>> = Mutex::new(Vec::new());

/// Start recording fixpoint telemetry, discarding any previous capture.
pub fn fixpoint_record_start() {
    FP_LOG.lock().unwrap().clear();
    FP_ON.store(true, Ordering::Relaxed);
}

/// Stop recording and return everything captured since [`fixpoint_record_start`].
pub fn fixpoint_record_take() -> Vec<FixpointRound> {
    FP_ON.store(false, Ordering::Relaxed);
    std::mem::take(&mut FP_LOG.lock().unwrap())
}

impl Pass for DataFlowPass {
    fn id(&self) -> &'static str {
        "dataflow"
    }
    fn title(&self) -> &'static str {
        "Data-flow / field-provenance analysis"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        let root = root_seed(wf);
        analyze_machine(wf, root, "", sink);
    }
}

/// Abstract document shape on the *may-present* lattice.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    /// Opaque value: any field may be present (lattice top).
    Top,
    /// A record whose keys are *exactly* those listed (a closed object). A key
    /// absent here is absent on every path that produced this shape. Scalars and
    /// arrays are modelled as `Obj({})` (a leaf: no sub-field can be present) or
    /// `Top` respectively.
    Obj(HashMap<String, Shape>),
}

#[derive(Debug, PartialEq)]
enum Presence {
    Present,
    Missing,
    Maybe,
}

impl Shape {
    fn leaf() -> Shape {
        Shape::Obj(HashMap::new())
    }
}

/// Seed for the start document: a closed record of the declared input fields
/// (typed tier), else the start task's declared input schema, else `Top`.
fn root_seed(wf: &Workflow) -> Shape {
    if let Some(fields) = &wf.input_fields {
        return closed_record(fields);
    }
    if let Some(start) = wf.states.get(&wf.start_at) {
        if let Some(fields) = &start.anno.input_fields {
            return closed_record(fields);
        }
    }
    Shape::Top
}

fn closed_record(fields: &[String]) -> Shape {
    Shape::Obj(fields.iter().map(|f| (f.clone(), Shape::Top)).collect())
}

/// Lattice join (pointwise union; `Top` absorbs).
fn join(a: &Shape, b: &Shape) -> Shape {
    match (a, b) {
        (Shape::Top, _) | (_, Shape::Top) => Shape::Top,
        (Shape::Obj(ma), Shape::Obj(mb)) => {
            let mut out = ma.clone();
            for (k, vb) in mb {
                out.entry(k.clone())
                    .and_modify(|va| *va = join(va, vb))
                    .or_insert_with(|| vb.clone());
            }
            Shape::Obj(out)
        }
    }
}

/// Parse a *simple* dotted JSONPath (`$`, `$.a`, `$.a.b`) into its field
/// segments. Returns `None` for anything with brackets, wildcards, filters,
/// functions or the context object — which callers then treat conservatively
/// (a reference is `Maybe`, a narrow yields `Top`). This conservatism is what
/// keeps the analysis sound on the full diversity of real ASL.
fn parse_path(p: &str) -> Option<Vec<String>> {
    let rest = p.strip_prefix('$')?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = rest.strip_prefix('.')?;
    let mut segs = Vec::new();
    for seg in rest.split('.') {
        if seg.is_empty()
            || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        segs.push(seg.to_string());
    }
    Some(segs)
}

fn lookup(shape: &Shape, segs: &[String]) -> Presence {
    match shape {
        _ if segs.is_empty() => Presence::Present,
        Shape::Top => Presence::Maybe,
        Shape::Obj(m) => match m.get(&segs[0]) {
            Some(sub) => lookup(sub, &segs[1..]),
            None => Presence::Missing,
        },
    }
}

/// Re-root a shape along an `InputPath`/`OutputPath` value.
fn narrow(shape: &Shape, path: &Option<Value>) -> Shape {
    match path {
        None => shape.clone(),
        Some(Value::Null) => Shape::leaf(), // `InputPath:null` => empty input {}
        Some(Value::String(p)) => {
            if p == "$" {
                return shape.clone();
            }
            match parse_path(p) {
                Some(segs) => nav(shape, &segs),
                None => Shape::Top, // complex selection: unknown sub-document
            }
        }
        Some(_) => shape.clone(),
    }
}

fn nav(shape: &Shape, segs: &[String]) -> Shape {
    if segs.is_empty() {
        return shape.clone();
    }
    match shape {
        Shape::Top => Shape::Top,
        Shape::Obj(m) => match m.get(&segs[0]) {
            Some(sub) => nav(sub, &segs[1..]),
            None => Shape::Top, // narrowing to a missing field: stay conservative
        },
    }
}

/// Place `val` into `doc` at a `ResultPath`, preserving sibling keys.
fn place(doc: &Shape, rp: &ResultPath, val: Shape) -> Shape {
    match rp {
        ResultPath::Default => val,
        ResultPath::Discard => doc.clone(),
        ResultPath::Path(p) => match parse_path(p) {
            Some(segs) if !segs.is_empty() => place_in(doc, &segs, val),
            // explicit `ResultPath: "$"` replaces the whole document with the result
            Some(_) => val,
            // an unparseable path: cannot pin the touched key precisely, so fall
            // back to top (never claim a sibling is absent unsoundly).
            None => Shape::Top,
        },
    }
}

fn place_in(doc: &Shape, segs: &[String], val: Shape) -> Shape {
    if segs.is_empty() {
        return val;
    }
    match doc {
        Shape::Top => Shape::Top, // placing under an opaque region stays opaque
        Shape::Obj(m) => {
            let mut m = m.clone();
            let child = m.get(&segs[0]).cloned().unwrap_or_else(Shape::leaf);
            m.insert(segs[0].clone(), place_in(&child, &segs[1..], val));
            Shape::Obj(m)
        }
    }
}

/// Shape of a *literal* `Result` value (no `.$` semantics).
fn shape_of_literal(v: &Value) -> Shape {
    match v {
        Value::Object(m) => {
            Shape::Obj(m.iter().map(|(k, val)| (k.clone(), shape_of_literal(val))).collect())
        }
        Value::Array(_) => Shape::Top,
        _ => Shape::leaf(),
    }
}

/// Shape of a *constructor* (`Parameters`/`ItemSelector`/`ResultSelector`): a
/// `k.$` key binds field `k` (to an opaque value we do not track), a static
/// object key recurses, a static scalar is a leaf.
fn shape_of_constructor(v: &Value) -> Shape {
    match v {
        Value::Object(m) => {
            let mut out = HashMap::new();
            for (k, val) in m {
                if let Some(name) = k.strip_suffix(".$") {
                    out.insert(name.to_string(), Shape::Top);
                } else {
                    out.insert(k.clone(), shape_of_constructor(val));
                }
            }
            Shape::Obj(out)
        }
        Value::Array(_) => Shape::Top,
        _ => Shape::leaf(),
    }
}

/// The shape a state's result-processing writes back (before `ResultPath`).
fn result_shape(st: &State, eff_in: &Shape) -> Shape {
    match st.kind {
        StateKind::Task | StateKind::Map | StateKind::Parallel => {
            match &st.result_selector {
                Some(rs) => shape_of_constructor(rs), // pins the result's keys
                None => Shape::Top,                   // opaque task/array result
            }
        }
        StateKind::Pass => {
            if let Some(r) = &st.result {
                shape_of_literal(r)
            } else if let Some(p) = &st.parameters {
                shape_of_constructor(p)
            } else {
                eff_in.clone()
            }
        }
        _ => eff_in.clone(),
    }
}

fn has_result_processing(st: &State) -> bool {
    matches!(
        st.kind,
        StateKind::Task | StateKind::Pass | StateKind::Map | StateKind::Parallel
    )
}

/// Document shape leaving a state (consumed by its successors).
fn out_shape(st: &State, in_shape: &Shape) -> Shape {
    // JSONata states reshape their document with `Output`/`Arguments`/`{% … %}`
    // expressions we do not model. Producing `Top` over-approximates whatever
    // they emit, so downstream reads of a JSONata-produced field resolve to
    // `Maybe` (never `Missing`) and are never flagged — the soundness fallback.
    if st.is_opaque_query() {
        return Shape::Top;
    }
    let eff_in = narrow(in_shape, &st.input_path);
    let combined = if has_result_processing(st) {
        // a Task/Pass/Map/Parallel merges its result into the *raw* state input
        let res = result_shape(st, &eff_in);
        place(in_shape, &st.result_path, res)
    } else {
        // Choice/Wait/terminals pass the InputPath-filtered document through
        eff_in
    };
    narrow(&combined, &st.output_path)
}

/// Document shape a `Catch` target receives: the state input with an opaque
/// error object placed per the catcher's `ResultPath`.
fn catch_shape(in_shape: &Shape, rp: &ResultPath) -> Shape {
    place(in_shape, rp, Shape::Top)
}

/// Collect only the JSONPath references that the ASL runtime \emph{evaluates and
/// hard-fails on when absent}: the value of every key suffixed `.$` (at any depth)
/// in a `Parameters`/`ItemSelector` constructor. We deliberately do **not**
/// harvest `Variable` keys here: a literal field named `Variable` inside a
/// `Parameters` payload is verbatim data, and a `Variable` operand of a `Choice`
/// rule is a *test* (`IsPresent`/`IsNull`/value comparators) that returns false on
/// an absent field rather than failing the execution. Including either would break
/// the soundness (no-false-positive) guarantee of `SC1101`.
fn dollar_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k.ends_with(".$") {
                    if let Value::String(s) = val {
                        out.push(s.clone());
                    }
                }
                dollar_refs(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                dollar_refs(val, out);
            }
        }
        _ => {}
    }
}

/// The references a state evaluates against its *effective* input (after
/// `InputPath`) that must resolve for the execution not to fail: the `.$`
/// constructors of `Parameters`/`ItemSelector`, and a `Map`'s `ItemsPath`.
fn refs_of(st: &State) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = &st.parameters {
        dollar_refs(p, &mut out);
    }
    if let Some(is) = &st.item_selector {
        dollar_refs(is, &mut out);
    }
    if let Some(ip) = &st.items_path {
        out.push(ip.clone());
    }
    out
}

/// An upper bound on the number of distinct field keys any shape in this machine
/// can hold: every key a constructor/result/selector mentions, plus the declared
/// input fields. The may-present lattice's height is bounded by this (a shape
/// only ever gains keys or lifts to `Top`), which in turn bounds the number of
/// forward-fixpoint iterations — the termination argument behind the soundness
/// theorem.
fn key_universe(wf: &Workflow) -> usize {
    use std::collections::HashSet;
    fn harvest(v: &Value, keys: &mut HashSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, val) in m {
                    keys.insert(k.trim_end_matches(".$").to_string());
                    harvest(val, keys);
                }
            }
            Value::Array(a) => a.iter().for_each(|val| harvest(val, keys)),
            _ => {}
        }
    }
    let mut keys: HashSet<String> = HashSet::new();
    if let Some(f) = &wf.input_fields {
        keys.extend(f.iter().cloned());
    }
    for st in wf.states.values() {
        for v in [&st.parameters, &st.result, &st.result_selector, &st.item_selector] {
            if let Some(val) = v {
                harvest(val, &mut keys);
            }
        }
    }
    keys.len()
}

/// Compute the entry shape of every state by forward fixpoint, then check
/// references, then recurse into nested machines.
fn analyze_machine(wf: &Workflow, root: Shape, scope: &str, sink: &mut DiagnosticSink) {
    // Forward dataflow to a fixpoint over normal + catch edges.
    let mut in_shapes: HashMap<String, Shape> = HashMap::new();
    if wf.states.contains_key(&wf.start_at) {
        in_shapes.insert(wf.start_at.clone(), root.clone());
    }
    // Forward fixpoint. Each state's shape only ever grows (a join unions keys
    // or lifts to `Top`) and the key universe is finite, so the ascending chain
    // stabilises; we iterate to a genuine fixpoint. The number of rounds is
    // bounded by `states * (key_universe + 2)` — at most one strict increase per
    // state per height level — which we assert. Should the bound ever be exceeded
    // (a bug), we do *not* report from the unconverged (under-approximated) state
    // shapes, since those could be smaller than the fixpoint and yield a false
    // positive; this keeps the no-false-positive guarantee unconditional.
    let bound = wf.states.len().saturating_mul(key_universe(wf) + 2) + 2;
    let mut rounds = 0usize;
    let converged = loop {
        let mut changed = false;
        for (name, st) in &wf.states {
            let Some(cur_in) = in_shapes.get(name).cloned() else { continue };
            let out = out_shape(st, &cur_in);
            // normal successors inherit the out shape
            for succ in st.normal_successors() {
                if wf.states.contains_key(succ) {
                    propagate(&mut in_shapes, succ, &out, &mut changed);
                }
            }
            // catch successors inherit the input + opaque error object
            for c in &st.catch {
                if wf.states.contains_key(&c.next) {
                    let cs = catch_shape(&cur_in, &c.result_path);
                    propagate(&mut in_shapes, &c.next, &cs, &mut changed);
                }
            }
        }
        rounds += 1;
        if !changed {
            break true;
        }
        if rounds > bound {
            debug_assert!(false, "data-flow fixpoint exceeded its finite-height bound");
            break false;
        }
    };

    if FP_ON.load(Ordering::Relaxed) {
        FP_LOG.lock().unwrap().push(FixpointRound {
            rounds,
            bound,
            converged,
            states: wf.states.len(),
        });
    }

    // Check references against each state's effective input (only at the fixpoint).
    if converged {
    for (name, st) in &wf.states {
        // A JSONata state's references are `{% … %}` expressions, not JSONPath
        // `.$` reads; we do not interpret them, so we never flag them (soundness).
        if st.is_opaque_query() {
            continue;
        }
        let in_shape = in_shapes.get(name).cloned().unwrap_or(Shape::Top);
        let eff_in = narrow(&in_shape, &st.input_path);
        for r in refs_of(st) {
            if r.starts_with("$$") || r.starts_with("States.") || !r.starts_with('$') {
                continue; // context object / intrinsic / non-path: out of scope
            }
            let Some(segs) = parse_path(&r) else { continue };
            if segs.is_empty() {
                continue; // `$` is the whole document
            }
            if lookup(&eff_in, &segs) == Presence::Missing {
                sink.push(
                    Diagnostic::error(
                        "SC1101",
                        &qualify(scope, name),
                        format!(
                            "data-flow: '{name}' reads '{r}', a field no execution reaching it can have produced"
                        ),
                    )
                    .with_note(
                        "the reference resolves against a document the workflow constructs; no state on any path binds this field",
                    ),
                );
            }
        }
    }

    // SC1110 (path-sensitive dead guard): a Choice rule whose match REQUIRES a
    // field that provenance proves is definitely absent here can never be taken,
    // so the branch is dead. Sound: we flag only on a definitely-absent operand
    // under a presence-requiring comparator, never on `Top`/`Maybe`.
    for (name, st) in &wf.states {
        if st.kind != StateKind::Choice || st.is_opaque_query() {
            continue;
        }
        let in_shape = in_shapes.get(name).cloned().unwrap_or(Shape::Top);
        let eff_in = narrow(&in_shape, &st.input_path);
        for rule in &st.choices {
            if let Some(var) = dead_guard_field(&rule.condition, &eff_in) {
                sink.push(
                    Diagnostic::warning(
                        "SC1110",
                        &qualify(scope, name),
                        format!(
                            "Choice branch to '{}' guards on '{var}', a field no execution reaching '{name}' can have produced---the branch is unreachable",
                            rule.next
                        ),
                    )
                    .with_note(
                        "dead guard: provenance shows the tested field is never present here; the comparison is always false",
                    ),
                );
            }
        }
    }
    // Recurse into nested machines with the right seed. Gated on convergence:
    // when the enclosing machine did not converge its shapes are under-approximate,
    // so a nested analysis seeded from them could report a false positive; we then
    // suppress the recursion, exactly as we suppress this machine's own reporting.
    for (name, st) in &wf.states {
        let in_shape = in_shapes.get(name).cloned().unwrap_or(Shape::Top);
        let eff_in = narrow(&in_shape, &st.input_path);
        if let Some(it) = &st.iterator {
            // Each iteration sees the per-item document: the ItemSelector record
            // if declared, else an opaque element.
            let item_root = match &st.item_selector {
                Some(is) => shape_of_constructor(is),
                None => Shape::Top,
            };
            analyze_machine(it, item_root, &qualify(scope, &format!("{name}[Map]")), sink);
        }
        for (i, br) in st.branches.iter().enumerate() {
            // Each Parallel branch receives the state's effective input, reshaped
            // by `Parameters` when present (as ASL constructs the branch payload).
            // Ignoring Parameters would under-approximate and could raise a
            // spurious SC1101 inside the branch.
            let branch_root = match &st.parameters {
                Some(p) => shape_of_constructor(p),
                None => eff_in.clone(),
            };
            analyze_machine(br, branch_root, &qualify(scope, &format!("{name}[Branch{i}]")), sink);
        }
    }
    } // end `if converged`
}

/// Whether satisfying this leaf comparator requires the `Variable` field to be
/// *present*. The only operator a genuinely absent field satisfies is
/// `IsPresent: false`; every other comparator (value tests, `IsNull`, the type
/// predicates, `IsPresent: true`) returns false on an absent field.
fn requires_presence(obj: &serde_json::Map<String, Value>) -> bool {
    for (k, v) in obj {
        if k == "Variable" || k == "Next" || k == "Comment" {
            continue;
        }
        if k == "IsPresent" {
            return v.as_bool() != Some(false);
        }
        return true; // any other comparator needs the field present
    }
    false
}

/// If a Choice condition can never be true *because* one of its operands is a
/// definitely-absent field, return that field path. Handles `And` (dead if any
/// conjunct is dead) and `Or` (dead only if every disjunct is dead); it is
/// conservative on `Not` (never flags), keeping the finding sound.
fn dead_guard_field(cond: &Value, shape: &Shape) -> Option<String> {
    let obj = cond.as_object()?;
    if let Some(Value::Array(subs)) = obj.get("And") {
        return subs.iter().find_map(|s| dead_guard_field(s, shape));
    }
    if let Some(Value::Array(subs)) = obj.get("Or") {
        if subs.is_empty() {
            return None;
        }
        let mut witness = None;
        for s in subs {
            match dead_guard_field(s, shape) {
                Some(p) => witness = Some(p),
                None => return None,
            }
        }
        return witness;
    }
    if obj.contains_key("Not") {
        return None;
    }
    let var = obj.get("Variable")?.as_str()?;
    if !requires_presence(obj) || !var.starts_with('$') || var.starts_with("$$") {
        return None;
    }
    let segs = parse_path(var)?;
    if segs.is_empty() {
        return None;
    }
    match lookup(shape, &segs) {
        Presence::Missing => Some(var.to_string()),
        _ => None,
    }
}

fn propagate(map: &mut HashMap<String, Shape>, target: &str, shape: &Shape, changed: &mut bool) {
    match map.get(target) {
        Some(existing) => {
            let j = join(existing, shape);
            if &j != existing {
                map.insert(target.to_string(), j);
                *changed = true;
            }
        }
        None => {
            map.insert(target.to_string(), shape.clone());
            *changed = true;
        }
    }
}
