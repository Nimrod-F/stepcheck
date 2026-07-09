//! `stepcheck` — a static verifier for serverless workflows (Amazon States
//! Language). Frontends lower to a common IR; a pipeline of analysis passes
//! reads only that IR; an emitter lowers verified workflows back to ASL.

mod annot;
mod asl;
mod cfn;
mod cncf;
mod concrete;
mod diag;
mod dsl;
mod ir;
mod mutate;
mod passes;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "stepcheck", version, about = "Static verifier for serverless workflows (ASL)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Check {
        path: PathBuf,
        /// Sidecar annotation file (TOML).
        #[arg(long)]
        annot: Option<PathBuf>,
        /// Infer idempotency/persistence from task names/resources.
        #[arg(long)]
        infer: bool,
        /// Emit findings as JSON.
        #[arg(long)]
        json: bool,
        /// Treat warnings as errors for the exit code.
        #[arg(long)]
        deny_warnings: bool,
        /// Ablation: model declared task outputs and known AWS service result envelopes.
        #[arg(long)]
        result_shapes: bool,
    },
    /// Print the annotations inferred for a workflow.
    Infer {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run the pipeline over a directory and emit a JSON report (eval harness).
    Scan {
        dir: PathBuf,
        #[arg(long)]
        annot: Option<PathBuf>,
        #[arg(long)]
        infer: bool,
        /// Ablation: model declared task outputs and known AWS service result envelopes.
        #[arg(long)]
        result_shapes: bool,
    },
    /// Print corpus statistics over a directory of ASL files.
    Stats {
        dir: PathBuf,
        #[arg(long)]
        tex: bool,
    },
    /// Run the mutation study + baseline + timing over a corpus (JSON report).
    Eval {
        dir: PathBuf,
        #[arg(long)]
        annot: Option<PathBuf>,
        #[arg(long)]
        infer: bool,
        /// Typed-tier data-flow mode: seed each workflow's input document as a
        /// closed record of the top-level fields it references, and additionally
        /// run the data-flow (SC1101) mutation class.
        #[arg(long)]
        strict_input: bool,
        /// Ablation: model declared task outputs and known AWS service result envelopes.
        #[arg(long)]
        result_shapes: bool,
    },
    /// Cross-check the data-flow `SC1101` findings against an independent
    /// bounded-path execution oracle (witnesses the soundness theorem at corpus
    /// scale, including bounded loop unrolling). Injects a data-flow miss per
    /// workflow (typed tier) so the check fires, then confirms each finding's
    /// field is absent on every explored reaching path; any `Present` is a
    /// soundness counterexample.
    Oracle {
        dir: PathBuf,
        #[arg(long)]
        annot: Option<PathBuf>,
        #[arg(long)]
        infer: bool,
        /// Ablation: model declared task outputs and known AWS service result envelopes.
        #[arg(long)]
        result_shapes: bool,
    },
    /// Certify the data-flow `SC1101` findings by finite fixpoint obligations.
    /// Like `oracle`, this injects one typed-tier missing-field mutant per
    /// workflow so the check fires, but it discharges loops by checking the
    /// widened invariant as a post-fixpoint instead of bounded unrolling.
    DataflowCert {
        dir: PathBuf,
        #[arg(long)]
        annot: Option<PathBuf>,
        #[arg(long)]
        infer: bool,
        /// Ablation: model declared task outputs and known AWS service result envelopes.
        #[arg(long)]
        result_shapes: bool,
    },
    /// Real-bug benchmark: replay StepCheck on (pre-fix, post-fix) workflow pairs.
    /// Place pairs as `<id>-pre.json` / `<id>-post.json` in DIR (mine these from
    /// fix commits that touch an ASL definition). A bug is "caught" when a code
    /// fires on the pre-fix version and is gone (or reduced) on the post-fix one.
    EvalPairs {
        dir: PathBuf,
        #[arg(long)]
        infer: bool,
    },
    /// Inject one defect of a given class into a workflow (error injection).
    Mutate {
        path: PathBuf,
        #[arg(long, value_enum)]
        kind: mutate::MutationKind,
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Write the mutated ASL here (default: stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Measure how close the data-flow fixpoint stays to its finite-height round
    /// bound `|states|*(|keys|+2)+2` across one or more corpora (evidence for the
    /// termination argument). Each workflow is analyzed in both the native
    /// (Top-seeded) and typed (closed-record-seeded) tiers; the report gives
    /// per-corpus and combined round/bound utilisation and the non-converged count.
    FixpointStats {
        /// One or more directories of workflow files, e.g. `corpus/asl corpus/cncf`.
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
    },
    /// Measure how many of a corpus's JSONPath field-reads fall inside the modeled
    /// dotted fragment (`$`, `$.a`, `$.a.b`) vs. the conservative fallbacks
    /// (bracket/wildcard/filter/function paths, the `$$` context object, and
    /// `States.*` intrinsics). Quantifies how much real ASL the precise data-flow
    /// analysis resolves exactly, and how much it soundly treats as `Maybe`.
    PathCoverage {
        /// One or more directories of workflow files, e.g. `corpus/asl corpus/cncf`.
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
    },
    /// Emit the built-in typed-DSL example workflow to ASL JSON.
    Demo {
        /// Which example: `order` (valid) or `order-bad` (reordered).
        #[arg(default_value = "order")]
        which: String,
        /// Emit the annotation sidecar (TOML) instead of the ASL.
        #[arg(long)]
        sidecar: bool,
    },
    /// Round-trip a workflow through the ASL emitter *unchanged*. Used as the
    /// control in the multi-validator baseline: a `mutate` defect is injected on
    /// top of this same emitted form, so comparing a validator's verdict on the
    /// emitted control vs. the emitted mutant isolates exactly the injected fault
    /// (free of any emitter-lossiness confound).
    Emit {
        path: PathBuf,
        /// Write the emitted ASL here (default: stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Check { path, annot, infer, json, deny_warnings, result_shapes } => {
            cmd_check(&path, annot.as_deref(), infer, json, deny_warnings, result_shapes)
        }
        Cmd::Infer { path, json } => cmd_infer(&path, json),
        Cmd::Scan { dir, annot, infer, result_shapes } => {
            cmd_scan(&dir, annot.as_deref(), infer, result_shapes)
        }
        Cmd::Stats { dir, tex } => cmd_stats(&dir, tex),
        Cmd::Eval { dir, annot, infer, strict_input, result_shapes } => {
            cmd_eval(&dir, annot.as_deref(), infer, strict_input, result_shapes)
        }
        Cmd::Oracle { dir, annot, infer, result_shapes } => {
            cmd_oracle(&dir, annot.as_deref(), infer, result_shapes)
        }
        Cmd::DataflowCert { dir, annot, infer, result_shapes } => {
            cmd_dataflow_cert(&dir, annot.as_deref(), infer, result_shapes)
        }
        Cmd::EvalPairs { dir, infer } => cmd_eval_pairs(&dir, infer),
        Cmd::Mutate { path, kind, seed, out } => cmd_mutate(&path, kind, seed, out.as_deref()),
        Cmd::FixpointStats { dirs } => cmd_fixpoint_stats(&dirs),
        Cmd::PathCoverage { dirs } => cmd_path_coverage(&dirs),
        Cmd::Demo { which, sidecar } => cmd_demo(&which, sidecar),
        Cmd::Emit { path, out } => cmd_emit(&path, out.as_deref()),
    };
    match code {
        Ok(c) => std::process::exit(c),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}

/// Whether a file is a workflow definition we can load (ASL JSON or CNCF YAML).
fn is_workflow_file(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("json") | Some("yaml") | Some("yml")
    )
}

/// Load every workflow in a file. A CloudFormation / SAM template or a CDK synth
/// output template yields one workflow per `AWS::StepFunctions::StateMachine`
/// (keyed by logical id); a bare `.asl.json` or a CNCF Serverless Workflow yields
/// one. Frontend selection is content-first (CloudFormation is detected by its
/// `AWS::StepFunctions::StateMachine` resource) then by extension.
fn load_all(path: &Path) -> Result<Vec<(String, ir::Workflow)>> {
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("workflow");
    if cfn::looks_like_template(&src) {
        return cfn::extract(&src, name, path.parent());
    }
    let wf = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") | Some("yml") => cncf::parse_str(&src, name)?,
        _ => asl::parse_str(&src, name)?,
    };
    Ok(vec![(name.to_string(), wf)])
}

fn attach_linked_children_to_loaded(machines: &mut [(String, ir::Workflow)]) {
    if machines.len() <= 1 {
        return;
    }
    let registry: BTreeMap<String, ir::Workflow> = machines
        .iter()
        .flat_map(|(label, wf)| {
            [
                (label.clone(), wf.clone()),
                (
                    format!("arn:aws:states:us-east-1:000000000000:stateMachine:{label}"),
                    wf.clone(),
                ),
                (format!("${{{label}}}"), wf.clone()),
                (format!("${{{label}.Arn}}"), wf.clone()),
            ]
        })
        .collect();
    let registry = Rc::new(registry);
    for (_, wf) in machines.iter_mut() {
        attach_registry_recursive(wf, registry.clone());
    }
}

fn attach_registry_recursive(wf: &mut ir::Workflow, registry: Rc<BTreeMap<String, ir::Workflow>>) {
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

/// Load a single workflow (the first state machine for a multi-machine template).
fn load(path: &Path) -> Result<ir::Workflow> {
    let mut machines = load_all(path)?;
    attach_linked_children_to_loaded(&mut machines);
    machines
        .into_iter()
        .next()
        .map(|(_, wf)| wf)
        .ok_or_else(|| anyhow::anyhow!("no workflow found in {}", path.display()))
}

fn load_resolved(path: &Path, annot: Option<&Path>, infer: bool) -> Result<(ir::Workflow, diag::DiagnosticSink)> {
    let mut wf = load(path)?;
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    let mut sink = diag::DiagnosticSink::new();
    annot::resolve(&mut wf, sc.as_ref(), infer, &mut sink);
    Ok((wf, sink))
}

fn run_pipeline_into(wf: &ir::Workflow, sink: &mut diag::DiagnosticSink, result_shapes: bool) {
    let pipeline = if result_shapes {
        passes::pipeline_with_result_shapes()
    } else {
        passes::default_pipeline()
    };
    passes::run_pipeline(&pipeline, wf, sink);
}

fn cmd_check(
    path: &Path,
    annot: Option<&Path>,
    infer: bool,
    json: bool,
    deny_warnings: bool,
    result_shapes: bool,
) -> Result<i32> {
    let mut machines = load_all(path)?;
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    attach_linked_children_to_loaded(&mut machines);
    let multi = machines.len() > 1;
    let mut worst = 0;
    let mut json_out = Vec::new();
    for (label, mut wf) in machines {
        let mut sink = diag::DiagnosticSink::new();
        annot::resolve(&mut wf, sc.as_ref(), infer, &mut sink);
        run_pipeline_into(&wf, &mut sink, result_shapes);
        if json {
            json_out.push(json!({ "machine": label, "result": sink }));
        } else {
            if multi {
                println!("# state machine: {label}");
            }
            print!("{}", sink.render_human(&wf.name));
        }
        if sink.errors() > 0 || (deny_warnings && sink.warnings() > 0) {
            worst = 1;
        }
    }
    if json {
        if multi {
            println!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if let Some(one) = json_out.into_iter().next() {
            println!("{}", serde_json::to_string_pretty(&one["result"])?);
        }
    }
    Ok(worst)
}

fn cmd_infer(path: &Path, json: bool) -> Result<i32> {
    let (wf, _) = load_resolved(path, None, true)?;
    let mut rows = Vec::new();
    wf.walk_machines(&mut |m| {
        for (name, st) in &m.states {
            if st.is_task() {
                rows.push((name.clone(), st.anno.idempotent, st.anno.persistent, st.anno.effect.clone()));
            }
        }
    });
    if json {
        let arr: Vec<_> = rows
            .iter()
            .map(|(n, i, p, e)| json!({"state": n, "idempotent": i, "persistent": p, "effect": e}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!("# inferred annotations for {}", wf.name);
        for (name, i, p, e) in &rows {
            println!("{name:<32} idempotent={i:?} persistent={p:?} effect={e:?}");
        }
    }
    Ok(0)
}

fn cmd_scan(dir: &Path, annot: Option<&Path>, infer: bool, result_shapes: bool) -> Result<i32> {
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    let mut files = Vec::new();
    let mut code_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_err = 0usize;
    let mut total_warn = 0usize;
    let mut flagged = 0usize;

    for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || !is_workflow_file(p) {
            continue;
        }
        let mut machines = match load_all(p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        attach_linked_children_to_loaded(&mut machines);
        let Some((_, mut wf)) = machines.into_iter().next() else { continue };
        let mut sink = diag::DiagnosticSink::new();
        annot::resolve(&mut wf, sc.as_ref(), infer, &mut sink);
        run_pipeline_into(&wf, &mut sink, result_shapes);
        let mut codes: BTreeMap<String, usize> = BTreeMap::new();
        for d in &sink.diagnostics {
            *codes.entry(d.code.clone()).or_default() += 1;
            *code_totals.entry(d.code.clone()).or_default() += 1;
        }
        total_err += sink.errors();
        total_warn += sink.warnings();
        if !sink.diagnostics.is_empty() {
            flagged += 1;
        }
        files.push(json!({
            "file": p.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "errors": sink.errors(),
            "warnings": sink.warnings(),
            "codes": codes,
            "diagnostics": sink.diagnostics,
        }));
    }

    let report = json!({
        "files": files.len(),
        "flagged_files": flagged,
        "total_errors": total_err,
        "total_warnings": total_warn,
        "code_totals": code_totals,
        "per_file": files,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn cmd_mutate(path: &Path, kind: mutate::MutationKind, seed: u64, out: Option<&Path>) -> Result<i32> {
    let wf = load(path)?;
    match mutate::mutate(&wf, kind, seed) {
        Some(mw) => {
            let text = serde_json::to_string_pretty(&asl::emit(&mw))?;
            match out {
                Some(o) => std::fs::write(o, text)?,
                None => println!("{text}"),
            }
            Ok(0)
        }
        None => {
            eprintln!("no applicable site for mutation kind {kind:?} in {}", wf.name);
            Ok(3)
        }
    }
}

fn code_counts(sink: &diag::DiagnosticSink) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for d in &sink.diagnostics {
        *m.entry(d.code.clone()).or_default() += 1;
    }
    m
}

/// Mine the top-level fields a workflow references (`$.<field>...`), used to
/// seed the typed-tier data-flow experiment with a closed input record.
fn mine_top_level_fields(wf: &ir::Workflow) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut set = BTreeSet::new();
    wf.walk_machines(&mut |m| {
        for st in m.states.values() {
            let mut refs = Vec::new();
            if let Some(p) = &st.parameters {
                ir::collect_jsonpath_refs(p, &mut refs);
            }
            if let Some(is) = &st.item_selector {
                ir::collect_jsonpath_refs(is, &mut refs);
            }
            for c in &st.choices {
                ir::collect_jsonpath_refs(&c.condition, &mut refs);
            }
            if let Some(ip) = &st.items_path {
                refs.push(ip.clone());
            }
            for r in refs {
                if let Some(rest) = r.strip_prefix("$.") {
                    let seg = rest.split('.').next().unwrap_or("");
                    if !seg.is_empty()
                        && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    {
                        set.insert(seg.to_string());
                    }
                }
            }
        }
    });
    set.into_iter().collect()
}

/// The mutation study (E3) + in-the-wild baseline (E2) + timing (E5), in-process.
fn cmd_eval(
    dir: &Path,
    annot: Option<&Path>,
    infer: bool,
    strict_input: bool,
    result_shapes: bool,
) -> Result<i32> {
    use std::time::Instant;
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };

    let mut kinds: Vec<mutate::MutationKind> = mutate::MutationKind::all().to_vec();
    if strict_input {
        kinds.push(mutate::MutationKind::Dataflow);
    }
    let kinds = kinds; // freeze
    let mut applicable = BTreeMap::<String, usize>::new();
    let mut detected = BTreeMap::<String, usize>::new();
    // operator -> (code -> #mutants on which that code newly fired): the confusion
    // matrix. A near-diagonal matrix shows each check is specific to its defect
    // class rather than trigger-happy (defuses "100% recall is trivial").
    let mut confusion = BTreeMap::<String, BTreeMap<String, usize>>::new();
    let mut baseline_codes = BTreeMap::<String, usize>::new();
    let mut baseline_errors = 0usize;
    let mut baseline_warnings = 0usize;
    let mut flagged = 0usize;
    let mut files = 0usize;
    let mut total_states = 0usize;
    let mut times_ns: Vec<u128> = Vec::new();

    for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || !is_workflow_file(p) {
            continue;
        }
        let base = match load(p) {
            Ok(w) => w,
            Err(_) => continue,
        };
        files += 1;
        total_states += base.total_states();

        // typed-tier seed: a closed input record of the fields the workflow uses
        let strict_fields = if strict_input {
            let f = mine_top_level_fields(&base);
            if f.is_empty() { None } else { Some(f) }
        } else {
            None
        };

        // baseline (timed)
        let mut b = base.clone();
        if let Some(f) = &strict_fields {
            b.input_fields = Some(f.clone());
        }
        let mut bsink = diag::DiagnosticSink::new();
        annot::resolve(&mut b, sc.as_ref(), infer, &mut bsink);
        let t = Instant::now();
        run_pipeline_into(&b, &mut bsink, result_shapes);
        times_ns.push(t.elapsed().as_nanos());

        let bcounts = code_counts(&bsink);
        for (c, n) in &bcounts {
            *baseline_codes.entry(c.clone()).or_default() += n;
        }
        baseline_errors += bsink.errors();
        baseline_warnings += bsink.warnings();
        if !bsink.diagnostics.is_empty() {
            flagged += 1;
        }

        // mutation study
        for &kind in &kinds {
            let ec = kind.expected_code();
            if let Some(mut mw) = mutate::mutate(&base, kind, 0) {
                if let Some(f) = &strict_fields {
                    mw.input_fields = Some(f.clone());
                }
                let mut msink = diag::DiagnosticSink::new();
                annot::resolve(&mut mw, sc.as_ref(), infer, &mut msink);
                run_pipeline_into(&mw, &mut msink, result_shapes);
                let mcounts = code_counts(&msink);
                let before = bcounts.get(ec).copied().unwrap_or(0);
                let after = mcounts.get(ec).copied().unwrap_or(0);
                *applicable.entry(format!("{kind:?}")).or_default() += 1;
                if after > before {
                    *detected.entry(format!("{kind:?}")).or_default() += 1;
                }
                // record every code that newly fired on this mutant (confusion row)
                let row = confusion.entry(format!("{kind:?}")).or_default();
                for (c, n) in &mcounts {
                    if *n > bcounts.get(c).copied().unwrap_or(0) {
                        *row.entry(c.clone()).or_default() += 1;
                    }
                }
            }
        }
    }

    times_ns.sort_unstable();
    let n = times_ns.len().max(1);
    let total_ns: u128 = times_ns.iter().sum();
    let mean_us = total_ns as f64 / n as f64 / 1000.0;
    let median_us = times_ns.get(n / 2).copied().unwrap_or(0) as f64 / 1000.0;
    let max_us = times_ns.last().copied().unwrap_or(0) as f64 / 1000.0;

    let mut per_kind = serde_json::Map::new();
    for &kind in &kinds {
        let key = format!("{kind:?}");
        let app = applicable.get(&key).copied().unwrap_or(0);
        let det = detected.get(&key).copied().unwrap_or(0);
        per_kind.insert(
            key,
            json!({
                "expected_code": kind.expected_code(),
                "applicable": app,
                "detected": det,
                "recall": if app > 0 { det as f64 / app as f64 } else { 0.0 },
            }),
        );
    }

    let report = json!({
        "corpus": { "files": files, "total_states": total_states },
        "ablation": { "result_shapes": result_shapes },
        "baseline": {
            "flagged_files": flagged,
            "errors": baseline_errors,
            "warnings": baseline_warnings,
            "code_totals": baseline_codes,
        },
        "mutation_study": per_kind,
        "confusion_matrix": confusion,
        "timing_us": { "mean": mean_us, "median": median_us, "max": max_us,
                       "total_ms": total_ns as f64 / 1.0e6 },
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

/// Diagnostic-code counts for a single workflow file (inference optional).
fn codes_for(p: &Path, infer: bool) -> BTreeMap<String, usize> {
    match load(p) {
        Ok(mut w) => {
            let mut sink = diag::DiagnosticSink::new();
            annot::resolve(&mut w, None, infer, &mut sink);
            run_pipeline_into(&w, &mut sink, false);
            code_counts(&sink)
        }
        Err(_) => BTreeMap::new(),
    }
}

/// Real-bug benchmark over (pre-fix, post-fix) pairs. For each `<id>-pre.json` with
/// a matching `<id>-post.json`, report the codes on each and the codes the fix
/// removed (fired on pre, gone/reduced on post) --- detection of an independently
/// introduced-and-fixed defect, free of the mutation study's construct bias.
fn cmd_eval_pairs(dir: &Path, infer: bool) -> Result<i32> {
    let mut pairs: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
        if let Some(id) = name.strip_suffix("-pre.json") {
            let post = p.with_file_name(format!("{id}-post.json"));
            if post.exists() {
                pairs.push((id.to_string(), p.to_path_buf(), post));
            }
        }
    }
    let mut caught = 0usize;
    let mut results = Vec::new();
    for (id, pre, post) in &pairs {
        let cp = codes_for(pre, infer);
        let cq = codes_for(post, infer);
        let fixed: Vec<String> = cp
            .iter()
            .filter(|(c, n)| cq.get(*c).copied().unwrap_or(0) < **n)
            .map(|(c, _)| c.clone())
            .collect();
        if !fixed.is_empty() {
            caught += 1;
        }
        results.push(json!({ "id": id, "codes_pre": cp, "codes_post": cq, "fixed_codes": fixed }));
    }
    let report = json!({
        "pairs": pairs.len(),
        "caught": caught,
        "recall": if pairs.is_empty() { 0.0 } else { caught as f64 / pairs.len() as f64 },
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

/// Extract the read path `r` from an `SC1101` message (`… reads 'r', a field …`).
fn extract_read_path(msg: &str) -> Option<String> {
    let start = msg.find("reads '")? + "reads '".len();
    let rest = &msg[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Execution-oracle cross-check: witness Theorem 1 at corpus scale. For each
/// workflow we seed the typed tier and inject a data-flow miss so `SC1101` fires,
/// then confirm with an independent bounded-path interpreter that every flagged
/// field is absent on the explored reaching paths. A `Present` verdict would be
/// a counterexample.
fn cmd_oracle(dir: &Path, annot: Option<&Path>, infer: bool, result_shapes: bool) -> Result<i32> {
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    let (mut files, mut findings, mut confirmed, mut present, mut unver, mut nested) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

    for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || !is_workflow_file(p) {
            continue;
        }
        let base = match load(p) {
            Ok(w) => w,
            Err(_) => continue,
        };
        files += 1;
        // typed-tier seed + injected miss so SC1101 actually fires in the wild
        let fields = mine_top_level_fields(&base);
        let Some(mut mw) = mutate::mutate(&base, mutate::MutationKind::Dataflow, 0) else { continue };
        if !fields.is_empty() {
            mw.input_fields = Some(fields);
        }
        let mut sink = diag::DiagnosticSink::new();
        annot::resolve(&mut mw, sc.as_ref(), infer, &mut sink);
        run_pipeline_into(&mw, &mut sink, result_shapes);
        for d in sink.diagnostics.iter().filter(|d| d.code == "SC1101") {
            findings += 1;
            if d.state.contains('/') {
                nested += 1; // oracle handles top-level machines only
                unver += 1;
                continue;
            }
            let Some(pref) = extract_read_path(&d.message) else { unver += 1; continue };
            let verdict = if result_shapes {
                concrete::check_ref_with_result_shapes(&mw, &d.state, &pref)
            } else {
                concrete::check_ref(&mw, &d.state, &pref)
            };
            match verdict {
                concrete::Verdict::ConfirmedAbsent => confirmed += 1,
                concrete::Verdict::Present => present += 1,
                concrete::Verdict::Unverifiable => unver += 1,
            }
        }
    }

    let report = json!({
        "files": files,
        "ablation": { "result_shapes": result_shapes },
        "sc1101_findings": findings,
        "oracle": {
            "confirmed_absent": confirmed,
            "counterexamples_present": present,
            "unverifiable": unver,
            "nested_skipped": nested,
        },
        "soundness_witnessed": present == 0,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if present == 0 { 0 } else { 1 })
}

/// Certificate check for Theorem 1 at corpus scale. This uses the same typed
/// mutation population as `oracle`, but proves each emitted `SC1101` report from
/// a finite widened-fixpoint invariant: all normal/catch transfer obligations
/// must be post-fixed, and the reported read must be definitely absent in the
/// certified invariant.
fn cmd_dataflow_cert(dir: &Path, annot: Option<&Path>, infer: bool, result_shapes: bool) -> Result<i32> {
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    let (mut files, mut findings, mut mismatches) = (0usize, 0usize, 0usize);
    let mut total = passes::dataflow::CertificateStats::default();

    for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || !is_workflow_file(p) {
            continue;
        }
        let base = match load(p) {
            Ok(w) => w,
            Err(_) => continue,
        };
        files += 1;
        let fields = mine_top_level_fields(&base);
        let Some(mut mw) = mutate::mutate(&base, mutate::MutationKind::Dataflow, 0) else { continue };
        if !fields.is_empty() {
            mw.input_fields = Some(fields);
        }
        let mut sink = diag::DiagnosticSink::new();
        annot::resolve(&mut mw, sc.as_ref(), infer, &mut sink);
        run_pipeline_into(&mw, &mut sink, result_shapes);
        let sc1101 = sink.diagnostics.iter().filter(|d| d.code == "SC1101").count();
        findings += sc1101;
        let cert = passes::dataflow::certify_sc1101(&mw, result_shapes);
        if cert.sc1101_reports != sc1101 {
            mismatches += 1;
        }
        total.machines += cert.machines;
        total.postconditions += cert.postconditions;
        total.failed_postconditions += cert.failed_postconditions;
        total.sc1101_reports += cert.sc1101_reports;
        total.certified_reports += cert.certified_reports;
    }

    let ok = total.ok() && mismatches == 0;
    let report = json!({
        "files": files,
        "ablation": { "result_shapes": result_shapes },
        "sc1101_findings": findings,
        "certificate": {
            "machines": total.machines,
            "postcondition_obligations": total.postconditions,
            "failed_postcondition_obligations": total.failed_postconditions,
            "sc1101_reports_in_certificate": total.sc1101_reports,
            "certified_sc1101_reports": total.certified_reports,
            "diagnostic_count_mismatches": mismatches,
            "ok": ok,
        },
        "loop_argument": "finite widened-fixpoint postcondition check; no bounded unrolling",
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if ok { 0 } else { 1 })
}

/// Round-trip a workflow through the ASL emitter unchanged (the control form for
/// the multi-validator baseline). Loads via the extension-chosen frontend and
/// re-emits ASL JSON.
fn cmd_emit(path: &Path, out: Option<&Path>) -> Result<i32> {
    let wf = load(path)?;
    let text = serde_json::to_string_pretty(&asl::emit(&wf))?;
    match out {
        Some(o) => std::fs::write(o, text)?,
        None => println!("{text}"),
    }
    Ok(0)
}

/// Summarise per-machine fixpoint telemetry into a JSON object: machine count,
/// how many failed to converge within the bound, the deepest iteration observed,
/// and the worst-case round/bound utilisation.
fn fixpoint_summary(
    dir: Option<String>,
    files: usize,
    rec: &[passes::dataflow::FixpointRound],
) -> serde_json::Value {
    let machines = rec.len();
    let non_converged = rec.iter().filter(|r| !r.converged).count();
    let max_rounds = rec.iter().map(|r| r.rounds).max().unwrap_or(0);
    let max_bound = rec.iter().map(|r| r.bound).max().unwrap_or(0);
    // worst-case round/bound utilisation (how close any machine came to the cap)
    let mut worst = (0f64, 0usize, 0usize, 0usize);
    for r in rec {
        let ratio = r.rounds as f64 / r.bound.max(1) as f64;
        if ratio > worst.0 {
            worst = (ratio, r.rounds, r.bound, r.states);
        }
    }
    let mut obj = serde_json::Map::new();
    if let Some(d) = dir {
        obj.insert("dir".into(), json!(d));
    }
    obj.insert("files".into(), json!(files));
    obj.insert("machines".into(), json!(machines));
    obj.insert("non_converged".into(), json!(non_converged));
    obj.insert("max_rounds".into(), json!(max_rounds));
    obj.insert("max_bound".into(), json!(max_bound));
    obj.insert(
        "worst_ratio".into(),
        json!({ "ratio": worst.0, "rounds": worst.1, "bound": worst.2, "states_in_machine": worst.3 }),
    );
    serde_json::Value::Object(obj)
}

/// Fixpoint-utilisation study: run the data-flow fixpoint over each corpus in
/// both tiers and report how far it stays below its termination bound.
fn cmd_fixpoint_stats(dirs: &[PathBuf]) -> Result<i32> {
    let mut per_corpus = Vec::new();
    let mut all: Vec<passes::dataflow::FixpointRound> = Vec::new();
    let mut all_files = 0usize;
    for dir in dirs {
        passes::dataflow::fixpoint_record_start();
        let mut files = 0usize;
        for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if !p.is_file() || !is_workflow_file(p) {
                continue;
            }
            let base = match load(p) {
                Ok(w) => w,
                Err(_) => continue,
            };
            files += 1;
            // native tier: Top-seeded start document
            let mut n = base.clone();
            let mut nsink = diag::DiagnosticSink::new();
            annot::resolve(&mut n, None, false, &mut nsink);
            run_pipeline_into(&n, &mut nsink, false);
            // typed tier: closed record of the top-level fields the workflow reads
            let mut t = base.clone();
            let f = mine_top_level_fields(&base);
            if !f.is_empty() {
                t.input_fields = Some(f);
            }
            let mut tsink = diag::DiagnosticSink::new();
            annot::resolve(&mut t, None, false, &mut tsink);
            run_pipeline_into(&t, &mut tsink, false);
        }
        let rec = passes::dataflow::fixpoint_record_take();
        per_corpus.push(fixpoint_summary(Some(dir.display().to_string()), files, &rec));
        all_files += files;
        all.extend(rec);
    }
    let report = json!({
        "corpora": per_corpus,
        "combined": fixpoint_summary(None, all_files, &all),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn cmd_path_coverage(dirs: &[PathBuf]) -> Result<i32> {
    use passes::dataflow::{path_coverage, PathCoverage};
    let mut per_corpus = Vec::new();
    let mut combined = PathCoverage::default();
    let mut all_files = 0usize;
    for dir in dirs {
        let mut cov = PathCoverage::default();
        let mut files = 0usize;
        for entry in WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if !p.is_file() || !is_workflow_file(p) {
                continue;
            }
            let wf = match load(p) {
                Ok(w) => w,
                Err(_) => continue,
            };
            files += 1;
            path_coverage(&wf, &mut cov);
        }
        per_corpus.push(path_coverage_summary(Some(dir.display().to_string()), files, &cov));
        all_files += files;
        combined.total += cov.total;
        combined.dotted += cov.dotted;
        combined.whole_doc += cov.whole_doc;
        combined.complex += cov.complex;
        combined.context += cov.context;
        combined.variable += cov.variable;
        combined.intrinsic += cov.intrinsic;
    }
    let report = json!({
        "corpora": per_corpus,
        "combined": path_coverage_summary(None, all_files, &combined),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn path_coverage_summary(
    dir: Option<String>,
    files: usize,
    cov: &passes::dataflow::PathCoverage,
) -> serde_json::Value {
    let pct = |n: usize| if cov.total == 0 { 0.0 } else { (n as f64) * 100.0 / (cov.total as f64) };
    json!({
        "dir": dir,
        "files": files,
        "reads_total": cov.total,
        "dotted": cov.dotted,
        "whole_doc": cov.whole_doc,
        "complex": cov.complex,
        "context": cov.context,
        "variable": cov.variable,
        "intrinsic": cov.intrinsic,
        "precise": cov.precise(),
        "precise_pct": (pct(cov.precise()) * 10.0).round() / 10.0,
        "conservative_pct": (pct(cov.complex + cov.context + cov.variable + cov.intrinsic) * 10.0).round() / 10.0,
    })
}

fn cmd_demo(which: &str, sidecar: bool) -> Result<i32> {
    let bad = which.ends_with("bad");
    let (wf, sc) = if which.starts_with("travel") {
        dsl::travel_example(bad)
    } else {
        dsl::order_example(bad)
    };
    if sidecar {
        print!("{}", toml::to_string(&sc)?);
    } else {
        let value = asl::emit(&wf);
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(0)
}

#[derive(Default)]
struct Stats {
    files: usize,
    parse_failures: usize,
    total_states: usize,
    type_counts: BTreeMap<String, usize>,
    feature_files: BTreeMap<String, usize>,
    service_task_counts: BTreeMap<String, usize>,
    service_file_counts: BTreeMap<String, usize>,
    with_retry: usize,
    with_catch: usize,
    jsonata_states: usize,
    jsonata_files: usize,
    callback_tasks: usize,
    callback_files: usize,
    unbounded_callback_tasks: usize,
    unbounded_callback_files: usize,
    distributed_map_states: usize,
    distributed_map_files: usize,
    sizes: Vec<usize>,
}

fn is_callback_or_activity(st: &ir::State) -> bool {
    if !st.is_task() {
        return false;
    }
    st.resource
        .as_deref()
        .map(|r| {
            let r = r.to_lowercase();
            r.contains(".waitfortasktoken") || r.contains(":activity:")
        })
        .unwrap_or(false)
}

fn is_unbounded_callback_or_activity(st: &ir::State) -> bool {
    is_callback_or_activity(st) && st.heartbeat_seconds.is_none() && st.timeout_seconds.is_none()
}

fn service_target(st: &ir::State) -> Option<&'static str> {
    if !st.is_task() {
        return None;
    }
    let r = st.resource.as_deref()?.to_lowercase();
    for (needle, service) in [
        (":states:startexecution", "sfn:startExecution"),
        ("events:putevents", "events:putEvents"),
        ("ecs:runtask", "ecs:runTask"),
        ("bedrock:invokemodel", "bedrock:invokeModel"),
        ("dynamodb:", "dynamodb"),
        ("s3:", "s3"),
        ("sqs:", "sqs"),
        ("sns:", "sns"),
        ("lambda:", "lambda"),
    ] {
        if r.contains(needle) {
            return Some(service);
        }
    }
    None
}

fn cmd_stats(dir: &Path, tex: bool) -> Result<i32> {
    let mut st = Stats::default();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || !is_workflow_file(p) {
            continue;
        }
        st.files += 1;
        let wf = match load(p) {
            Ok(w) => w,
            Err(_) => {
                st.parse_failures += 1;
                continue;
            }
        };
        st.total_states += wf.total_states();
        st.sizes.push(wf.total_states());

        let mut local_types: BTreeMap<String, usize> = BTreeMap::new();
        let mut local_services: BTreeMap<String, usize> = BTreeMap::new();
        let mut has_retry = false;
        let mut has_catch = false;
        let mut has_jsonata = false;
        let mut has_callback = false;
        let mut has_unbounded_callback = false;
        let mut has_distributed_map = false;
        wf.walk_machines(&mut |m| {
            for s in m.states.values() {
                *st.type_counts.entry(s.kind.as_str().to_string()).or_default() += 1;
                *local_types.entry(s.kind.as_str().to_string()).or_default() += 1;
                if matches!(s.query_language, ir::QueryLang::JsonAta) {
                    st.jsonata_states += 1;
                    has_jsonata = true;
                }
                if s.distributed_map {
                    st.distributed_map_states += 1;
                    has_distributed_map = true;
                }
                if is_callback_or_activity(s) {
                    st.callback_tasks += 1;
                    has_callback = true;
                }
                if is_unbounded_callback_or_activity(s) {
                    st.unbounded_callback_tasks += 1;
                    has_unbounded_callback = true;
                }
                if let Some(service) = service_target(s) {
                    *st.service_task_counts.entry(service.to_string()).or_default() += 1;
                    *local_services.entry(service.to_string()).or_default() += 1;
                }
                if s.has_retry() {
                    has_retry = true;
                }
                if !s.catch.is_empty() {
                    has_catch = true;
                }
            }
        });
        for k in local_types.keys() {
            *st.feature_files.entry(k.clone()).or_default() += 1;
        }
        for k in local_services.keys() {
            *st.service_file_counts.entry(k.clone()).or_default() += 1;
        }
        if has_retry {
            st.with_retry += 1;
        }
        if has_catch {
            st.with_catch += 1;
        }
        if has_jsonata {
            st.jsonata_files += 1;
        }
        if has_callback {
            st.callback_files += 1;
        }
        if has_unbounded_callback {
            st.unbounded_callback_files += 1;
        }
        if has_distributed_map {
            st.distributed_map_files += 1;
        }
    }

    let ok = st.files - st.parse_failures;
    st.sizes.sort_unstable();
    let median = st.sizes.get(st.sizes.len() / 2).copied().unwrap_or(0);
    let mean = if ok > 0 { st.total_states as f64 / ok as f64 } else { 0.0 };
    let max = st.sizes.last().copied().unwrap_or(0);
    let min = st.sizes.first().copied().unwrap_or(0);

    if tex {
        print_stats_tex(&st, ok, mean, median, min, max);
    } else {
        println!("workflows parsed : {ok}/{} ({} parse failures)", st.files, st.parse_failures);
        println!("total states     : {}", st.total_states);
        println!("states/workflow  : min {min}, median {median}, mean {mean:.1}, max {max}");
        println!("state-type counts:");
        for (k, v) in &st.type_counts {
            println!("    {k:<10} {v}");
        }
        println!("workflows using each feature:");
        for (k, v) in &st.feature_files {
            println!("    {k:<10} {v}");
        }
        println!("with >=1 Retry   : {}", st.with_retry);
        println!("with >=1 Catch   : {}", st.with_catch);
        println!("JSONata workflows: {} ({} states)", st.jsonata_files, st.jsonata_states);
        println!("callback tasks   : {} tasks in {} workflows", st.callback_tasks, st.callback_files);
        println!("unbounded callback: {} tasks in {} workflows", st.unbounded_callback_tasks, st.unbounded_callback_files);
        println!("Distributed Map  : {} workflows ({} states)", st.distributed_map_files, st.distributed_map_states);
        println!("service targets:");
        for (k, v) in &st.service_task_counts {
            let files = st.service_file_counts.get(k).copied().unwrap_or(0);
            println!("    {k:<20} {v} tasks in {files} workflows");
        }
    }
    Ok(0)
}

fn print_stats_tex(st: &Stats, ok: usize, mean: f64, median: usize, min: usize, max: usize) {
    println!("% generated by `stepcheck stats --tex`");
    println!("\\begin{{tabular}}{{lr}}");
    println!("\\toprule");
    println!("Property & Value \\\\\n\\midrule");
    println!("Workflows & {ok} \\\\");
    println!("Total states & {} \\\\", st.total_states);
    println!("States per workflow (min/median/mean/max) & {min} / {median} / {mean:.1} / {max} \\\\");
    for k in ["Task", "Choice", "Parallel", "Map", "Wait", "Pass", "Succeed", "Fail"] {
        if let Some(v) = st.type_counts.get(k) {
            println!("{k} states & {v} \\\\");
        }
    }
    println!("Workflows with retry policies & {} \\\\", st.with_retry);
    println!("Workflows with catch handlers & {} \\\\", st.with_catch);
    println!(r#"JSONata workflows / states & {} / {} \\"#, st.jsonata_files, st.jsonata_states);
    println!(r#"Callback or Activity tasks & {} \\"#, st.callback_tasks);
    println!(r#"Unbounded callback or Activity tasks & {} \\"#, st.unbounded_callback_tasks);
    println!(r#"Distributed Map workflows / states & {} / {} \\"#, st.distributed_map_files, st.distributed_map_states);
    println!("\\bottomrule");
    println!("\\end{{tabular}}");
}
