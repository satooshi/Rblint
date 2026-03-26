use crate::diagnostic::Diagnostic;
use crate::rules::{LintContext, Rule};

pub struct DeepNestingRule {
    pub(crate) max_nesting: usize,
}

impl Rule for DeepNestingRule {
    fn name(&self) -> &'static str {
        "R062"
    }

    fn check(&self, _ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let _ = self.max_nesting;
        Vec::new()
    }
}
