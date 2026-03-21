use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;

/// Style rules: spacing, semicolons, parentheses, etc.
pub struct StyleRule;

/// Returns the leading whitespace of a line (spaces/tabs before first non-ws char).
fn leading_whitespace(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

/// Returns true if the character position `col` (0-based byte index) in `line`
/// is inside a string literal.
fn is_inside_string(line: &str, col: usize) -> bool {
    let mut in_string = false;
    let mut quote_char = '"';
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if i >= col {
            break;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escaped = true;
            } else if c == quote_char {
                in_string = false;
            }
        } else if c == '"' || c == '\'' {
            in_string = true;
            quote_char = c;
        }
    }
    in_string
}

impl Rule for StyleRule {
    fn name(&self) -> &'static str {
        "R020"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // R020: No semicolons at end of line (use newline instead)
        // Fix: split `x = 1; y = 2` into two lines with proper indentation.
        // Skip semicolons inside string literals.
        for tok in tokens {
            if tok.kind == TokenKind::Semicolon {
                // tok.col is 1-based; convert to 0-based for is_inside_string
                let line_idx = tok.line.saturating_sub(1);
                let line = ctx.lines.get(line_idx).copied().unwrap_or("");
                let col0 = tok.col.saturating_sub(1);
                if is_inside_string(line, col0) {
                    continue;
                }
                let mut diag = Diagnostic::new(
                    ctx.file,
                    tok.line,
                    tok.col,
                    "R020",
                    "Avoid using semicolons to separate statements; use a newline instead",
                    Severity::Warning,
                );
                // Build fix: split on first semicolon, indent second statement
                let indent = leading_whitespace(line);
                // Find the semicolon in the raw line (col is 1-based)
                if let Some(semi_pos) = line.find(';') {
                    let before = line[..semi_pos].trim_end();
                    let after = line[semi_pos + 1..].trim_start();
                    if !after.is_empty() {
                        let fixed = format!("{}\n{}{}", before, indent, after);
                        diag = diag.with_fix(fixed);
                    }
                }
                diags.push(diag);
            }
        }

        // R021: Space around operators (=, ==, !=, <, >, etc.)
        // Fix: insert missing spaces around the operator on the whole line.
        // Exclude ** (exponent) — the lexer emits two Star tokens, so we skip
        // Star when the previous non-ws real token is also Star.
        // Exclude default argument `=` (inside method def parameter list).
        let mut i = 1;
        while i < tokens.len() {
            let tok = &tokens[i];

            let is_binary_op = matches!(
                tok.kind,
                TokenKind::EqEq
                    | TokenKind::NotEq
                    | TokenKind::Lt
                    | TokenKind::Gt
                    | TokenKind::LtEq
                    | TokenKind::GtEq
                    | TokenKind::And2
                    | TokenKind::Or2
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
            );

            if is_binary_op {
                // Exclude `**` (exponent): Star preceded by another Star (possibly with whitespace)
                if tok.kind == TokenKind::Star {
                    let prev_real = tokens[..i]
                        .iter()
                        .rev()
                        .find(|t| t.kind != TokenKind::Whitespace);
                    if prev_real.is_some_and(|t| t.kind == TokenKind::Star) {
                        i += 1;
                        continue;
                    }
                }

                let prev = &tokens[i - 1];
                let next = tokens.get(i + 1);

                let missing_before =
                    prev.kind != TokenKind::Whitespace && prev.kind != TokenKind::Newline;
                let missing_after = next.is_some_and(|n| {
                    n.kind != TokenKind::Whitespace && n.kind != TokenKind::Newline
                });

                if missing_before || missing_after {
                    let line_idx = tok.line.saturating_sub(1);
                    let line = ctx.lines.get(line_idx).copied().unwrap_or("");
                    let fixed = fix_operator_spacing(line);

                    if missing_before {
                        diags.push(
                            Diagnostic::new(
                                ctx.file,
                                tok.line,
                                tok.col,
                                "R021",
                                format!("Missing space before `{}`", tok.text),
                                Severity::Warning,
                            )
                            .with_fix(fixed.clone()),
                        );
                    }

                    if missing_after {
                        diags.push(
                            Diagnostic::new(
                                ctx.file,
                                tok.line,
                                tok.col + tok.text.len(),
                                "R021",
                                format!("Missing space after `{}`", tok.text),
                                Severity::Warning,
                            )
                            .with_fix(fixed),
                        );
                    }
                }
            }

            // R022: Trailing comma in method definition parameters
            if tok.kind == TokenKind::Comma {
                if let Some(next) = tokens.get(i + 1) {
                    let real_next = if next.kind == TokenKind::Whitespace {
                        tokens.get(i + 2)
                    } else {
                        Some(next)
                    };
                    if let Some(rn) = real_next {
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
            }

            i += 1;
        }

        // R023: Multiple empty lines (> 2 consecutive blank lines)
        // Fix: delete the excess blank line.
        let mut blank_count = 0usize;
        for (i, line) in ctx.lines.iter().enumerate() {
            if line.trim().is_empty() {
                blank_count += 1;
                if blank_count > 2 {
                    diags.push(
                        Diagnostic::new(
                            ctx.file,
                            i + 1,
                            1,
                            "R023",
                            "Too many consecutive blank lines (maximum 2)",
                            Severity::Warning,
                        )
                        .with_delete_line_fix(),
                    );
                }
            } else {
                blank_count = 0;
            }
        }

        // R024: `puts` with no args should use `puts` not `p nil`
        for (i, tok) in tokens.iter().enumerate() {
            if tok.kind == TokenKind::Ident && tok.text == "p" {
                if let Some(next) = tokens.get(i + 1) {
                    let is_nil = if next.kind == TokenKind::Whitespace {
                        tokens.get(i + 2).is_some_and(|t| t.kind == TokenKind::Nil)
                    } else {
                        next.kind == TokenKind::Nil
                    };
                    if is_nil {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R024",
                            "Use `puts` instead of `p nil` to print a blank line",
                            Severity::Info,
                        ));
                    }
                }
            }
        }

        // R025: Missing final newline
        if !ctx.source.is_empty() && !ctx.source.ends_with('\n') {
            let last_line = ctx.lines.len();
            let last_line_content = ctx.lines.last().copied().unwrap_or("").to_string();
            let fixed = format!("{}\n", last_line_content);
            diags.push(
                Diagnostic::new(
                    ctx.file,
                    last_line,
                    1,
                    "R025",
                    "Missing final newline at end of file",
                    Severity::Warning,
                )
                .with_fix(fixed),
            );
        }

        // R026: Missing blank line between method definitions
        // When `end` of one method is immediately followed by `def` of the next method
        // (with no blank line between), warn and insert a blank line.
        // We look at line-level patterns: an "end" line followed directly by a "def" line.
        for i in 0..ctx.lines.len().saturating_sub(1) {
            let current = ctx.lines[i].trim();
            let next = ctx.lines[i + 1].trim();
            if current == "end"
                && (next.starts_with("def ") || next == "def")
            {
                // Warn on the `def` line (i+2 is 1-based)
                diags.push(
                    Diagnostic::new(
                        ctx.file,
                        i + 2,
                        1,
                        "R026",
                        "Missing blank line between method definitions",
                        Severity::Warning,
                    )
                    .with_insert_before_fix(String::new()),
                );
            }
        }

        diags
    }
}

/// Heuristic: add missing spaces around binary operators in a line of Ruby code.
/// This handles simple cases like `x==y` → `x == y`.
fn fix_operator_spacing(line: &str) -> String {
    // Operators to fix, ordered longest-first to avoid partial matches.
    // `<=>` must come before `<=` and `>=`; `==` before `=`, etc.
    let ops: &[&str] = &[
        "<=>", "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/",
    ];

    let mut result = line.to_string();
    for op in ops {
        result = fix_single_op(&result, op);
    }
    result
}

/// Ensure spaces around `op` in `line`, skipping inside string literals.
fn fix_single_op(line: &str, op: &str) -> String {
    let op_len = op.len();
    let mut out = String::with_capacity(line.len() + 4);
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Check if we're inside a string at position i (crude but functional)
        if is_inside_string(line, i) {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if i + op_len <= bytes.len() && &line[i..i + op_len] == op {
            // Don't double-space: check what's before/after
            let before = if i > 0 { bytes[i - 1] } else { 0 };
            let after = if i + op_len < bytes.len() {
                bytes[i + op_len]
            } else {
                0
            };

            let need_space_before = before != b' ' && before != b'\t' && before != 0;
            let need_space_after = after != b' ' && after != b'\t' && after != 0 && after != b'\n';

            if need_space_before {
                out.push(' ');
            }
            out.push_str(op);
            if need_space_after {
                out.push(' ');
            }
            i += op_len;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::FixKind;
    use crate::lexer::Lexer;

    fn check(source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext {
            file: "test.rb",
            source,
            lines: &lines,
            tokens: &tokens,
        };
        StyleRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    fn count_rule(diags: &[Diagnostic], rule: &str) -> usize {
        diags.iter().filter(|d| d.rule == rule).count()
    }

    fn fix_for_rule<'a>(diags: &'a [Diagnostic], rule: &str) -> Option<&'a str> {
        diags
            .iter()
            .find(|d| d.rule == rule && d.fix.is_some())
            .and_then(|d| d.fix.as_deref())
    }

    // --- R020: semicolons ---

    #[test]
    fn violation_semicolon_between_statements() {
        let diags = check("x = 1; y = 2");
        assert!(has_rule(&diags, "R020"));
    }

    #[test]
    fn no_violation_no_semicolons() {
        let diags = check("x = 1\ny = 2");
        assert!(!has_rule(&diags, "R020"));
    }

    #[test]
    fn fix_semicolon_splits_line() {
        let diags = check("x = 1; y = 2");
        let fix = fix_for_rule(&diags, "R020").expect("should have fix");
        assert_eq!(fix, "x = 1\ny = 2");
    }

    #[test]
    fn fix_semicolon_preserves_indentation() {
        let diags = check("  x = 1; y = 2");
        let fix = fix_for_rule(&diags, "R020").expect("should have fix");
        assert_eq!(fix, "  x = 1\n  y = 2");
    }

    #[test]
    fn no_violation_semicolon_in_string() {
        // Semicolons inside string literals should not trigger R020
        let diags = check(r#"x = "a; b""#);
        assert!(!has_rule(&diags, "R020"));
    }

    // --- R021: operator spacing ---

    #[test]
    fn no_violation_spaced_operator() {
        let diags = check("x == y");
        assert!(!has_rule(&diags, "R021"));
    }

    #[test]
    fn violation_no_space_before_eq_eq() {
        let diags = check("x== y");
        assert!(has_rule(&diags, "R021"));
    }

    #[test]
    fn violation_no_space_after_eq_eq() {
        let diags = check("x ==y");
        assert!(has_rule(&diags, "R021"));
    }

    #[test]
    fn violation_no_space_around_plus() {
        let diags = check("a+b");
        assert_eq!(count_rule(&diags, "R021"), 2); // before and after
    }

    #[test]
    fn no_violation_newline_before_operator() {
        // Operator at start of continuation line is fine
        let diags = check("x\n== y");
        assert!(!has_rule(&diags, "R021"));
    }

    #[test]
    fn fix_operator_spacing_eq_eq() {
        let diags = check("x==y");
        let fix = fix_for_rule(&diags, "R021").expect("should have fix");
        assert!(fix.contains("x == y") || fix.contains("x==y "), "fix: {fix}");
    }

    // --- R022: trailing comma ---

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

    // --- R023: blank lines ---

    #[test]
    fn no_violation_two_blank_lines() {
        let diags = check("a = 1\n\n\nb = 2");
        assert!(!has_rule(&diags, "R023"));
    }

    #[test]
    fn violation_three_blank_lines() {
        let diags = check("a = 1\n\n\n\nb = 2");
        assert!(has_rule(&diags, "R023"), "{diags:?}");
    }

    #[test]
    fn fix_r023_deletes_excess_blank_line() {
        let diags = check("a = 1\n\n\n\nb = 2");
        let diag = diags.iter().find(|d| d.rule == "R023").expect("R023 diag");
        assert_eq!(diag.fix_kind, FixKind::DeleteLine);
        // The fix field is set (empty string used as marker for DeleteLine)
        assert!(diag.fix.is_some());
    }

    #[test]
    fn fix_r023_applied_removes_line() {
        use crate::fixer::apply_fixes;
        let source = "a = 1\n\n\n\nb = 2\n";
        let diags = check(source);
        let (fixed, count) = apply_fixes(source, &diags);
        assert!(count > 0);
        // After fix: at most 2 consecutive blank lines
        let blank_runs: Vec<usize> = {
            let mut runs = Vec::new();
            let mut run = 0usize;
            for line in fixed.lines() {
                if line.trim().is_empty() {
                    run += 1;
                } else {
                    runs.extend((run > 0).then_some(run));
                    run = 0;
                }
            }
            runs
        };
        assert!(blank_runs.iter().all(|&r| r <= 2));
    }

    // --- R024: p nil ---

    #[test]
    fn violation_p_nil() {
        let diags = check("p nil");
        assert!(has_rule(&diags, "R024"), "{diags:?}");
    }

    #[test]
    fn no_violation_p_with_value() {
        let diags = check("p some_object");
        assert!(!has_rule(&diags, "R024"));
    }

    #[test]
    fn no_violation_puts() {
        let diags = check("puts");
        assert!(!has_rule(&diags, "R024"));
    }

    // --- R025: missing final newline ---

    #[test]
    fn violation_missing_final_newline() {
        let diags = check("x = 1");
        assert!(has_rule(&diags, "R025"), "{diags:?}");
    }

    #[test]
    fn no_violation_has_final_newline() {
        let diags = check("x = 1\n");
        assert!(!has_rule(&diags, "R025"));
    }

    #[test]
    fn no_violation_empty_source() {
        let diags = check("");
        assert!(!has_rule(&diags, "R025"));
    }

    #[test]
    fn fix_r025_appends_newline() {
        let diags = check("x = 1");
        let fix = fix_for_rule(&diags, "R025").expect("should have fix");
        assert!(fix.ends_with('\n'), "fix should end with newline: {fix:?}");
        assert!(fix.starts_with("x = 1"));
    }

    #[test]
    fn fix_r025_applied() {
        use crate::fixer::apply_fixes;
        let source = "x = 1";
        let diags = check(source);
        let (fixed, count) = apply_fixes(source, &diags);
        assert!(count > 0);
        assert!(fixed.ends_with('\n'));
    }

    // --- R026: missing blank line between methods ---

    #[test]
    fn violation_no_blank_line_between_methods() {
        let source = "def foo\n  1\nend\ndef bar\n  2\nend\n";
        let diags = check(source);
        assert!(has_rule(&diags, "R026"), "{diags:?}");
    }

    #[test]
    fn no_violation_blank_line_between_methods() {
        let source = "def foo\n  1\nend\n\ndef bar\n  2\nend\n";
        let diags = check(source);
        assert!(!has_rule(&diags, "R026"), "{diags:?}");
    }

    #[test]
    fn fix_r026_insert_before() {
        let source = "def foo\n  1\nend\ndef bar\n  2\nend\n";
        let diags = check(source);
        let diag = diags.iter().find(|d| d.rule == "R026").expect("R026 diag");
        assert_eq!(diag.fix_kind, FixKind::InsertBefore);
    }

    #[test]
    fn fix_r026_applied() {
        use crate::fixer::apply_fixes;
        let source = "def foo\n  1\nend\ndef bar\n  2\nend\n";
        let diags = check(source);
        let (fixed, count) = apply_fixes(source, &diags);
        assert!(count > 0);
        // There should now be a blank line between methods
        assert!(fixed.contains("end\n\ndef"), "fixed: {fixed:?}");
    }
}
