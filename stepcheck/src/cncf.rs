//! CNCF Serverless Workflow frontend: lowers the vendor-neutral Serverless
//! Workflow DSL (1.0, YAML/JSON) into the same [`crate::ir`] the ASL frontend
//! produces. It is a second, independent input format and demonstrates the
//! modularity of the architecture: **no analysis pass and no IR type is changed
//! to support it** --- adding a format is adding a frontend.
//!
//! It covers the task kinds that appear in real workflows: `call`/`run`/`emit`
//! (Task), `set` (Pass), `wait`/`listen` (Wait), `raise` (Fail), `switch`
//! (Choice), `fork` (Parallel), `for` (Map), `try`/`catch` (Task with Retry and
//! Catch), and nested `do` sequences. Because the CNCF DSL can declare JSON
//! Schemas for task input/output, the typed contract check applies *natively*
//! (without inference) wherever schemas are present.

use crate::ir::*;
use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use serde_json::Value;

pub fn parse_str(src: &str, name: &str) -> Result<Workflow> {
    // serde_yaml deserialises YAML *and* JSON into the serde_json data model.
    let v: Value = serde_yaml::from_str(src).context("invalid CNCF YAML/JSON")?;
    lower_workflow(&v, name)
}

/// Where a task hands control when it completes.
#[derive(Clone)]
enum Exit {
    End,
    Next(String),
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

/// The single `{name: definition}` entry of a `do`-list element.
fn one_entry(v: &Value) -> Option<(String, &Value)> {
    let obj = v.as_object()?;
    let (k, val) = obj.iter().next()?;
    Some((k.clone(), val))
}

fn lower_workflow(v: &Value, fallback: &str) -> Result<Workflow> {
    let name = v
        .get("document")
        .and_then(|d| d.get("name"))
        .and_then(|x| x.as_str())
        .unwrap_or(fallback)
        .to_string();
    let do_list = v
        .get("do")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow!("CNCF workflow has no `do` task list"))?;
    let mut states = IndexMap::new();
    let start = lower_do(do_list, &mut states, &Exit::End)
        .with_context(|| format!("lowering CNCF workflow '{name}'"))?;
    let mut wf = Workflow::new(name, start);
    wf.states = states;
    Ok(wf)
}

/// Lower a `do`-list into `states`, wiring sequential control flow. Returns the
/// entry state name. `exit` is where the last task hands control.
fn lower_do(list: &[Value], states: &mut IndexMap<String, State>, exit: &Exit) -> Result<String> {
    if list.is_empty() {
        return Err(anyhow!("empty `do` list"));
    }
    // Each top-level task becomes one state named by its key, so the entry of a
    // task is just its key and positional wiring is straightforward.
    let names: Vec<String> = list
        .iter()
        .filter_map(|e| one_entry(e).map(|(k, _)| k))
        .collect();
    if names.is_empty() {
        return Err(anyhow!("`do` list has no named tasks"));
    }
    for (i, elem) in list.iter().enumerate() {
        let Some((name, def)) = one_entry(elem) else { continue };
        let default_exit = if i + 1 < names.len() {
            Exit::Next(names[i + 1].clone())
        } else {
            exit.clone()
        };
        lower_task(&name, def, states, &default_exit)?;
    }
    Ok(names[0].clone())
}

fn resolve_exit(def: &Value, default: &Exit) -> Exit {
    match s(def, "then") {
        Some("exit") | Some("end") => Exit::End,
        Some("continue") | None => default.clone(),
        Some(target) => Exit::Next(target.to_string()),
    }
}

fn apply_exit(st: &mut State, exit: &Exit) {
    match exit {
        Exit::End => st.end = true,
        Exit::Next(n) => st.next = Some(n.clone()),
    }
}

/// CNCF JSON-Schema property names of a task's `input`/`output` schema, if any.
fn schema_fields(def: &Value, dir: &str) -> Option<Vec<String>> {
    let props = def
        .get(dir)
        .and_then(|d| d.get("schema"))
        .and_then(|s| s.get("document"))
        .and_then(|doc| doc.get("properties"))
        .and_then(|p| p.as_object())?;
    Some(props.keys().cloned().collect())
}

fn lower_task(
    name: &str,
    def: &Value,
    states: &mut IndexMap<String, State>,
    default_exit: &Exit,
) -> Result<()> {
    let exit = resolve_exit(def, default_exit);

    // dispatch on the task-kind key present in the definition
    if let Some(call) = def.get("call") {
        let mut st = State::new(name, StateKind::Task);
        st.resource = Some(format!("cncf:call:{}", call.as_str().unwrap_or("custom")));
        st.parameters = def.get("with").cloned();
        st.anno.input_fields = schema_fields(def, "input");
        st.anno.output_fields = schema_fields(def, "output");
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if def.get("run").is_some() || def.get("emit").is_some() {
        let mut st = State::new(name, StateKind::Task);
        st.resource = Some(if def.get("emit").is_some() { "cncf:emit" } else { "cncf:run" }.into());
        st.anno.input_fields = schema_fields(def, "input");
        st.anno.output_fields = schema_fields(def, "output");
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if def.get("set").is_some() {
        let mut st = State::new(name, StateKind::Pass);
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if def.get("wait").is_some() || def.get("listen").is_some() {
        // `listen` waits for events; modelled as a Wait (no AWS analogue).
        let mut st = State::new(name, StateKind::Wait);
        st.resource = def.get("listen").map(|_| "cncf:listen".to_string());
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if def.get("raise").is_some() {
        if def.get("if").is_some() {
            // A conditionally-raised error: when the `if` guard is false the task
            // is skipped and control falls through, so the successor stays
            // reachable. The error path itself is abstracted.
            let mut st = State::new(name, StateKind::Pass);
            apply_exit(&mut st, default_exit);
            states.insert(name.to_string(), st);
        } else {
            let st = State::new(name, StateKind::Fail);
            states.insert(name.to_string(), st);
        }
    } else if let Some(sw) = def.get("switch").and_then(|x| x.as_array()) {
        let mut st = State::new(name, StateKind::Choice);
        for case in sw {
            let Some((_, body)) = one_entry(case) else { continue };
            let Some(target) = s(body, "then") else { continue };
            if body.get("when").is_none() {
                st.default = Some(target.to_string());
            } else {
                // Store the CNCF runtime expression under a key the ASL-specific
                // JSONPath checks ignore (it is jq, not JSONPath).
                let mut cond = serde_json::Map::new();
                cond.insert("CncfWhen".into(), body.get("when").cloned().unwrap_or(Value::Null));
                st.choices.push(ChoiceRule { next: target.to_string(), condition: Value::Object(cond) });
            }
        }
        states.insert(name.to_string(), st);
    } else if let Some(branches) = def.get("fork").and_then(|f| f.get("branches")).and_then(|b| b.as_array()) {
        let mut st = State::new(name, StateKind::Parallel);
        for (i, b) in branches.iter().enumerate() {
            let mut bstates = IndexMap::new();
            let bentry = lower_do(std::slice::from_ref(b), &mut bstates, &Exit::End)?;
            let mut bwf = Workflow::new(format!("{name}::branch{i}"), bentry);
            bwf.states = bstates;
            st.branches.push(bwf);
        }
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if let Some(forr) = def.get("for") {
        // In the CNCF DSL the loop body `do:` is a *sibling* of `for:`, which
        // itself only carries the iteration config (`each`/`in`/`at`).
        let mut st = State::new(name, StateKind::Map);
        if let Some(inner) = def.get("do").and_then(|x| x.as_array()) {
            let mut istates = IndexMap::new();
            let ientry = lower_do(inner, &mut istates, &Exit::End)?;
            let mut iwf = Workflow::new(format!("{name}::iterator"), ientry);
            iwf.states = istates;
            st.iterator = Some(Box::new(iwf));
        }
        st.items_path = s(forr, "in").map(String::from);
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if def.get("try").is_some() {
        // The protected block is collapsed into a single Task carrying the
        // catch's retry/handler structure, which is what the retry and
        // compensation checks reason about.
        let mut st = State::new(name, StateKind::Task);
        st.resource = Some("cncf:try".into());
        let catch = def.get("catch");
        let err_type = catch
            .and_then(|c| c.get("errors"))
            .and_then(|e| e.get("with"))
            .and_then(|w| w.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("CommunicationError")
            .to_string();
        if let Some(retry) = catch.and_then(|c| c.get("retry")) {
            let max = retry
                .get("limit")
                .and_then(|l| l.get("attempt"))
                .and_then(|a| a.get("count"))
                .and_then(|c| c.as_i64());
            let interval = retry.get("delay").and_then(|d| d.get("seconds")).and_then(|x| x.as_f64());
            let backoff = retry.get("backoff").and_then(|b| b.get("exponential")).map(|_| 2.0);
            st.retry.push(RetryRule {
                error_equals: vec![err_type.clone()],
                max_attempts: max,
                interval_seconds: interval,
                backoff_rate: backoff,
            });
        }
        // a catch handler (`catch.do`) becomes reachable via a Catch transition
        if let Some(handler) = catch.and_then(|c| c.get("do")).and_then(|x| x.as_array()) {
            let hentry = lower_do(handler, states, &exit)?;
            st.catch.push(CatchRule { error_equals: vec![err_type], next: hentry, result_path: ResultPath::Default, assign: None });
        }
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    } else if let Some(inner) = def.get("do").and_then(|x| x.as_array()) {
        // a nested `do` sequence: a transparent Pass entering the sub-sequence
        let sub_entry = lower_do(inner, states, &exit)?;
        let mut st = State::new(name, StateKind::Pass);
        st.next = Some(sub_entry);
        states.insert(name.to_string(), st);
    } else {
        // unrecognised task kind: keep a transparent node so we never reject input
        let mut st = State::new(name, StateKind::Pass);
        apply_exit(&mut st, &exit);
        states.insert(name.to_string(), st);
    }
    Ok(())
}
