use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};

/// R001 - Lines should not exceed max_length characters
pub struct LineLengthRule {
    pub max_length: usize,
}

impl Default for LineLengthRule {
    fn default() -> Self {
        LineLengthRule { max_length: 120 }
    }
}

impl Rule for LineLengthRule {
    fn name(&self) -> &'static str {
        "R001"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for (i, line) in ctx.lines.iter().enumerate() {
            let len = line.chars().count();
            if len > self.max_length {
                diags.push(Diagnostic::new(
                    ctx.file,
                    i + 1,
                    self.max_length + 1,
                    "R001",
                    format!("Line too long ({} > {} characters)", len, self.max_length),
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
        LineLengthRule::default().check(&ctx)
    }

    fn check_with_max(source: &str, max: usize) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        LineLengthRule { max_length: max }.check(&ctx)
    }

    #[test]
    fn no_violation_short_line() {
        assert!(check("x = 1").is_empty());
    }

    #[test]
    fn no_violation_empty_line() {
        assert!(check("").is_empty());
        assert!(check("\n\n").is_empty());
    }

    #[test]
    fn no_violation_exactly_at_limit() {
        let line = "x".repeat(120);
        assert!(check(&line).is_empty());
    }

    #[test]
    fn violation_one_over_limit() {
        let line = "x".repeat(121);
        let diags = check(&line);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R001");
        assert_eq!(diags[0].line, 1);
    }

    #[test]
    fn violation_reports_correct_length() {
        let line = "x".repeat(130);
        let diags = check(&line);
        assert!(diags[0].message.contains("130"));
        assert!(diags[0].message.contains("120"));
    }

    #[test]
    fn violation_only_on_long_lines() {
        let source = format!("short\n{}\nshort", "x".repeat(121));
        let diags = check(&source);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 2);
    }

    #[test]
    fn unicode_chars_count_as_one() {
        // 120 emoji chars — each counts as 1 character
        let line = "🎉".repeat(120);
        assert!(check(&line).is_empty());

        let line = "🎉".repeat(121);
        let diags = check(&line);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn custom_max_length() {
        let line = "x".repeat(80);
        assert!(check_with_max(&line, 79).len() == 1);
        assert!(check_with_max(&line, 80).is_empty());
    }
}
