//! The analysis pass framework.
//!
//! Every check implements [`Pass`] and reads only the [`Workflow`] IR, writing
//! findings to a [`DiagnosticSink`]. Adding a new analysis is adding a new
//! `Pass`; the rest of the tool is unchanged. This is the modular spine the
//! paper refers to as the verification pipeline.

use crate::diag::DiagnosticSink;
use crate::ir::Workflow;

pub mod compensation;
pub mod concurrency;
pub mod contract;
pub mod dataflow;
pub mod retry;
pub mod structural;
pub mod temporal;
pub mod typestate;

/// A single static analysis over the workflow IR.
pub trait Pass {
    /// Short stable identifier, e.g. `"structural"`.
    fn id(&self) -> &'static str;
    /// Human title for `--explain`/reports.
    fn title(&self) -> &'static str;
    /// Run the analysis, appending findings to `sink`.
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink);
}

/// The default verification pipeline, in execution order.
pub fn default_pipeline() -> Vec<Box<dyn Pass>> {
    pipeline(false)
}

/// Verification pipeline with the optional result-shape ablation enabled.
pub fn pipeline_with_result_shapes() -> Vec<Box<dyn Pass>> {
    pipeline(true)
}

fn pipeline(result_shapes: bool) -> Vec<Box<dyn Pass>> {
    vec![
        Box::new(structural::StructuralPass),
        Box::new(contract::ContractPass),
        Box::new(typestate::TypestatePass),
        if result_shapes {
            Box::new(dataflow::DataFlowPass::with_result_shapes())
        } else {
            Box::new(dataflow::DataFlowPass::new())
        },
        Box::new(concurrency::ConcurrencyPass),
        Box::new(temporal::TemporalPass),
        Box::new(retry::RetrySafetyPass),
        Box::new(compensation::CompensationPass),
    ]
}

/// Run a pipeline over a workflow.
pub fn run_pipeline(passes: &[Box<dyn Pass>], wf: &Workflow, sink: &mut DiagnosticSink) {
    for p in passes {
        p.run(wf, sink);
    }
}

/// Tasks (and `Map`/`Parallel` work states) reachable from `from` via normal
/// control flow, skipping *transparent* states (`Choice`/`Pass`/`Wait`) that do
/// not change the business contract. Shared by the contract and typestate passes.
pub(crate) fn forward_tasks(wf: &Workflow, from: &str) -> Vec<String> {
    use crate::ir::StateKind;
    use std::collections::HashSet;
    fn go(wf: &Workflow, from: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        let Some(st) = wf.states.get(from) else { return };
        for succ in st.normal_successors() {
            if !seen.insert(succ.to_string()) {
                continue;
            }
            let Some(s) = wf.states.get(succ) else { continue };
            match s.kind {
                StateKind::Task | StateKind::Map | StateKind::Parallel => out.push(succ.to_string()),
                StateKind::Choice | StateKind::Pass | StateKind::Wait => go(wf, succ, seen, out),
                _ => {}
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    go(wf, from, &mut seen, &mut out);
    out
}
