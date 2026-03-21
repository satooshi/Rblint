use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
        }
    }
}

/// How a fix should be applied to the source file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub enum FixKind {
    /// Replace the content of the diagnostic's line with the fix string
    #[default]
    ReplaceLine,
    /// Insert the fix string as a new line *before* the diagnostic's line
    InsertBefore,
}

impl FixKind {
    fn is_default(&self) -> bool {
        matches!(self, FixKind::ReplaceLine)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub rule: &'static str,
    pub message: String,
    pub severity: Severity,
    /// Optional auto-fix suggestion text
    pub fix: Option<String>,
    /// How to apply the fix (defaults to ReplaceLine)
    #[serde(skip_serializing_if = "FixKind::is_default")]
    pub fix_kind: FixKind,
}

impl Diagnostic {
    pub fn new(
        file: impl Into<String>,
        line: usize,
        col: usize,
        rule: &'static str,
        message: impl Into<String>,
        severity: Severity,
    ) -> Self {
        Diagnostic {
            file: file.into(),
            line,
            col,
            rule,
            message: message.into(),
            severity,
            fix: None,
            fix_kind: FixKind::ReplaceLine,
        }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn with_insert_before_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self.fix_kind = FixKind::InsertBefore;
        self
    }
}
