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
//! (construct a closed record; each `k.$:"$.p"` *reads* `$.p`), task results
//! (`Top` unless a `ResultSelector`, declared closed output contract, or known
//! AWS service envelope pins keys), `ResultPath` (merge into the *raw* state
//! input), and `OutputPath` (re-root). The result is **sound**:
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

pub struct DataFlowPass {
    result_shapes: bool,
}

#[derive(Clone, Copy)]
struct Options {
    result_shapes: bool,
}

impl DataFlowPass {
    pub fn new() -> Self {
        Self { result_shapes: false }
    }

    pub fn with_result_shapes() -> Self {
        Self { result_shapes: true }
    }

    fn options(&self) -> Options {
        Options { result_shapes: self.result_shapes }
    }
}

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

/// How the field-reads a workflow evaluates (the `.$` operands of
/// `Parameters`/`ItemSelector`/`Assign` operands and a `Map`'s `ItemsPath`) split
/// between the *modeled* JSONPath fragment and the conservative fallbacks. The
/// data-flow check resolves precisely exactly the `dotted` reads (and trivially
/// the whole-document `$`); every other class is treated conservatively (`Maybe`)
/// and can never produce an `SC1101`. Used by the `path-coverage` command to
/// quantify how much of real ASL falls inside the modeled fragment.
#[derive(Clone, Copy, Default)]
pub struct PathCoverage {
    /// Total field-reads examined.
    pub total: usize,
    /// `$.a`, `$.a.b`: dotted access, resolved precisely against the shape.
    pub dotted: usize,
    /// `$` exactly: the whole document (always present, trivially precise).
    pub whole_doc: usize,
    /// `$`-rooted but with brackets, wildcards, filters or functions: conservative.
    pub complex: usize,
    /// `$$…` context object: out of the document model, conservative.
    pub context: usize,
    /// `$var` / `$var.path` workflow-variable reads: out of the document model.
    pub variable: usize,
    /// `States.*` intrinsics or non-`$` literals: not a document path.
    pub intrinsic: usize,
}

impl PathCoverage {
    /// Reads the analysis resolves precisely (dotted access plus whole-document `$`).
    pub fn precise(&self) -> usize {
        self.dotted + self.whole_doc
    }
}

/// Classify every field-read in `wf` (recursing into `Map` iterators and
/// `Parallel` branches) into [`PathCoverage`] buckets, using the *same*
/// [`refs_of`]/[`parse_path`] logic the `SC1101` check applies.
pub fn path_coverage(wf: &Workflow, acc: &mut PathCoverage) {
    for st in wf.states.values() {
        for r in refs_of(st).into_iter().chain(assign_refs_of(st)) {
            acc.total += 1;
            if r.starts_with("$$") {
                acc.context += 1;
            } else if is_variable_ref(&r) {
                acc.variable += 1;
            } else if r.starts_with("States.") || !r.starts_with('$') {
                acc.intrinsic += 1;
            } else {
                match parse_path(&r) {
                    Some(segs) if segs.is_empty() => acc.whole_doc += 1,
                    Some(_) => acc.dotted += 1,
                    None => acc.complex += 1,
                }
            }
        }
        if let Some(it) = &st.iterator {
            path_coverage(it, acc);
        }
        for br in &st.branches {
            path_coverage(br, acc);
        }
    }
}

impl Pass for DataFlowPass {
    fn id(&self) -> &'static str {
        "dataflow"
    }
    fn title(&self) -> &'static str {
        "Data-flow / field-provenance analysis"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        let root = root_env(wf);
        analyze_machine(wf, root, "", self.options(), sink);
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

#[derive(Debug, Clone, PartialEq)]
struct Env {
    doc: Shape,
    vars: Shape,
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

fn root_env(wf: &Workflow) -> Env {
    Env { doc: root_seed(wf), vars: Shape::leaf() }
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

fn join_env(a: &Env, b: &Env) -> Env {
    Env { doc: join(&a.doc, &b.doc), vars: join(&a.vars, &b.vars) }
}

/// Parse a precise JSONPath member chain (`$`, `$.a`, `$.a['b']`, `$["a"]`) into
/// its field segments. Returns `None` for wildcards, filters, array indexes,
/// functions or the context object — which callers then treat conservatively (a
/// reference is `Maybe`, a narrow yields `Top`). This conservatism is what keeps
/// the analysis sound on the full diversity of real ASL.
fn parse_path(p: &str) -> Option<Vec<String>> {
    let rest = p.strip_prefix('$')?;
    parse_member_tail(rest)
}

fn valid_member_name(seg: &str) -> bool {
    !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn parse_member_tail(mut rest: &str) -> Option<Vec<String>> {
    let mut segs = Vec::new();
    while !rest.is_empty() {
        if let Some(after_dot) = rest.strip_prefix('.') {
            let end = after_dot.find(|c| c == '.' || c == '[').unwrap_or(after_dot.len());
            let seg = &after_dot[..end];
            if !valid_member_name(seg) {
                return None;
            }
            segs.push(seg.to_string());
            rest = &after_dot[end..];
        } else if let Some(after_bracket) = rest.strip_prefix('[') {
            let quote = after_bracket.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            let after_quote = &after_bracket[quote.len_utf8()..];
            let close = after_quote.find(quote)?;
            let seg = &after_quote[..close];
            let after_member = &after_quote[close + quote.len_utf8()..];
            rest = after_member.strip_prefix(']')?;
            if !valid_member_name(seg) {
                return None;
            }
            segs.push(seg.to_string());
        } else {
            return None;
        }
    }
    Some(segs)
}

/// JSONPath-mode workflow variables also begin with `$` (`$x`, `$order.id`) but
/// are not document paths (`$.x`). We classify them explicitly so they are a
/// conservative, documented non-SC1101 case rather than an accidental parse miss.
fn is_variable_ref(r: &str) -> bool {
    if !r.starts_with('$') || r == "$" || r.starts_with("$.") || r.starts_with("$$") {
        return false;
    }
    let tail = &r[1..];
    let end = tail.find(|c| c == '.' || c == '[').unwrap_or(tail.len());
    !tail[..end].is_empty()
}

fn parse_variable_ref(r: &str) -> Option<Vec<String>> {
    if !is_variable_ref(r) {
        return None;
    }
    let tail = &r[1..];
    let end = tail.find(|c| c == '.' || c == '[').unwrap_or(tail.len());
    let name = &tail[..end];
    if !valid_member_name(name) {
        return None;
    }
    let mut segs = vec![name.to_string()];
    segs.extend(parse_member_tail(&tail[end..])?);
    Some(segs)
}

fn ref_shape(r: &str, doc: &Shape, vars: &Shape) -> Shape {
    if let Some(segs) = parse_path(r) {
        return nav(doc, &segs);
    }
    if let Some(segs) = parse_variable_ref(r) {
        return nav(vars, &segs);
    }
    Shape::Top
}

enum JsonataRef {
    Input(Vec<String>),
    Result(Vec<String>),
    ErrorOutput(Vec<String>),
    Variable(Vec<String>),
    Context,
}

struct JsonataSources<'a> {
    input: &'a Shape,
    result: &'a Shape,
    error: &'a Shape,
    vars: &'a Shape,
}

fn jsonata_expr_body(s: &str) -> Option<&str> {
    let t = s.trim();
    let body = t.strip_prefix("{%")?.strip_suffix("%}")?;
    Some(body.trim())
}

fn parse_dot_tail(tail: &str) -> Option<Vec<String>> {
    parse_member_tail(tail)
}

fn parse_jsonata_ref(expr: &str) -> Option<JsonataRef> {
    let e = expr.trim();
    if let Some(tail) = e.strip_prefix("$states.input") {
        return parse_dot_tail(tail).map(JsonataRef::Input);
    }
    if let Some(tail) = e.strip_prefix("$states.result") {
        return parse_dot_tail(tail).map(JsonataRef::Result);
    }
    if let Some(tail) = e.strip_prefix("$states.errorOutput") {
        return parse_dot_tail(tail).map(JsonataRef::ErrorOutput);
    }
    if e.starts_with("$states.context") {
        return Some(JsonataRef::Context);
    }
    parse_variable_ref(e).map(JsonataRef::Variable)
}

fn jsonata_ref_shape(r: &JsonataRef, src: &JsonataSources<'_>) -> Shape {
    match r {
        JsonataRef::Input(segs) => nav(src.input, segs),
        JsonataRef::Result(segs) => nav(src.result, segs),
        JsonataRef::ErrorOutput(segs) => nav(src.error, segs),
        JsonataRef::Variable(segs) => nav(src.vars, segs),
        JsonataRef::Context => Shape::Top,
    }
}

fn split_top_level(s: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

fn find_top_level_colon(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_jsonata_key(s: &str) -> Option<String> {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return Some(inner.to_string());
    }
    if let Some(inner) = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(inner.to_string());
    }
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Some(t.to_string());
    }
    None
}

fn parse_jsonata_object(expr: &str) -> Option<Vec<(String, &str)>> {
    let t = expr.trim();
    let inner = t.strip_prefix('{')?.strip_suffix('}')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in split_top_level(inner, ',') {
        let idx = find_top_level_colon(part)?;
        let key = parse_jsonata_key(&part[..idx])?;
        out.push((key, part[idx + 1..].trim()));
    }
    Some(out)
}

fn jsonata_expr_shape(expr: &str, src: &JsonataSources<'_>) -> Shape {
    if let Some(r) = parse_jsonata_ref(expr) {
        return jsonata_ref_shape(&r, src);
    }
    if let Some(fields) = parse_jsonata_object(expr) {
        return Shape::Obj(
            fields
                .into_iter()
                .map(|(k, v)| (k, jsonata_expr_shape(v, src)))
                .collect(),
        );
    }
    Shape::Top
}

fn shape_of_jsonata_value(v: &Value, src: &JsonataSources<'_>) -> Shape {
    match v {
        Value::String(s) => match jsonata_expr_body(s) {
            Some(expr) => jsonata_expr_shape(expr, src),
            None => Shape::leaf(),
        },
        Value::Object(m) => Shape::Obj(
            m.iter()
                .map(|(k, val)| (k.clone(), shape_of_jsonata_value(val, src)))
                .collect(),
        ),
        Value::Array(_) => Shape::Top,
        _ => Shape::leaf(),
    }
}

/// Collect only the JSONata reads the ASL runtime *hard-fails* on when the path
/// is absent: a field-value expression whose *entire* body is a bare direct
/// reference (`$states.input.p`, `$states.result.p`, `$var.p`, …). Such an
/// expression evaluates to `undefined` on a missing path, and ASL turns a
/// top-level `undefined` field value into `States.QueryEvaluationError`
/// ("failure to return a result"), so absence is a definite runtime fault.
///
/// We deliberately do **not** descend into a JSONata *object constructor*
/// `{% { 'k': $ref } %}`: JSONata 2.0.6 (the dialect ASL implements) *omits* a
/// key whose value is `undefined`, so the constructor yields `{}` rather than an
/// error — flagging `$ref` there would be a false positive. The constructor's
/// *shape* is still tracked by [`jsonata_expr_shape`] for downstream provenance;
/// this function governs only the hard-failing read sites.
fn jsonata_strict_refs_in_expr<'a>(expr: &'a str, out: &mut Vec<&'a str>) {
    if parse_jsonata_ref(expr).is_some() {
        out.push(expr.trim());
    }
}

fn jsonata_strict_refs_in_value<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => {
            if let Some(expr) = jsonata_expr_body(s) {
                jsonata_strict_refs_in_expr(expr, out);
            }
        }
        Value::Object(m) => {
            for val in m.values() {
                jsonata_strict_refs_in_value(val, out);
            }
        }
        Value::Array(a) => {
            for val in a {
                jsonata_strict_refs_in_value(val, out);
            }
        }
        _ => {}
    }
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

fn shape_of_jsonpath_assign_value(v: &Value, source_doc: &Shape, vars: &Shape) -> Shape {
    match v {
        Value::Object(m) => {
            let mut out = HashMap::new();
            for (k, val) in m {
                if let Some(name) = k.strip_suffix(".$") {
                    let shape = val
                        .as_str()
                        .map(|r| ref_shape(r, source_doc, vars))
                        .unwrap_or(Shape::Top);
                    out.insert(name.to_string(), shape);
                } else {
                    out.insert(k.clone(), shape_of_jsonpath_assign_value(val, source_doc, vars));
                }
            }
            Shape::Obj(out)
        }
        Value::Array(_) => Shape::Top,
        _ => Shape::leaf(),
    }
}

fn bind_var(vars: &Shape, name: &str, val: Shape) -> Shape {
    match vars {
        Shape::Top => Shape::Top,
        Shape::Obj(m) => {
            let mut out = m.clone();
            out.insert(name.to_string(), val);
            Shape::Obj(out)
        }
    }
}

fn apply_jsonpath_assign(vars: &Shape, assign: &Option<Value>, source_doc: &Shape) -> Shape {
    let Some(Value::Object(m)) = assign else { return vars.clone() };
    let mut out = vars.clone();
    for (k, val) in m {
        let (name, shape) = if let Some(name) = k.strip_suffix(".$") {
            let shape = val
                .as_str()
                .map(|r| ref_shape(r, source_doc, vars))
                .unwrap_or(Shape::Top);
            (name, shape)
        } else {
            (k.as_str(), shape_of_jsonpath_assign_value(val, source_doc, vars))
        };
        out = bind_var(&out, name, shape);
    }
    out
}

fn apply_jsonata_assign(vars: &Shape, assign: &Option<Value>, src: &JsonataSources<'_>) -> Shape {
    let Some(Value::Object(m)) = assign else { return vars.clone() };
    let mut out = vars.clone();
    for (name, val) in m {
        out = bind_var(&out, name, shape_of_jsonata_value(val, src));
    }
    out
}

fn jsonata_result_source_shape(st: &State, opts: Options) -> Shape {
    match st.kind {
        StateKind::Task => task_result_shape(st, opts).unwrap_or(Shape::Top),
        StateKind::Map | StateKind::Parallel => Shape::Top,
        _ => Shape::Top,
    }
}

fn jsonata_output_shape(st: &State, env: &Env, opts: Options) -> Shape {
    let result = jsonata_result_source_shape(st, opts);
    let error = Shape::Top;
    let src = JsonataSources { input: &env.doc, result: &result, error: &error, vars: &env.vars };
    match &st.output {
        Some(output) => shape_of_jsonata_value(output, &src),
        None => match st.kind {
            StateKind::Task => result,
            StateKind::Map | StateKind::Parallel => Shape::Top,
            _ => env.doc.clone(),
        },
    }
}

fn closed_record_names(fields: &[&str]) -> Shape {
    Shape::Obj(fields.iter().map(|f| ((*f).to_string(), Shape::Top)).collect())
}

/// Closed-world task output contracts are interpreted as the JSON object the
/// Task contributes to ResultPath. Unknown or undeclared task results stay `Top`.
fn declared_task_output_shape(st: &State) -> Option<Shape> {
    st.anno.output_fields.as_ref().map(|fields| closed_record(fields))
}

fn integration_op<'a>(resource: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = resource.strip_prefix(prefix)?;
    Some(rest.split('.').next().unwrap_or(rest))
}

fn dynamodb_result_shape(op: &str) -> Option<Shape> {
    match op.to_ascii_lowercase().as_str() {
        "getitem" => Some(closed_record_names(&["Item", "ConsumedCapacity"])),
        "putitem" | "updateitem" | "deleteitem" => Some(closed_record_names(&[
            "Attributes",
            "ConsumedCapacity",
            "ItemCollectionMetrics",
        ])),
        "scan" | "query" => Some(closed_record_names(&[
            "Items",
            "Count",
            "ScannedCount",
            "LastEvaluatedKey",
            "ConsumedCapacity",
        ])),
        "batchgetitem" => Some(closed_record_names(&[
            "Responses",
            "UnprocessedKeys",
            "ConsumedCapacity",
        ])),
        "batchwriteitem" => Some(closed_record_names(&[
            "UnprocessedItems",
            "ItemCollectionMetrics",
            "ConsumedCapacity",
        ])),
        _ => None,
    }
}

fn known_service_result_shape(st: &State) -> Option<Shape> {
    let resource = st.resource.as_deref()?;
    if resource.starts_with("arn:aws:states:::lambda:invoke") {
        return Some(closed_record_names(&[
            "Payload",
            "StatusCode",
            "ExecutedVersion",
            "SdkHttpMetadata",
            "SdkResponseMetadata",
        ]));
    }
    if let Some(op) = integration_op(resource, "arn:aws:states:::dynamodb:") {
        return dynamodb_result_shape(op);
    }
    if let Some(rest) = resource.strip_prefix("arn:aws:states:::aws-sdk:") {
        let mut parts = rest.split(':');
        let service = parts.next()?;
        let op = parts.next()?.split('.').next().unwrap_or("");
        if service == "dynamodb" {
            return dynamodb_result_shape(op);
        }
    }
    if resource.starts_with("arn:aws:states:::events:putEvents") {
        return Some(closed_record_names(&["Entries", "FailedEntryCount"]));
    }
    if resource.starts_with("arn:aws:states:::sns:publish") {
        return Some(closed_record_names(&["MessageId", "SequenceNumber"]));
    }
    if resource.starts_with("arn:aws:states:::sqs:sendMessage") {
        return Some(closed_record_names(&[
            "MD5OfMessageBody",
            "MD5OfMessageAttributes",
            "MD5OfMessageSystemAttributes",
            "MessageId",
            "SequenceNumber",
        ]));
    }
    if resource.starts_with("arn:aws:states:::states:startExecution.sync:2") {
        return Some(closed_record_names(&[
            "ExecutionArn",
            "StateMachineArn",
            "Name",
            "StartDate",
            "StopDate",
            "Status",
            "Output",
            "OutputDetails",
            "Error",
            "Cause",
        ]));
    }
    if resource.starts_with("arn:aws:states:::states:startExecution") {
        return Some(closed_record_names(&["ExecutionArn", "StartDate"]));
    }
    None
}

fn task_result_shape(st: &State, opts: Options) -> Option<Shape> {
    if !opts.result_shapes {
        return None;
    }
    declared_task_output_shape(st).or_else(|| known_service_result_shape(st))
}

/// The shape a state's result-processing writes back (before `ResultPath`).
fn result_shape(st: &State, eff_in: &Shape, opts: Options) -> Shape {
    match st.kind {
        StateKind::Task | StateKind::Map | StateKind::Parallel => {
            match &st.result_selector {
                Some(rs) => shape_of_constructor(rs), // pins the result's keys
                None if st.kind == StateKind::Task => task_result_shape(st, opts).unwrap_or(Shape::Top),
                None => Shape::Top, // opaque map/parallel result
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

/// Abstract environment leaving a state (consumed by its successors).
fn out_env(st: &State, env: &Env, opts: Options) -> Env {
    if st.is_opaque_query() {
        let result = jsonata_result_source_shape(st, opts);
        let error = Shape::Top;
        let src = JsonataSources { input: &env.doc, result: &result, error: &error, vars: &env.vars };
        return Env {
            doc: jsonata_output_shape(st, env, opts),
            vars: apply_jsonata_assign(&env.vars, &st.assign, &src),
        };
    }
    let eff_in = narrow(&env.doc, &st.input_path);
    let doc = if has_result_processing(st) {
        // a Task/Pass/Map/Parallel merges its result into the *raw* state input
        let res = result_shape(st, &eff_in, opts);
        place(&env.doc, &st.result_path, res)
    } else {
        // Choice/Wait/terminals pass the InputPath-filtered document through
        eff_in
    };
    let assign_source = assign_source_shape(st, &narrow(&env.doc, &st.input_path), opts);
    Env {
        doc: narrow(&doc, &st.output_path),
        vars: apply_jsonpath_assign(&env.vars, &st.assign, &assign_source),
    }
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

/// JSONPath `Assign` is also a payload-template-like hard-failing read site.
/// Its `$` source is state-type dependent, so callers check these separately
/// from [`refs_of`], which resolves against the effective state input.
fn assign_refs_of(st: &State) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(a) = &st.assign {
        dollar_refs(a, &mut out);
    }
    out
}

fn assign_source_shape(st: &State, eff_in: &Shape, opts: Options) -> Shape {
    match st.kind {
        // JSONPath Assign on these states sees the raw API/sub-workflow result,
        // before ResultSelector/ResultPath. Without a result schema that value is
        // opaque, so reads are never definitely absent.
        StateKind::Task => task_result_shape(st, opts).unwrap_or(Shape::Top),
        StateKind::Map | StateKind::Parallel => Shape::Top,
        // Pass Assign sees the Pass result (Result, Parameters, or effective input).
        StateKind::Pass => result_shape(st, eff_in, opts),
        // Choice and Wait Assign read from the effective input.
        StateKind::Choice | StateKind::Wait => eff_in.clone(),
        _ => eff_in.clone(),
    }
}

fn check_required_ref(
    scope: &str,
    name: &str,
    r: &str,
    doc_shape: &Shape,
    vars_shape: &Shape,
    sink: &mut DiagnosticSink,
) {
    if r.starts_with("$$") || r.starts_with("States.") || !r.starts_with('$') {
        return; // context object / intrinsic / non-path: out of modeled scope
    }
    let (segs, shape) = if let Some(segs) = parse_path(r) {
        (segs, doc_shape)
    } else if let Some(segs) = parse_variable_ref(r) {
        (segs, vars_shape)
    } else {
        return;
    };
    if segs.is_empty() {
        return; // `$` is the whole document
    }
    if lookup(shape, &segs) == Presence::Missing {
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

fn check_jsonata_required_ref(
    scope: &str,
    name: &str,
    expr: &str,
    src: &JsonataSources<'_>,
    sink: &mut DiagnosticSink,
) {
    let Some(r) = parse_jsonata_ref(expr) else { return };
    let (segs, shape) = match &r {
        JsonataRef::Input(segs) => (segs, src.input),
        JsonataRef::Result(segs) => (segs, src.result),
        JsonataRef::ErrorOutput(segs) => (segs, src.error),
        JsonataRef::Variable(segs) => (segs, src.vars),
        JsonataRef::Context => return,
    };
    if segs.is_empty() {
        return;
    }
    if lookup(shape, segs) == Presence::Missing {
        sink.push(
            Diagnostic::error(
                "SC1101",
                &qualify(scope, name),
                format!(
                    "data-flow: '{name}' reads JSONata expression '{expr}', a field no execution reaching it can have produced"
                ),
            )
            .with_note(
                "the modeled JSONata reference resolves against a document or variable the workflow constructs; no state on any path binds this field",
            ),
        );
    }
}

fn check_jsonata_value_refs(
    scope: &str,
    name: &str,
    v: &Value,
    src: &JsonataSources<'_>,
    sink: &mut DiagnosticSink,
) {
    let mut refs = Vec::new();
    jsonata_strict_refs_in_value(v, &mut refs);
    for r in refs {
        check_jsonata_required_ref(scope, name, r, src, sink);
    }
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
    fn harvest_jsonata_expr(expr: &str, keys: &mut HashSet<String>) {
        if let Some(fields) = parse_jsonata_object(expr) {
            for (k, v) in fields {
                keys.insert(k);
                harvest_jsonata_expr(v, keys);
            }
        }
    }
    fn harvest_jsonata(v: &Value, keys: &mut HashSet<String>) {
        match v {
            Value::String(s) => {
                if let Some(expr) = jsonata_expr_body(s) {
                    harvest_jsonata_expr(expr, keys);
                }
            }
            Value::Object(m) => {
                for (k, val) in m {
                    keys.insert(k.trim_end_matches(".$").to_string());
                    harvest_jsonata(val, keys);
                }
            }
            Value::Array(a) => a.iter().for_each(|val| harvest_jsonata(val, keys)),
            _ => {}
        }
    }
    let mut keys: HashSet<String> = HashSet::new();
    if let Some(f) = &wf.input_fields {
        keys.extend(f.iter().cloned());
    }
    for st in wf.states.values() {
        for v in [
            &st.parameters,
            &st.arguments,
            &st.assign,
            &st.output,
            &st.items,
            &st.result,
            &st.result_selector,
            &st.item_selector,
        ] {
            if let Some(val) = v {
                harvest(val, &mut keys);
                harvest_jsonata(val, &mut keys);
            }
        }
        for c in &st.catch {
            if let Some(assign) = &c.assign {
                harvest(assign, &mut keys);
                harvest_jsonata(assign, &mut keys);
            }
        }
    }
    keys.len()
}

/// Compute the entry shape of every state by forward fixpoint, then check
/// references, then recurse into nested machines.
fn analyze_machine(wf: &Workflow, root: Env, scope: &str, opts: Options, sink: &mut DiagnosticSink) {
    // Forward dataflow to a fixpoint over normal + catch edges.
    let mut in_envs: HashMap<String, Env> = HashMap::new();
    if wf.states.contains_key(&wf.start_at) {
        in_envs.insert(wf.start_at.clone(), root.clone());
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
            let Some(cur_in) = in_envs.get(name).cloned() else { continue };
            let out = out_env(st, &cur_in, opts);
            // normal successors inherit the out shape
            for succ in st.normal_successors() {
                if wf.states.contains_key(succ) {
                    propagate(&mut in_envs, succ, &out, &mut changed);
                }
            }
            // catch successors inherit the input + opaque error object
            for c in &st.catch {
                if wf.states.contains_key(&c.next) {
                    let mut cs = Env { doc: catch_shape(&cur_in.doc, &c.result_path), vars: cur_in.vars.clone() };
                    if c.assign.is_some() {
                        let error = Shape::Top;
                        if st.is_opaque_query() {
                            let result = jsonata_result_source_shape(st, opts);
                            let src = JsonataSources { input: &cur_in.doc, result: &result, error: &error, vars: &cur_in.vars };
                            cs.vars = apply_jsonata_assign(&cur_in.vars, &c.assign, &src);
                        } else {
                            cs.vars = apply_jsonpath_assign(&cur_in.vars, &c.assign, &error);
                        }
                    }
                    propagate(&mut in_envs, &c.next, &cs, &mut changed);
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
        let in_env = in_envs.get(name).cloned().unwrap_or_else(|| Env { doc: Shape::Top, vars: Shape::Top });
        if st.is_opaque_query() {
            let result = jsonata_result_source_shape(st, opts);
            let error = Shape::Top;
            let src = JsonataSources { input: &in_env.doc, result: &result, error: &error, vars: &in_env.vars };
            for v in [&st.arguments, &st.output, &st.assign, &st.items] {
                if let Some(val) = v {
                    check_jsonata_value_refs(scope, name, val, &src, sink);
                }
            }
            for rule in &st.choices {
                check_jsonata_value_refs(scope, name, &rule.condition, &src, sink);
            }
            for c in &st.catch {
                if let Some(assign) = &c.assign {
                    check_jsonata_value_refs(scope, name, assign, &src, sink);
                }
            }
            continue;
        }
        let eff_in = narrow(&in_env.doc, &st.input_path);
        for r in refs_of(st) {
            check_required_ref(scope, name, &r, &eff_in, &in_env.vars, sink);
        }
        let assign_source = assign_source_shape(st, &eff_in, opts);
        for r in assign_refs_of(st) {
            check_required_ref(scope, name, &r, &assign_source, &in_env.vars, sink);
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
        let in_env = in_envs.get(name).cloned().unwrap_or_else(|| Env { doc: Shape::Top, vars: Shape::Top });
        let eff_in = narrow(&in_env.doc, &st.input_path);
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
        let in_env = in_envs.get(name).cloned().unwrap_or_else(|| Env { doc: Shape::Top, vars: Shape::Top });
        let eff_in = narrow(&in_env.doc, &st.input_path);
        if let Some(it) = &st.iterator {
            // Each iteration sees the per-item document: the ItemSelector record
            // if declared, else an opaque element.
            let item_root = match &st.item_selector {
                Some(is) => shape_of_constructor(is),
                None => Shape::Top,
            };
            let item_vars = if st.distributed_map { Shape::Top } else { in_env.vars.clone() };
            analyze_machine(it, Env { doc: item_root, vars: item_vars }, &qualify(scope, &format!("{name}[Map]")), opts, sink);
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
            analyze_machine(br, Env { doc: branch_root, vars: in_env.vars.clone() }, &qualify(scope, &format!("{name}[Branch{i}]")), opts, sink);
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

fn propagate(map: &mut HashMap<String, Env>, target: &str, env: &Env, changed: &mut bool) {
    match map.get(target) {
        Some(existing) => {
            let j = join_env(existing, env);
            if &j != existing {
                map.insert(target.to_string(), j);
                *changed = true;
            }
        }
        None => {
            map.insert(target.to_string(), env.clone());
            *changed = true;
        }
    }
}
