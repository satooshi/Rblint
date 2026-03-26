use crate::diagnostic::Diagnostic;
use crate::rules::{LintContext, Rule};

pub struct NilComparisonRule;

impl Rule for NilComparisonRule {
    fn name(&self) -> &'static str {
        "R061"
    }

    fn check(&self, _ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        Vec::new()
    }
}
