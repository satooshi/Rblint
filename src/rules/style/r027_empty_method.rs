use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;
use crate::rules::{LintContext, Rule};

/// R027 - Empty method body (`def foo; end`)
pub struct EmptyMethodRule;

impl Rule for EmptyMethodRule {
    fn name(&self) -> &'static str {
        "R027"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind != TokenKind::Def {
                i += 1;
                continue;
            }

            let def_line = tokens[i].line;
            let def_col = tokens[i].col;

            // Find the method name token (skip whitespace)
            let mut j = i + 1;
            while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                j += 1;
            }

            // Skip the method name
            if j >= tokens.len() {
                i += 1;
                continue;
            }
            let name_text = tokens[j].text.clone();
            j += 1;

            // Skip optional parameter list (everything until newline, skipping parens)
            // We need to find the end of the def line
            let mut paren_depth = 0usize;
            while j < tokens.len() {
                match tokens[j].kind {
                    TokenKind::LParen => {
                        paren_depth += 1;
                        j += 1;
                    }
                    TokenKind::RParen => {
                        paren_depth = paren_depth.saturating_sub(1);
                        j += 1;
                    }
                    TokenKind::Newline if paren_depth == 0 => {
                        j += 1;
                        break;
                    }
                    _ => {
                        j += 1;
                    }
                }
            }

            // Now skip blank lines (newlines and whitespace only)
            let body_start = j;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Newline | TokenKind::Whitespace)
            {
                j += 1;
            }

            // Check if the very next non-blank token is `end`
            if j < tokens.len() && tokens[j].kind == TokenKind::End {
                let end_line = tokens[j].line;

                // Only report if the method spans multiple lines (def ... \n end)
                // Single-line defs (def foo; end) are already compact
                if end_line > def_line {
                    // Build the one-liner fix
                    // Find the def line content in ctx.lines
                    let def_line_text = ctx
                        .lines
                        .get(def_line.saturating_sub(1))
                        .copied()
                        .unwrap_or("");
                    let indent = {
                        let trimmed = def_line_text.trim_start();
                        &def_line_text[..def_line_text.len() - trimmed.len()]
                    };
                    let def_header = def_line_text.trim();
                    let fix_line = format!("{}{}; end", indent, def_header);

                    diags.push(
                        Diagnostic::new(
                            ctx.file,
                            def_line,
                            def_col,
                            "R027",
                            format!("Empty method body for `{name_text}` — use one-liner `{def_header}; end`"),
                            Severity::Info,
                        )
                        .with_fix(fix_line),
                    );

                    // Mark end line for deletion via body_start..j lines
                    // The fix replaces the def line; the blank lines and end line need deletion.
                    // We emit delete diagnostics for each extra line.
                    // body_start index covers blank lines between def and end.
                    // We'll emit DeleteLine for each line from (def_line+1) to end_line inclusive.
                    let _ = body_start; // used implicitly via line range
                    for del_line in (def_line + 1)..=end_line {
                        diags.push(
                            Diagnostic::new(
                                ctx.file,
                                del_line,
                                1,
                                "R027",
                                "Empty method body — extra line to remove",
                                Severity::Info,
                            )
                            .with_delete_line_fix(),
                        );
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
        EmptyMethodRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    #[test]
    fn violation_empty_method() {
        let src = "def foo\nend\n";
        assert!(has_rule(&check(src), "R027"), "{:?}", check(src));
    }

    #[test]
    fn violation_empty_method_with_blank_lines() {
        let src = "def foo\n\nend\n";
        assert!(has_rule(&check(src), "R027"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_method_with_body() {
        let src = "def foo\n  42\nend\n";
        assert!(!has_rule(&check(src), "R027"));
    }

    #[test]
    fn no_violation_already_one_liner() {
        // def foo; end on the same line — def_line == end_line, so no violation
        let src = "def foo; end\n";
        // The lexer should put def and end on the same line
        assert!(!has_rule(&check(src), "R027"));
    }

    #[test]
    fn fix_contains_one_liner() {
        let src = "def foo\nend\n";
        let diags = check(src);
        let r027 = diags.iter().find(|d| d.rule == "R027" && d.fix.is_some());
        assert!(r027.is_some(), "Expected fixable R027 diagnostic");
        let fix = r027.unwrap().fix.as_deref().unwrap_or("");
        assert!(fix.contains("def foo; end"), "fix: {fix}");
    }

    #[test]
    fn fix_preserves_parameters() {
        let src = "def foo(a, b)\nend\n";
        let diags = check(src);
        let r027 = diags.iter().find(|d| d.rule == "R027" && d.fix.is_some());
        assert!(r027.is_some(), "Expected fixable R027 diagnostic");
        let fix = r027.unwrap().fix.as_deref().unwrap_or("");
        assert!(fix.contains("def foo(a, b); end"), "fix: {fix}");
    }
}
