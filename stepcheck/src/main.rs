//! `stepcheck` — a static verifier for serverless workflows (Amazon States
//! Language). Frontends lower to a common IR; a pipeline of analysis passes
//! reads only that IR; an emitter lowers verified workflows back to ASL.

mod annot;
mod asl;
mod cncf;
mod diag;
mod dsl;
mod ir;
mod mutate;
mod passes;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "stepcheck", version, about = "Static verifier for serverless workflows (ASL)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verify a single workflow file.
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
    /// Emit the built-in typed-DSL example workflow to ASL JSON.
    Demo {
        /// Which example: `order` (valid) or `order-bad` (reordered).
        #[arg(default_value = "order")]
        which: String,
        /// Emit the annotation sidecar (TOML) instead of the ASL.
        #[arg(long)]
        sidecar: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Check { path, annot, infer, json, deny_warnings } => {
            cmd_check(&path, annot.as_deref(), infer, json, deny_warnings)
        }
        Cmd::Infer { path, json } => cmd_infer(&path, json),
        Cmd::Scan { dir, annot, infer } => cmd_scan(&dir, annot.as_deref(), infer),
        Cmd::Stats { dir, tex } => cmd_stats(&dir, tex),
        Cmd::Eval { dir, annot, infer } => cmd_eval(&dir, annot.as_deref(), infer),
        Cmd::Mutate { path, kind, seed, out } => cmd_mutate(&path, kind, seed, out.as_deref()),
        Cmd::Demo { which, sidecar } => cmd_demo(&which, sidecar),
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

/// Load a workflow, choosing the frontend by file extension: `.yaml`/`.yml` use
/// the CNCF Serverless Workflow frontend, everything else uses the ASL frontend.
fn load(path: &Path) -> Result<ir::Workflow> {
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("workflow");
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") | Some("yml") => cncf::parse_str(&src, name),
        _ => asl::parse_str(&src, name),
    }
}

fn load_resolved(path: &Path, annot: Option<&Path>, infer: bool) -> Result<ir::Workflow> {
    let mut wf = load(path)?;
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };
    annot::resolve(&mut wf, sc.as_ref(), infer);
    Ok(wf)
}

fn run_pipeline(wf: &ir::Workflow) -> diag::DiagnosticSink {
    let pipeline = passes::default_pipeline();
    let mut sink = diag::DiagnosticSink::new();
    passes::run_pipeline(&pipeline, wf, &mut sink);
    sink
}

fn cmd_check(
    path: &Path,
    annot: Option<&Path>,
    infer: bool,
    json: bool,
    deny_warnings: bool,
) -> Result<i32> {
    let wf = load_resolved(path, annot, infer)?;
    let sink = run_pipeline(&wf);
    if json {
        println!("{}", serde_json::to_string_pretty(&sink)?);
    } else {
        print!("{}", sink.render_human(&wf.name));
    }
    let bad = sink.errors() > 0 || (deny_warnings && sink.warnings() > 0);
    Ok(if bad { 1 } else { 0 })
}

fn cmd_infer(path: &Path, json: bool) -> Result<i32> {
    let wf = load_resolved(path, None, true)?;
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

fn cmd_scan(dir: &Path, annot: Option<&Path>, infer: bool) -> Result<i32> {
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
        let mut wf = match load(p) {
            Ok(w) => w,
            Err(_) => continue,
        };
        annot::resolve(&mut wf, sc.as_ref(), infer);
        let sink = run_pipeline(&wf);
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

/// The mutation study (E3) + in-the-wild baseline (E2) + timing (E5), in-process.
fn cmd_eval(dir: &Path, annot: Option<&Path>, infer: bool) -> Result<i32> {
    use std::time::Instant;
    let sc = match annot {
        Some(p) => Some(annot::Sidecar::load(p)?),
        None => None,
    };

    let kinds = mutate::MutationKind::all();
    let mut applicable = BTreeMap::<String, usize>::new();
    let mut detected = BTreeMap::<String, usize>::new();
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

        // baseline (timed)
        let mut b = base.clone();
        annot::resolve(&mut b, sc.as_ref(), infer);
        let t = Instant::now();
        let bsink = run_pipeline(&b);
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
        for kind in kinds {
            let ec = kind.expected_code();
            if let Some(mut mw) = mutate::mutate(&base, kind, 0) {
                annot::resolve(&mut mw, sc.as_ref(), infer);
                let msink = run_pipeline(&mw);
                let before = bcounts.get(ec).copied().unwrap_or(0);
                let after = code_counts(&msink).get(ec).copied().unwrap_or(0);
                *applicable.entry(format!("{kind:?}")).or_default() += 1;
                if after > before {
                    *detected.entry(format!("{kind:?}")).or_default() += 1;
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
    for kind in kinds {
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
        "baseline": {
            "flagged_files": flagged,
            "errors": baseline_errors,
            "warnings": baseline_warnings,
            "code_totals": baseline_codes,
        },
        "mutation_study": per_kind,
        "timing_us": { "mean": mean_us, "median": median_us, "max": max_us,
                       "total_ms": total_ns as f64 / 1.0e6 },
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn cmd_demo(which: &str, sidecar: bool) -> Result<i32> {
    let (wf, sc) = dsl::order_example(which.ends_with("bad"));
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
    with_retry: usize,
    with_catch: usize,
    sizes: Vec<usize>,
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
        let mut has_retry = false;
        let mut has_catch = false;
        wf.walk_machines(&mut |m| {
            for s in m.states.values() {
                *st.type_counts.entry(s.kind.as_str().to_string()).or_default() += 1;
                *local_types.entry(s.kind.as_str().to_string()).or_default() += 1;
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
        if has_retry {
            st.with_retry += 1;
        }
        if has_catch {
            st.with_catch += 1;
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
    println!("\\bottomrule");
    println!("\\end{{tabular}}");
}
