use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;
use crate::rules::{LintContext, Rule};

/// R028 - Prefer `unless` over `if !condition`
pub struct NegatedIfRule;

impl Rule for NegatedIfRule {
    fn name(&self) -> &'static str {
        "R028"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        let mut i = 0;
        while i < tokens.len() {
            // Look for: `if` at statement start followed by `!` or `not`
            if tokens[i].kind != TokenKind::If {
                i += 1;
                continue;
            }

            // Check it's a block-form `if` (at statement start)
            let prev_non_ws = (0..i)
                .rev()
                .find(|&j| tokens[j].kind != TokenKind::Whitespace)
                .map(|j| &tokens[j]);
            let at_statement_start = match prev_non_ws {
                None => true,
                Some(p) => matches!(p.kind, TokenKind::Newline),
            };
            if !at_statement_start {
                i += 1;
                continue;
            }

            let if_line = tokens[i].line;
            let if_col = tokens[i].col;

            // Skip whitespace after `if`
            let mut j = i + 1;
            while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                j += 1;
            }

            // Check for `!` (Bang token) immediately after `if`
            if j < tokens.len() && tokens[j].kind == TokenKind::Bang {
                // Look ahead: the next token should be a simple identifier (not another `!`)
                let k = j + 1;
                if k < tokens.len() && tokens[k].kind == TokenKind::Bang {
                    // `if !!x` — double negation, different rule
                    i += 1;
                    continue;
                }

                // Build the fix: replace `if !<rest>` with `unless <rest>`
                let line_text = ctx
                    .lines
                    .get(if_line.saturating_sub(1))
                    .copied()
                    .unwrap_or("");
                let fix = build_unless_fix(line_text);

                diags.push(
                    Diagnostic::new(
                        ctx.file,
                        if_line,
                        if_col,
                        "R028",
                        "Use `unless` instead of `if !condition`",
                        Severity::Warning,
                    )
                    .with_fix(fix),
                );
            }

            i += 1;
        }

        diags
    }
}

/// Replace `if !<expr>` with `unless <expr>` in a line.
fn build_unless_fix(line: &str) -> String {
    // Find `if !` in the line and replace with `unless `
    // Handle indentation
    if let Some(pos) = line.find("if !") {
        let before = &line[..pos];
        let after = &line[pos + 4..]; // skip "if !"
                                      // `after` is the expression after `!`
        format!("{before}unless {after}")
    } else {
        line.to_string()
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
        NegatedIfRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    #[test]
    fn violation_if_bang() {
        let src = "if !ready\n  do_work\nend\n";
        assert!(has_rule(&check(src), "R028"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_unless() {
        let src = "unless ready\n  do_work\nend\n";
        assert!(!has_rule(&check(src), "R028"));
    }

    #[test]
    fn no_violation_modifier_if_bang() {
        // Modifier form: `do_work if !ready` — not at statement start
        let src = "do_work if !ready\n";
        assert!(!has_rule(&check(src), "R028"));
    }

    #[test]
    fn no_violation_if_without_bang() {
        let src = "if ready\n  do_work\nend\n";
        assert!(!has_rule(&check(src), "R028"));
    }

    #[test]
    fn fix_replaces_if_bang_with_unless() {
        let src = "if !ready\n  do_work\nend\n";
        let diags = check(src);
        let d = diags.iter().find(|d| d.rule == "R028").unwrap();
        let fix = d.fix.as_deref().unwrap_or("");
        assert!(fix.contains("unless ready"), "fix: {fix}");
    }
}
