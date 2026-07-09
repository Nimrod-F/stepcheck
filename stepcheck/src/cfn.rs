//! CloudFormation / SAM / CDK frontend.
//!
//! Extracts every `AWS::StepFunctions::StateMachine` from a CloudFormation or SAM
//! template (YAML or JSON) or a CDK synth output template
//! (`cdk.out/*.template.json`), resolving the CloudFormation intrinsics in the
//! state machine definition into runnable ASL. This lets \tool run directly on
//! the deployment artifact a team actually commits---not only a standalone
//! `.asl.json`. One template can define several machines; each is returned
//! separately, keyed by its logical id.
//!
//! Resolution keeps the AWS pseudo-parameters exact (`AWS::Partition` -> `aws`,
//! `AWS::Region`, `AWS::AccountId`, `AWS::URLSuffix`) so that service-integration
//! ARNs such as `arn:aws:states:::sns:publish` survive, and maps resource
//! references (`${Fn.Arn}`, `!Ref`, `!GetAtt`) to stable placeholder ARNs, which
//! is all the analyses need (the identity of a resource, not its live ARN).
//! `DefinitionSubstitutions` provided in the template are applied first.

use crate::asl;
use crate::ir::Workflow;
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

/// Cheap textual signal that a file is a CloudFormation/SAM/CDK template rather
/// than a bare ASL workflow or a CNCF Serverless Workflow. Matches both the raw
/// CloudFormation resource and the SAM (`AWS::Serverless::StateMachine`) form.
pub fn looks_like_template(text: &str) -> bool {
    text.contains("AWS::StepFunctions::StateMachine")
        || text.contains("AWS::Serverless::StateMachine")
}

fn is_state_machine(res: &Value) -> bool {
    matches!(
        res.get("Type").and_then(|t| t.as_str()),
        Some("AWS::StepFunctions::StateMachine") | Some("AWS::Serverless::StateMachine")
    )
}

/// Extract all state machines from a template. Returns `(logical_id, workflow)`.
/// `base_dir` (the template's directory) lets a `DefinitionUri` be followed to a
/// sibling ASL file, the dominant SAM idiom; pass `None` when there is no path.
pub fn extract(src: &str, filename: &str, base_dir: Option<&Path>) -> Result<Vec<(String, Workflow)>> {
    let root = parse_template(src)?;
    let resources = root
        .get("Resources")
        .and_then(|r| r.as_object())
        .ok_or_else(|| anyhow!("CloudFormation template has no Resources section"))?;

    let state_machine_ids: BTreeSet<String> = resources
        .iter()
        .filter(|(_, res)| is_state_machine(res))
        .map(|(logical_id, _)| logical_id.clone())
        .collect();

    let mut out = Vec::new();
    let mut aliases_by_id = BTreeMap::<String, Vec<String>>::new();
    for (logical_id, res) in resources {
        if !is_state_machine(res) {
            continue;
        }
        let Some(props) = res.get("Properties").and_then(|p| p.as_object()) else {
            continue;
        };
        let subs = props
            .get("DefinitionSubstitutions")
            .and_then(|s| s.as_object())
            .cloned()
            .unwrap_or_default();
        let state_machine_name = props
            .get("StateMachineName")
            .map(|v| resolve_scalar(v, &subs, &state_machine_ids))
            .filter(|s| !s.is_empty());
        aliases_by_id.insert(logical_id.clone(), state_machine_aliases(logical_id, state_machine_name.as_deref()));

        let asl_value = if let Some(defs) = props.get("DefinitionString") {
            let text = resolve_to_string(defs, &subs, &state_machine_ids)
                .with_context(|| format!("resolving DefinitionString of '{logical_id}'"))?;
            serde_json::from_str::<Value>(&text).with_context(|| {
                format!("state machine '{logical_id}' definition is not valid JSON after intrinsic resolution")
            })?
        } else if let Some(def) = props.get("Definition") {
            // SAM inline Definition: an ASL object possibly holding intrinsics.
            resolve_value(def, &subs, &state_machine_ids)
        } else if let (Some(uri), Some(base)) =
            (props.get("DefinitionUri").and_then(|u| u.as_str()), base_dir)
        {
            // SAM `DefinitionUri: statemachine/foo.asl.json` -> read the sibling
            // ASL file and apply the template's DefinitionSubstitutions (${...}).
            let full = base.join(uri);
            let Ok(body) = std::fs::read_to_string(&full) else { continue };
            let resolved = resolve_placeholders(&body, &subs, &state_machine_ids);
            serde_json::from_str::<Value>(&resolved).with_context(|| {
                format!("state machine '{logical_id}' DefinitionUri {uri} is not valid ASL JSON")
            })?
        } else {
            // DefinitionUri as an S3 object, or none: cannot resolve locally.
            continue;
        };
        let text = serde_json::to_string(&asl_value)?;
        let wf = asl::parse_str(&text, &format!("{filename}::{logical_id}"))
            .with_context(|| format!("parsing extracted ASL of '{logical_id}'"))?;
        out.push((logical_id.clone(), wf));
    }
    if out.is_empty() {
        return Err(anyhow!(
            "no AWS::StepFunctions::StateMachine with an inline definition found (DefinitionUri references are not resolved)"
        ));
    }
    attach_linked_children(&mut out, &aliases_by_id);
    Ok(out)
}

fn attach_linked_children(out: &mut [(String, Workflow)], aliases_by_id: &BTreeMap<String, Vec<String>>) {
    let mut registry = BTreeMap::new();
    for (logical_id, wf) in out.iter() {
        let aliases = aliases_by_id
            .get(logical_id)
            .cloned()
            .unwrap_or_else(|| state_machine_aliases(logical_id, None));
        for alias in aliases {
            registry.insert(alias, wf.clone());
        }
    }
    let shared = Rc::new(registry);
    for (_, wf) in out.iter_mut() {
        attach_registry_recursive(wf, shared.clone());
    }
}

fn attach_registry_recursive(wf: &mut Workflow, registry: Rc<BTreeMap<String, Workflow>>) {
    wf.linked_children = registry.clone();
    for st in wf.states.values_mut() {
        if let Some(it) = st.iterator.as_mut() {
            attach_registry_recursive(it, registry.clone());
        }
        for br in st.branches.iter_mut() {
            attach_registry_recursive(br, registry.clone());
        }
    }
}

fn state_machine_aliases(logical_id: &str, state_machine_name: Option<&str>) -> Vec<String> {
    let mut aliases = vec![
        format!("arn:aws:states:us-east-1:000000000000:stateMachine:{logical_id}"),
        format!("${{{logical_id}}}"),
        format!("${{{logical_id}.Arn}}"),
    ];
    if let Some(name) = state_machine_name {
        aliases.push(name.to_string());
        aliases.push(format!("arn:aws:states:us-east-1:000000000000:stateMachine:{name}"));
    }
    aliases
}

/// Parse a template body. CDK synth output and JSON CloudFormation are plain JSON
/// (intrinsics already in `Fn::` long form); YAML CloudFormation uses the
/// short-form intrinsic tags (`!Sub`, `!Ref`, `!GetAtt`, ...), which we normalise.
fn parse_template(src: &str) -> Result<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(src) {
        return Ok(v);
    }
    let y: serde_yaml::Value =
        serde_yaml::from_str(src).context("invalid CloudFormation/SAM YAML")?;
    Ok(yaml_to_json(&y))
}

fn yaml_to_json(v: &serde_yaml::Value) -> Value {
    use serde_yaml::Value as Y;
    match v {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(*b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(u) = n.as_u64() {
                Value::from(u)
            } else {
                serde_json::Number::from_f64(n.as_f64().unwrap_or(0.0))
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }
        }
        Y::String(s) => Value::String(s.clone()),
        Y::Sequence(seq) => Value::Array(seq.iter().map(yaml_to_json).collect()),
        Y::Mapping(m) => {
            let mut o = Map::new();
            for (k, val) in m {
                let key = match k {
                    Y::String(s) => s.clone(),
                    other => yaml_to_json(other).to_string(),
                };
                o.insert(key, yaml_to_json(val));
            }
            Value::Object(o)
        }
        Y::Tagged(t) => translate_tag(&t.tag.to_string(), &t.value),
    }
}

/// Translate a CloudFormation short-form intrinsic tag into its `Fn::` long form.
fn translate_tag(tag: &str, value: &serde_yaml::Value) -> Value {
    let short = tag.trim_start_matches('!');
    let inner = yaml_to_json(value);
    let long = match short {
        "Ref" => return obj("Ref", inner),
        "Condition" => return obj("Condition", inner),
        "GetAtt" => {
            let ga = match &inner {
                Value::String(s) => {
                    Value::Array(s.split('.').map(|p| Value::String(p.to_string())).collect())
                }
                other => other.clone(),
            };
            return obj("Fn::GetAtt", ga);
        }
        "Sub" => "Fn::Sub",
        "Join" => "Fn::Join",
        "Select" => "Fn::Select",
        "Split" => "Fn::Split",
        "FindInMap" => "Fn::FindInMap",
        "GetAZs" => "Fn::GetAZs",
        "ImportValue" => "Fn::ImportValue",
        "Base64" => "Fn::Base64",
        "Cidr" => "Fn::Cidr",
        "If" => "Fn::If",
        "Equals" => "Fn::Equals",
        "And" => "Fn::And",
        "Or" => "Fn::Or",
        "Not" => "Fn::Not",
        _ => return inner, // unknown tag: pass the value through
    };
    obj(long, inner)
}

fn obj(key: &str, v: Value) -> Value {
    let mut m = Map::new();
    m.insert(key.to_string(), v);
    Value::Object(m)
}

/// Resolve a `DefinitionString` node (a literal string, `Fn::Sub`, or `Fn::Join`)
/// into the ASL JSON text.
fn resolve_to_string(node: &Value, subs: &Map<String, Value>, state_machines: &BTreeSet<String>) -> Result<String> {
    match node {
        Value::String(s) => Ok(resolve_placeholders(s, subs, state_machines)),
        Value::Object(m) if m.contains_key("Fn::Sub") => {
            let (s, local) = match &m["Fn::Sub"] {
                Value::Array(a) if a.len() == 2 => (
                    a[0].as_str().unwrap_or("").to_string(),
                    a[1].as_object().cloned().unwrap_or_default(),
                ),
                Value::String(s) => (s.clone(), Map::new()),
                _ => return Err(anyhow!("unsupported Fn::Sub form")),
            };
            let mut merged = subs.clone();
            for (k, v) in local {
                merged.insert(k, v);
            }
            Ok(resolve_placeholders(&s, &merged, state_machines))
        }
        Value::Object(m) if m.contains_key("Fn::Join") => {
            let arr = m["Fn::Join"].as_array().ok_or_else(|| anyhow!("bad Fn::Join"))?;
            let sep = arr.first().and_then(|x| x.as_str()).unwrap_or("");
            let parts = arr.get(1).and_then(|x| x.as_array()).ok_or_else(|| anyhow!("bad Fn::Join parts"))?;
            Ok(parts.iter().map(|p| resolve_scalar(p, subs, state_machines)).collect::<Vec<_>>().join(sep))
        }
        _ => Err(anyhow!("unsupported DefinitionString form")),
    }
}

/// Replace `${...}` substitutions in an `Fn::Sub` template string.
fn resolve_placeholders(s: &str, subs: &Map<String, Value>, state_machines: &BTreeSet<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("${") {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 2..];
        match rest.find('}') {
            Some(end) => {
                out.push_str(&resolve_ref_key(&rest[..end], subs, state_machines));
                rest = &rest[end + 1..];
            }
            None => {
                out.push_str("${");
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The literal a `${key}` (or `!Ref key`) resolves to.
fn resolve_ref_key(key: &str, subs: &Map<String, Value>, state_machines: &BTreeSet<String>) -> String {
    let key = key.trim();
    if let Some(lit) = key.strip_prefix('!') {
        // Fn::Sub literal escape: ${!Literal} stands for the text ${Literal}.
        return format!("${{{lit}}}");
    }
    if let Some(v) = subs.get(key) {
        return resolve_scalar(v, subs, state_machines);
    }
    let base = key.split('.').next().unwrap_or(key);
    match base {
        "AWS::Partition" => "aws".into(),
        "AWS::Region" => "us-east-1".into(),
        "AWS::AccountId" => "000000000000".into(),
        "AWS::URLSuffix" => "amazonaws.com".into(),
        "AWS::StackName" => "Stack".into(),
        "AWS::StackId" => "arn:aws:cloudformation:us-east-1:000000000000:stack/Stack".into(),
        _ if state_machines.contains(base) && (key == base || key.ends_with(".Arn")) => {
            format!("arn:aws:states:us-east-1:000000000000:stateMachine:{base}")
        }
        _ => format!("arn:aws:lambda:us-east-1:000000000000:function:{base}"),
    }
}

/// Resolve a value used in a scalar position (`Fn::Join` part, `Ref`, `GetAtt`).
fn resolve_scalar(node: &Value, subs: &Map<String, Value>, state_machines: &BTreeSet<String>) -> String {
    match node {
        Value::String(s) => s.clone(),
        Value::Object(m) => {
            if let Some(v) = m.get("Ref") {
                return resolve_ref_key(v.as_str().unwrap_or(""), subs, state_machines);
            }
            if let Some(v) = m.get("Fn::GetAtt") {
                let (base, attr) = match v {
                    Value::Array(a) => (
                        a.first().and_then(|x| x.as_str()).unwrap_or(""),
                        a.get(1).and_then(|x| x.as_str()).unwrap_or(""),
                    ),
                    Value::String(s) => {
                        let mut parts = s.splitn(2, '.');
                        (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
                    }
                    _ => ("", ""),
                };
                if attr == "Arn" && state_machines.contains(base) {
                    return format!("arn:aws:states:us-east-1:000000000000:stateMachine:{base}");
                }
                return format!("arn:aws:lambda:us-east-1:000000000000:function:{base}");
            }
            if m.contains_key("Fn::Sub") || m.contains_key("Fn::Join") {
                return resolve_to_string(node, subs, state_machines).unwrap_or_default();
            }
            String::new()
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Resolve intrinsics inside an inline SAM `Definition:` object.
fn resolve_value(node: &Value, subs: &Map<String, Value>, state_machines: &BTreeSet<String>) -> Value {
    match node {
        Value::Object(m) if m.len() == 1 => {
            if m.contains_key("Ref") || m.contains_key("Fn::GetAtt") {
                return Value::String(resolve_scalar(node, subs, state_machines));
            }
            if m.contains_key("Fn::Sub") || m.contains_key("Fn::Join") {
                return Value::String(resolve_to_string(node, subs, state_machines).unwrap_or_default());
            }
            Value::Object(m.iter().map(|(k, v)| (k.clone(), resolve_value(v, subs, state_machines))).collect())
        }
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), resolve_value(v, subs, state_machines))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| resolve_value(v, subs, state_machines)).collect()),
        _ => node.clone(),
    }
}
