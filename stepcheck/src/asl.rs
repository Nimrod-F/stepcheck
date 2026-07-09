//! Amazon States Language (ASL) frontend: parse real Step Functions JSON into
//! the [`crate::ir`]. Deliberately *tolerant* — it lowers from `serde_json::Value`
//! rather than rigid structs, so the wide variety of real-world ASL (intrinsic
//! functions, `.$` parameters, `$$` context, SAM `${}` placeholders, `Iterator`
//! vs `ItemProcessor`, …) never causes a hard parse failure.

use crate::ir::*;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

pub fn parse_str(src: &str, name: &str) -> Result<Workflow> {
    let mut de = serde_json::Deserializer::from_str(src);
    de.disable_recursion_limit();
    let de = serde_stacker::Deserializer::new(&mut de);
    let v = Value::deserialize(de).context("invalid JSON")?;
    lower_machine(&v, name, QueryLang::JsonPath)
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn extra_fields(v: &Value, known: &[&str]) -> Map<String, Value> {
    let Some(obj) = v.as_object() else { return Map::new() };
    obj.iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Resolve a `QueryLanguage` directive against an inherited default.
fn query_lang(v: &Value, parent: QueryLang) -> QueryLang {
    match s(v, "QueryLanguage").as_deref() {
        Some("JSONata") => QueryLang::JsonAta,
        Some("JSONPath") => QueryLang::JsonPath,
        _ => parent,
    }
}

/// Whether a state object uses JSONata constructs (`Arguments`/`Output`, or any
/// `{% … %}` expression), independently of an explicit `QueryLanguage` directive.
/// Such states reshape their document with expressions we do not model, so the
/// analysis must treat them opaquely to stay sound.
fn has_jsonata_constructs(v: &Value) -> bool {
    if v.get("Arguments").is_some() || v.get("Output").is_some() {
        return true;
    }
    fn scan(v: &Value) -> bool {
        match v {
            Value::String(s) => s.contains("{%"),
            Value::Object(m) => m.values().any(scan),
            Value::Array(a) => a.iter().any(scan),
            _ => false,
        }
    }
    scan(v)
}

fn lower_machine(v: &Value, name: &str, parent_ql: QueryLang) -> Result<Workflow> {
    let machine_ql = query_lang(v, parent_ql);
    let start_at = s(v, "StartAt").ok_or_else(|| anyhow!("missing StartAt in {name}"))?;
    let states_obj = v
        .get("States")
        .and_then(|x| x.as_object())
        .ok_or_else(|| anyhow!("missing States object in {name}"))?;

    let mut wf = Workflow::new(name, start_at);
    wf.comment = s(v, "Comment");
    wf.timeout_seconds = v.get("TimeoutSeconds").and_then(|x| x.as_f64());
    wf.extra = extra_fields(v, &["Comment", "StartAt", "States", "TimeoutSeconds"]);
    for (sname, sval) in states_obj {
        // A state entry whose value is not an object is malformed (e.g. a
        // top-level `QueryLanguage` directive mistakenly nested in `States`).
        // Keep it as a marked placeholder so structural validation can report
        // it precisely instead of crashing.
        let st = if sval.is_object() {
            lower_state(sname, sval, machine_ql)
                .with_context(|| format!("while lowering state '{sname}'"))?
        } else {
            State::new(sname.clone(), StateKind::Unknown("<non-object>".into()))
        };
        wf.states.insert(sname.clone(), st);
    }
    Ok(wf)
}

fn result_path(v: &Value) -> ResultPath {
    match v.get("ResultPath") {
        None => ResultPath::Default,
        Some(Value::Null) => ResultPath::Discard,
        Some(Value::String(p)) => ResultPath::Path(p.clone()),
        Some(_) => ResultPath::Default,
    }
}

fn is_distributed_map(v: &Value) -> bool {
    if v.get("ItemReader").is_some() || v.get("ItemBatcher").is_some() {
        return true;
    }
    v.get("ItemProcessor")
        .and_then(|x| x.get("ProcessorConfig"))
        .and_then(|x| x.get("Mode"))
        .and_then(|x| x.as_str())
        .map(|s| s == "DISTRIBUTED")
        .unwrap_or(false)
}

fn lower_state(name: &str, v: &Value, machine_ql: QueryLang) -> Result<State> {
    let kind = match s(v, "Type").as_deref() {
        Some("Task") => StateKind::Task,
        Some("Choice") => StateKind::Choice,
        Some("Parallel") => StateKind::Parallel,
        Some("Map") => StateKind::Map,
        Some("Wait") => StateKind::Wait,
        Some("Pass") => StateKind::Pass,
        Some("Succeed") => StateKind::Succeed,
        Some("Fail") => StateKind::Fail,
        Some(other) => StateKind::Unknown(other.to_string()),
        None => StateKind::Unknown("<missing>".into()),
    };

    let mut st = State::new(name, kind.clone());
    st.extra = extra_fields(v, &[
        "Type", "Comment", "Resource", "Next", "End", "InputPath", "OutputPath", "ResultPath",
        "Parameters", "Arguments", "Assign", "Output", "Items", "ResultSelector", "Result", "Retry",
        "Catch", "Choices", "Default", "TimeoutSeconds", "HeartbeatSeconds", "MaxConcurrency",
        "ItemSelector", "Iterator", "ItemProcessor", "ItemsPath", "Branches",
    ]);
    // Effective query language: explicit directive, else the machine default;
    // a state using JSONata constructs is treated as JSONata regardless, so the
    // data-flow analysis stays sound on it.
    st.query_language = if has_jsonata_constructs(v) {
        QueryLang::JsonAta
    } else {
        query_lang(v, machine_ql)
    };
    st.comment = s(v, "Comment");
    st.resource = s(v, "Resource");
    st.next = s(v, "Next");
    st.end = v.get("End").and_then(|x| x.as_bool()).unwrap_or(false);
    st.input_path = v.get("InputPath").cloned();
    st.output_path = v.get("OutputPath").cloned();
    st.result_path = result_path(v);
    st.parameters = v.get("Parameters").cloned();
    st.arguments = v.get("Arguments").cloned();
    st.assign = v.get("Assign").cloned();
    st.output = v.get("Output").cloned();
    st.items = v.get("Items").cloned();
    st.result_selector = v.get("ResultSelector").cloned();
    st.result = v.get("Result").cloned();
    st.items_path = s(v, "ItemsPath");
    st.default = s(v, "Default");
    st.timeout_seconds = v.get("TimeoutSeconds").and_then(|x| x.as_f64());
    st.heartbeat_seconds = v.get("HeartbeatSeconds").and_then(|x| x.as_f64());
    st.max_concurrency = v.get("MaxConcurrency").and_then(|x| x.as_i64());
    st.distributed_map = matches!(kind, StateKind::Map) && is_distributed_map(v);
    st.map_uses_item_processor = v.get("ItemProcessor").is_some();
    // `ItemSelector` (distributed Map) / `Parameters` of a Map describe the
    // per-item document handed to each iteration.
    st.item_selector = v.get("ItemSelector").cloned();

    // Retry rules
    if let Some(arr) = v.get("Retry").and_then(|x| x.as_array()) {
        for r in arr {
            st.retry.push(RetryRule {
                error_equals: str_array(r.get("ErrorEquals")),
                max_attempts: r.get("MaxAttempts").and_then(|x| x.as_i64()),
                interval_seconds: r.get("IntervalSeconds").and_then(|x| x.as_f64()),
                backoff_rate: r.get("BackoffRate").and_then(|x| x.as_f64()),
                extra: extra_fields(r, &["ErrorEquals", "MaxAttempts", "IntervalSeconds", "BackoffRate"]),
            });
        }
    }
    // Catch rules
    if let Some(arr) = v.get("Catch").and_then(|x| x.as_array()) {
        for c in arr {
            if let Some(next) = s(c, "Next") {
                st.catch.push(CatchRule {
                    error_equals: str_array(c.get("ErrorEquals")),
                    next,
                    result_path: result_path(c),
                    assign: c.get("Assign").cloned(),
                    extra: extra_fields(c, &["ErrorEquals", "Next", "ResultPath", "Assign"]),
                });
            }
        }
    }
    // Choice rules
    if let Some(arr) = v.get("Choices").and_then(|x| x.as_array()) {
        for c in arr {
            if let Some(next) = s(c, "Next") {
                let mut cond = c.clone();
                // drop the Next key from the stored condition for clarity
                if let Some(obj) = cond.as_object_mut() {
                    obj.remove("Next");
                }
                st.choices.push(ChoiceRule { next, condition: cond });
            }
        }
    }
    // Map sub-machine: Iterator (classic) or ItemProcessor (distributed/new)
    let sub = v.get("Iterator").or_else(|| v.get("ItemProcessor"));
    if let Some(subv) = sub {
        let it = lower_machine(subv, &format!("{name}::iterator"), machine_ql)
            .with_context(|| format!("in Map iterator of '{name}'"))?;
        st.iterator = Some(Box::new(it));
    }
    // Parallel branches
    if let Some(arr) = v.get("Branches").and_then(|x| x.as_array()) {
        for (i, b) in arr.iter().enumerate() {
            let br = lower_machine(b, &format!("{name}::branch{i}"), machine_ql)
                .with_context(|| format!("in Parallel branch {i} of '{name}'"))?;
            st.branches.push(br);
        }
    }

    Ok(st)
}

/// Emit a duration as an integer when it is whole (ASL `TimeoutSeconds`/
/// `HeartbeatSeconds` are integer-typed), else as a float.
fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        Value::from(x)
    }
}

fn str_array(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Re-emit a workflow to ASL JSON. Used by the DSL frontend / AWS round-trip.
pub fn emit(wf: &Workflow) -> Value {
    let mut states = serde_json::Map::new();
    for (name, st) in &wf.states {
        states.insert(name.clone(), emit_state(st));
    }
    let mut top = wf.extra.clone();
    if let Some(c) = &wf.comment {
        top.insert("Comment".into(), Value::String(c.clone()));
    }
    if let Some(t) = wf.timeout_seconds {
        top.insert("TimeoutSeconds".into(), num(t));
    }
    top.insert("StartAt".into(), Value::String(wf.start_at.clone()));
    top.insert("States".into(), Value::Object(states));
    Value::Object(top)
}

fn emit_state(st: &State) -> Value {
    let mut o = st.extra.clone();
    o.insert("Type".into(), Value::String(st.kind.as_str().to_string()));
    if let Some(c) = &st.comment {
        o.insert("Comment".into(), Value::String(c.clone()));
    }
    if let Some(r) = &st.resource {
        o.insert("Resource".into(), Value::String(r.clone()));
    }
    if let Some(ip) = &st.input_path {
        o.insert("InputPath".into(), ip.clone());
    }
    if let Some(p) = &st.parameters {
        o.insert("Parameters".into(), p.clone());
    }
    if let Some(a) = &st.arguments {
        o.insert("Arguments".into(), a.clone());
    }
    if let Some(a) = &st.assign {
        o.insert("Assign".into(), a.clone());
    }
    if let Some(o2) = &st.output {
        o.insert("Output".into(), o2.clone());
    }
    if let Some(r) = &st.result {
        o.insert("Result".into(), r.clone());
    }
    if let Some(rs) = &st.result_selector {
        o.insert("ResultSelector".into(), rs.clone());
    }
    if let Some(op) = &st.output_path {
        o.insert("OutputPath".into(), op.clone());
    }
    match &st.result_path {
        ResultPath::Default => {}
        ResultPath::Discard => {
            o.insert("ResultPath".into(), Value::Null);
        }
        ResultPath::Path(p) => {
            o.insert("ResultPath".into(), Value::String(p.clone()));
        }
    }
    if !st.retry.is_empty() {
        o.insert("Retry".into(), Value::Array(st.retry.iter().map(emit_retry).collect()));
    }
    if !st.catch.is_empty() {
        o.insert("Catch".into(), Value::Array(st.catch.iter().map(emit_catch).collect()));
    }
    if !st.choices.is_empty() {
        let arr = st
            .choices
            .iter()
            .map(|c| {
                let mut cv = c.condition.clone();
                if let Some(m) = cv.as_object_mut() {
                    m.insert("Next".into(), Value::String(c.next.clone()));
                }
                cv
            })
            .collect();
        o.insert("Choices".into(), Value::Array(arr));
    }
    if let Some(d) = &st.default {
        o.insert("Default".into(), Value::String(d.clone()));
    }
    if let Some(t) = st.timeout_seconds {
        o.insert("TimeoutSeconds".into(), num(t));
    }
    if let Some(h) = st.heartbeat_seconds {
        o.insert("HeartbeatSeconds".into(), num(h));
    }
    if let Some(mc) = st.max_concurrency {
        o.insert("MaxConcurrency".into(), Value::from(mc));
    }
    if let Some(is) = &st.item_selector {
        o.insert("ItemSelector".into(), is.clone());
    }
    if let Some(it) = &st.iterator {
        let key = if st.map_uses_item_processor { "ItemProcessor" } else { "Iterator" };
        o.insert(key.into(), emit(it));
    }
    if let Some(ip) = &st.items_path {
        o.insert("ItemsPath".into(), Value::String(ip.clone()));
    }
    if let Some(items) = &st.items {
        o.insert("Items".into(), items.clone());
    }
    if !st.branches.is_empty() {
        o.insert("Branches".into(), Value::Array(st.branches.iter().map(emit).collect()));
    }
    if let Some(n) = &st.next {
        o.insert("Next".into(), Value::String(n.clone()));
    }
    // Succeed/Fail are implicitly terminal; ASL rejects an explicit End on them.
    if st.end && !matches!(st.kind, StateKind::Succeed | StateKind::Fail) {
        o.insert("End".into(), Value::Bool(true));
    }
    Value::Object(o)
}

fn emit_retry(r: &RetryRule) -> Value {
    let mut o = r.extra.clone();
    o.insert(
        "ErrorEquals".into(),
        Value::Array(r.error_equals.iter().map(|e| Value::String(e.clone())).collect()),
    );
    if let Some(m) = r.max_attempts {
        o.insert("MaxAttempts".into(), Value::from(m));
    }
    if let Some(i) = r.interval_seconds {
        o.insert("IntervalSeconds".into(), Value::from(i));
    }
    if let Some(b) = r.backoff_rate {
        o.insert("BackoffRate".into(), Value::from(b));
    }
    Value::Object(o)
}

fn emit_catch(c: &CatchRule) -> Value {
    let mut o = c.extra.clone();
    o.insert(
        "ErrorEquals".into(),
        Value::Array(c.error_equals.iter().map(|e| Value::String(e.clone())).collect()),
    );
    if let ResultPath::Path(p) = &c.result_path {
        o.insert("ResultPath".into(), Value::String(p.clone()));
    } else if let ResultPath::Discard = &c.result_path {
        o.insert("ResultPath".into(), Value::Null);
    }
    if let Some(assign) = &c.assign {
        o.insert("Assign".into(), assign.clone());
    }
    o.insert("Next".into(), Value::String(c.next.clone()));
    Value::Object(o)
}
