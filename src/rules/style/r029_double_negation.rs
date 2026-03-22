use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;
use crate::rules::{LintContext, Rule};

/// R029 - Double negation `!!x` (use explicit boolean conversion instead)
pub struct DoubleNegationRule;

impl Rule for DoubleNegationRule {
    fn name(&self) -> &'static str {
        "R029"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        for i in 0..tokens.len().saturating_sub(1) {
            if tokens[i].kind == TokenKind::Bang && tokens[i + 1].kind == TokenKind::Bang {
                diags.push(Diagnostic::new(
                    ctx.file,
                    tokens[i].line,
                    tokens[i].col,
                    "R029",
                    "Avoid double negation `!!` — use explicit boolean conversion (e.g. `!value.nil?` or `.present?`)",
                    Severity::Warning,
                ));
            }
        }

        diags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn check(source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        DoubleNegationRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    #[test]
    fn violation_double_negation() {
        assert!(has_rule(&check("result = !!value"), "R029"));
    }

    #[test]
    fn no_violation_single_negation() {
        assert!(!has_rule(&check("result = !value"), "R029"));
    }

    #[test]
    fn no_violation_normal_not_eq() {
        assert!(!has_rule(&check("x != y"), "R029"));
    }
}
