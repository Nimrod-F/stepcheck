//! Retry / timeout-budget (temporal) analysis (codes `SC6xxx`).
//!
//! Step Functions' resilience knobs (`Retry`, `TimeoutSeconds`,
//! `HeartbeatSeconds`, the machine-level `TimeoutSeconds`) interact in ways the
//! platform never cross-checks. We catch three temporal defects statically:
//!
//!   * **SC6001** — a `.waitForTaskToken` callback (or an Activity) with neither
//!     a `HeartbeatSeconds` nor a `TimeoutSeconds` waits up to one year for a
//!     token that may never arrive: an unbounded hang.
//!   * **SC6002** — a task whose worst-case retry budget (the geometric sum of
//!     its back-off delays) exceeds the declared machine `TimeoutSeconds`, so
//!     the execution times out before the retries it configures can complete.
//!   * **SC6003** — `HeartbeatSeconds >= TimeoutSeconds` on a task, which is a
//!     definite mis-configuration (the heartbeat can never fire before the task
//!     times out); the platform rejects it only at deploy/run time.

use super::retry::qualify;
use super::Pass;
use crate::diag::{Diagnostic, DiagnosticSink};
use crate::ir::{State, Workflow};

pub struct TemporalPass;

impl Pass for TemporalPass {
    fn id(&self) -> &'static str {
        "temporal"
    }
    fn title(&self) -> &'static str {
        "Retry / timeout-budget analysis"
    }
    fn run(&self, wf: &Workflow, sink: &mut DiagnosticSink) {
        run_machine(wf, "", wf.timeout_seconds, sink);
    }
}

/// Worst-case cumulative retry delay of a task: the geometric back-off sum
/// `Σ interval·backoff^k` over each retry rule (ASL defaults: interval 1 s,
/// backoff 2.0, 3 attempts).
fn retry_budget(st: &State) -> f64 {
    let mut total = 0.0;
    for r in &st.retry {
        let attempts = r.max_attempts.unwrap_or(3).max(0);
        let interval = r.interval_seconds.unwrap_or(1.0);
        let backoff = r.backoff_rate.unwrap_or(2.0).max(1.0);
        let mut d = interval;
        for _ in 0..attempts {
            total += d;
            d *= backoff;
        }
    }
    total
}

fn is_callback(st: &State) -> bool {
    st.resource
        .as_ref()
        .map(|r| r.contains(".waitForTaskToken") || r.contains(":activity:"))
        .unwrap_or(false)
}

fn run_machine(wf: &Workflow, scope: &str, machine_timeout: Option<f64>, sink: &mut DiagnosticSink) {
    for (name, st) in &wf.states {
        if st.is_task() {
            let qname = qualify(scope, name);

            // SC6001: unbounded callback / activity wait.
            if is_callback(st) && st.heartbeat_seconds.is_none() && st.timeout_seconds.is_none() {
                sink.push(
                    Diagnostic::warning(
                        "SC6001",
                        &qname,
                        format!(
                            "callback task '{name}' has no TimeoutSeconds or HeartbeatSeconds and may wait indefinitely"
                        ),
                    )
                    .with_note("a lost task token leaves the execution blocked for up to one year; set a TimeoutSeconds/HeartbeatSeconds"),
                );
            }

            // SC6003: heartbeat must be shorter than the task timeout.
            if let (Some(h), Some(t)) = (st.heartbeat_seconds, st.timeout_seconds) {
                if h >= t {
                    sink.push(
                        Diagnostic::error(
                            "SC6003",
                            &qname,
                            format!(
                                "HeartbeatSeconds ({h}) is not smaller than TimeoutSeconds ({t}) on task '{name}'"
                            ),
                        )
                        .with_note("the heartbeat can never fire before the task times out"),
                    );
                }
            }

            // SC6002: retry budget cannot complete within the machine timeout.
            if let Some(mt) = machine_timeout {
                let budget = retry_budget(st);
                if budget > mt {
                    sink.push(
                        Diagnostic::warning(
                            "SC6002",
                            &qname,
                            format!(
                                "retry budget of '{name}' (~{budget:.0}s of back-off) exceeds the state-machine TimeoutSeconds ({mt:.0}s)"
                            ),
                        )
                        .with_note("the execution times out before these retries can complete; lower MaxAttempts/BackoffRate or raise the timeout"),
                    );
                }
            }
        }
        recurse(st, scope, machine_timeout, sink);
    }
}

fn recurse(st: &State, scope: &str, machine_timeout: Option<f64>, sink: &mut DiagnosticSink) {
    if let Some(it) = &st.iterator {
        run_machine(it, &qualify(scope, &format!("{}[Map]", st.name)), machine_timeout, sink);
    }
    for (i, br) in st.branches.iter().enumerate() {
        run_machine(br, &qualify(scope, &format!("{}[Branch{i}]", st.name)), machine_timeout, sink);
    }
}
