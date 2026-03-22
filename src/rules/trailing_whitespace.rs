use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};

/// R002 - No trailing whitespace
pub struct TrailingWhitespaceRule;

impl Rule for TrailingWhitespaceRule {
    fn name(&self) -> &'static str {
        "R002"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for (i, line) in ctx.lines.iter().enumerate() {
            if line.ends_with(' ') || line.ends_with('\t') {
                let trailing = line.len() - line.trim_end().len();
                diags.push(
                    Diagnostic::new(
                        ctx.file,
                        i + 1,
                        line.trim_end().len() + 1,
                        "R002",
                        format!(
                            "Trailing whitespace ({} character{})",
                            trailing,
                            if trailing == 1 { "" } else { "s" }
                        ),
                        Severity::Warning,
                    )
                    .with_fix(line.trim_end().to_string()),
                );
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
        TrailingWhitespaceRule.check(&ctx)
    }

    #[test]
    fn no_violation_clean_line() {
        assert!(check("x = 1").is_empty());
    }

    #[test]
    fn no_violation_empty_line() {
        assert!(check("").is_empty());
    }

    #[test]
    fn violation_single_trailing_space() {
        let diags = check("x = 1 ");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R002");
        assert_eq!(diags[0].line, 1);
    }

    #[test]
    fn violation_multiple_trailing_spaces() {
        let diags = check("x = 1   ");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("3 characters"));
    }

    #[test]
    fn violation_single_trailing_tab() {
        let diags = check("x = 1\t");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R002");
    }

    #[test]
    fn singular_message_for_one_char() {
        let diags = check("x ");
        assert!(diags[0].message.contains("1 character"));
        assert!(!diags[0].message.contains("characters"));
    }

    #[test]
    fn fix_trims_trailing_whitespace() {
        let diags = check("hello   ");
        assert_eq!(diags[0].fix.as_deref(), Some("hello"));
    }

    #[test]
    fn spaces_only_line_is_violation() {
        let diags = check("   ");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].fix.as_deref(), Some(""));
    }

    #[test]
    fn only_flags_lines_with_trailing_ws() {
        let source = "clean\ntrailing  \nclean again";
        let diags = check(source);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 2);
    }
}
