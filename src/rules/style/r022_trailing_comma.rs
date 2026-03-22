use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;
use crate::rules::{LintContext, Rule};

pub struct TrailingCommaRule;

impl Rule for TrailingCommaRule {
    fn name(&self) -> &'static str {
        "R022"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // R022: Trailing comma in method definition parameters
        let mut i = 1;
        while i < tokens.len() {
            let tok = &tokens[i];

            if tok.kind == TokenKind::Comma {
                // Skip all trivia (whitespace and newlines) to find the real next token.
                let mut j = i + 1;
                while let Some(t) = tokens.get(j) {
                    if t.kind == TokenKind::Whitespace || t.kind == TokenKind::Newline {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if let Some(rn) = tokens.get(j) {
                    if rn.kind == TokenKind::RParen {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R022",
                            "Avoid trailing comma before closing parenthesis",
                            Severity::Warning,
                        ));
                    }
                }
            }

            i += 1;
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
        TrailingCommaRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    #[test]
    fn violation_trailing_comma_before_rparen() {
        let diags = check("foo(a, b,)");
        assert!(has_rule(&diags, "R022"), "{diags:?}");
    }

    #[test]
    fn no_violation_no_trailing_comma() {
        let diags = check("foo(a, b)");
        assert!(!has_rule(&diags, "R022"));
    }

    #[test]
    fn violation_trailing_comma_before_rparen_multiline() {
        // foo(a, b,
        // )
        let diags = check("foo(a, b,\n)");
        assert!(has_rule(&diags, "R022"), "{diags:?}");
    }

    #[test]
    fn violation_trailing_comma_newline_indent_rparen() {
        // foo(a, b,
        //   )  ← indented closing paren
        let diags = check("foo(a, b,\n  )");
        assert!(has_rule(&diags, "R022"), "{diags:?}");
    }

    #[test]
    fn violation_trailing_comma_space_then_newline() {
        // foo(a, b, \n)  ← space before newline
        let diags = check("foo(a, b, \n)");
        assert!(has_rule(&diags, "R022"), "{diags:?}");
    }
}
