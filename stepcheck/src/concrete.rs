//! An independent *execution oracle* that witnesses the data-flow soundness
//! theorem (Theorem 1) by a different algorithm than the analysis it checks.
//!
//! Where [`crate::passes::dataflow`] computes one entry shape per state by a
//! join-based forward fixpoint, this module *enumerates concrete acyclic paths*
//! from the start to a target state and computes the real document along each,
//! representing an opaque region (a task result, a JSONata output, the execution
//! input) as [`CShape::Opaque`]. A reference flagged `SC1101` is then cross-checked:
//!
//!   * `ConfirmedAbsent` — the field is absent on *every* reaching path (the
//!     theorem holds for this finding);
//!   * `Present` — some path makes the field present: a genuine soundness
//!     *counterexample* (must never happen for a real `SC1101`);
//!   * `Unverifiable` — a reaching path leaves the field under `Opaque`, or the
//!     path budget/complexity bound was hit, so the oracle abstains (excluded
//!     from the witness; it never reports a false counterexample).
//!
//! Because the two implementations share no code, agreement is meaningful
//! evidence that the abstract transfer functions and join are sound.

use crate::ir::{ResultPath, State, StateKind, Workflow};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// A concrete document along one path. `Opaque` = could hold any field.
#[derive(Clone, Debug)]
pub enum CShape {
    Opaque,
    Rec(BTreeMap<String, CShape>),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Verdict {
    ConfirmedAbsent,
    Present,
    Unverifiable,
}

const PATH_CAP: usize = 4000;

fn leaf() -> CShape {
    CShape::Rec(BTreeMap::new())
}

/// Simple dotted JSONPath (`$`, `$.a`, `$.a.b`); `None` for anything richer.
fn parse_path(p: &str) -> Option<Vec<String>> {
    let rest = p.strip_prefix('$')?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = rest.strip_prefix('.')?;
    let mut segs = Vec::new();
    for seg in rest.split('.') {
        if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return None;
        }
        segs.push(seg.to_string());
    }
    Some(segs)
}

fn nav(doc: &CShape, segs: &[String]) -> CShape {
    if segs.is_empty() {
        return doc.clone();
    }
    match doc {
        CShape::Opaque => CShape::Opaque,
        CShape::Rec(m) => match m.get(&segs[0]) {
            Some(sub) => nav(sub, &segs[1..]),
            None => CShape::Opaque, // narrowing to a missing field is unknown
        },
    }
}

fn narrow(doc: &CShape, path: &Option<Value>) -> CShape {
    match path {
        None => doc.clone(),
        Some(Value::Null) => leaf(),
        Some(Value::String(p)) if p == "$" => doc.clone(),
        Some(Value::String(p)) => match parse_path(p) {
            Some(segs) => nav(doc, &segs),
            None => CShape::Opaque,
        },
        Some(_) => doc.clone(),
    }
}

fn literal(v: &Value) -> CShape {
    match v {
        Value::Object(m) => CShape::Rec(m.iter().map(|(k, val)| (k.clone(), literal(val))).collect()),
        Value::Array(_) => CShape::Opaque,
        _ => leaf(),
    }
}

fn constructor(v: &Value) -> CShape {
    match v {
        Value::Object(m) => {
            let mut out = BTreeMap::new();
            for (k, val) in m {
                if let Some(name) = k.strip_suffix(".$") {
                    out.insert(name.to_string(), CShape::Opaque);
                } else {
                    out.insert(k.clone(), constructor(val));
                }
            }
            CShape::Rec(out)
        }
        Value::Array(_) => CShape::Opaque,
        _ => leaf(),
    }
}

fn place_in(doc: &CShape, segs: &[String], val: CShape) -> CShape {
    if segs.is_empty() {
        return val;
    }
    match doc {
        CShape::Opaque => CShape::Opaque,
        CShape::Rec(m) => {
            let mut m = m.clone();
            let child = m.get(&segs[0]).cloned().unwrap_or_else(leaf);
            m.insert(segs[0].clone(), place_in(&child, &segs[1..], val));
            CShape::Rec(m)
        }
    }
}

fn place(doc: &CShape, rp: &ResultPath, val: CShape) -> CShape {
    match rp {
        ResultPath::Default => val,
        ResultPath::Discard => doc.clone(),
        ResultPath::Path(p) => match parse_path(p) {
            Some(segs) if !segs.is_empty() => place_in(doc, &segs, val),
            Some(_) => val,
            None => CShape::Opaque,
        },
    }
}

fn result_shape(st: &State, eff_in: &CShape) -> CShape {
    match st.kind {
        StateKind::Task | StateKind::Map | StateKind::Parallel => match &st.result_selector {
            Some(rs) => constructor(rs),
            None => CShape::Opaque,
        },
        StateKind::Pass => {
            if let Some(r) = &st.result {
                literal(r)
            } else if let Some(p) = &st.parameters {
                constructor(p)
            } else {
                eff_in.clone()
            }
        }
        _ => eff_in.clone(),
    }
}

/// Document leaving a state along this path (mirrors `dataflow::out_shape`).
fn out_doc(st: &State, in_doc: &CShape) -> CShape {
    if st.is_opaque_query() {
        return CShape::Opaque;
    }
    let eff_in = narrow(in_doc, &st.input_path);
    let combined = match st.kind {
        StateKind::Task | StateKind::Pass | StateKind::Map | StateKind::Parallel => {
            place(in_doc, &st.result_path, result_shape(st, &eff_in))
        }
        _ => eff_in,
    };
    narrow(&combined, &st.output_path)
}

fn presence(doc: &CShape, segs: &[String]) -> Verdict {
    match doc {
        _ if segs.is_empty() => Verdict::Present,
        CShape::Opaque => Verdict::Unverifiable,
        CShape::Rec(m) => match m.get(&segs[0]) {
            Some(sub) => presence(sub, &segs[1..]),
            None => Verdict::ConfirmedAbsent,
        },
    }
}

/// Seed document, mirroring `dataflow::root_seed`.
fn root_seed(wf: &Workflow) -> CShape {
    let fields = wf.input_fields.clone().or_else(|| {
        wf.states.get(&wf.start_at).and_then(|s| s.anno.input_fields.clone())
    });
    match fields {
        Some(f) => CShape::Rec(f.into_iter().map(|k| (k, CShape::Opaque)).collect()),
        None => CShape::Opaque,
    }
}

/// Enumerate acyclic paths to `target` and collect its entry document on each.
fn entry_docs(wf: &Workflow, target: &str) -> (Vec<CShape>, bool) {
    let mut entries = Vec::new();
    let mut capped = false;
    let mut on_path: HashSet<String> = HashSet::new();
    fn go(
        wf: &Workflow, cur: &str, doc: CShape, target: &str,
        on_path: &mut HashSet<String>, entries: &mut Vec<CShape>, capped: &mut bool,
    ) {
        if entries.len() >= PATH_CAP {
            *capped = true;
            return;
        }
        if cur == target {
            entries.push(doc);
            return;
        }
        if !on_path.insert(cur.to_string()) {
            return; // cycle on this path
        }
        if let Some(st) = wf.states.get(cur) {
            let out = out_doc(st, &doc);
            for succ in st.normal_successors() {
                if wf.states.contains_key(succ) {
                    go(wf, succ, out.clone(), target, on_path, entries, capped);
                }
            }
            for c in &st.catch {
                if wf.states.contains_key(&c.next) {
                    let cs = place(&doc, &c.result_path, CShape::Opaque);
                    go(wf, &c.next, cs, target, on_path, entries, capped);
                }
            }
        }
        on_path.remove(cur);
    }
    if wf.states.contains_key(&wf.start_at) {
        go(wf, &wf.start_at, root_seed(wf), target, &mut on_path, &mut entries, &mut capped);
    }
    (entries, capped)
}

/// Cross-check a single flagged reference `path` at `state` against concrete
/// path enumeration. `ConfirmedAbsent` witnesses the theorem; `Present` is a
/// soundness counterexample; `Unverifiable` abstains.
pub fn check_ref(wf: &Workflow, state: &str, path: &str) -> Verdict {
    let Some(segs) = parse_path(path) else { return Verdict::Unverifiable };
    if segs.is_empty() {
        return Verdict::Unverifiable;
    }
    let st = match wf.states.get(state) {
        Some(s) => s,
        None => return Verdict::Unverifiable,
    };
    let (entries, capped) = entry_docs(wf, state);
    if entries.is_empty() || capped {
        return Verdict::Unverifiable;
    }
    let mut any_unverifiable = false;
    for e in &entries {
        let eff = narrow(e, &st.input_path);
        match presence(&eff, &segs) {
            Verdict::Present => return Verdict::Present, // counterexample
            Verdict::Unverifiable => any_unverifiable = true,
            Verdict::ConfirmedAbsent => {}
        }
    }
    if any_unverifiable {
        Verdict::Unverifiable
    } else {
        Verdict::ConfirmedAbsent
    }
}
