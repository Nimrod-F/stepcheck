//! Diagnostics: structured findings with a stable code, a severity and a
//! human- or machine-readable rendering.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    fn label(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// One finding produced by a [`crate::passes::Pass`].
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// Stable diagnostic code, e.g. `SC4001`.
    pub code: String,
    pub severity: Severity,
    /// State the finding is attached to (or the workflow, when empty).
    pub state: String,
    pub message: String,
    /// Optional remediation hint.
    pub note: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &str, state: &str, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, state, message)
    }
    pub fn warning(code: &str, state: &str, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, state, message)
    }
    pub fn info(code: &str, state: &str, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, state, message)
    }
    pub fn new(severity: Severity, code: &str, state: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            code: code.to_string(),
            severity,
            state: state.to_string(),
            message: message.into(),
            note: None,
        }
    }
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// Collector passed to every [`crate::passes::Pass`].
#[derive(Debug, Default, Serialize)]
pub struct DiagnosticSink {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }
    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }
    pub fn count_with_prefix(&self, prefix: &str) -> usize {
        self.diagnostics.iter().filter(|d| d.code.starts_with(prefix)).count()
    }
    pub fn has_code(&self, code: &str) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }

    /// Render for a terminal.
    pub fn render_human(&self, source_name: &str) -> String {
        if self.diagnostics.is_empty() {
            return format!("{source_name}: ok (no findings)\n");
        }
        let mut out = String::new();
        for d in &self.diagnostics {
            out.push_str(&format!(
                "{}[{}]: {}\n  --> {}: state \"{}\"\n",
                d.severity.label(),
                d.code,
                d.message,
                source_name,
                d.state
            ));
            if let Some(n) = &d.note {
                out.push_str(&format!("  = note: {n}\n"));
            }
        }
        out.push_str(&format!(
            "\n{}: {} error(s), {} warning(s)\n",
            source_name,
            self.errors(),
            self.warnings()
        ));
        out
    }
}
