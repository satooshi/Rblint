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

/// Returns true if the character position `col` (0-based byte offset) in `line`
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
        // Warning only (no auto-fix). Skip semicolons inside string literals.
        for tok in tokens {
            if tok.kind == TokenKind::Semicolon {
                // tok.col is 1-based character column; convert to 0-based byte offset
                let line_idx = tok.line.saturating_sub(1);
                let line = ctx.lines.get(line_idx).copied().unwrap_or("");
                // Convert 1-based char column to byte offset for is_inside_string
                let col_char = tok.col.saturating_sub(1);
                let col_byte = line
                    .char_indices()
                    .nth(col_char)
                    .map(|(b, _)| b)
                    .unwrap_or(line.len());
                if is_inside_string(line, col_byte) {
                    continue;
                }
                // No auto-fix for R020: splitting at semicolons requires multi-line
                // fix support which the fixer does not yet implement.
                diags.push(Diagnostic::new(
                    ctx.file,
                    tok.line,
                    tok.col,
                    "R020",
                    "Avoid using semicolons to separate statements; use a newline instead",
                    Severity::Warning,
                ));
            }
        }

        // R021: Space around operators (==, !=, <, >, etc.)
        // Fix: insert missing spaces around the operator on the whole line.
        // Exclude ** (exponent) — the lexer emits two Star tokens, so we skip
        // both stars when they appear consecutively.
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
                // Exclude `**` (exponent): Star preceded by another Star (possibly with whitespace).
                // Skip *both* stars so neither triggers a spacing diagnostic.
                if tok.kind == TokenKind::Star {
                    // Check if this star is the second of a `**` pair
                    let prev_real = tokens[..i]
                        .iter()
                        .rev()
                        .find(|t| t.kind != TokenKind::Whitespace);
                    if prev_real.is_some_and(|t| t.kind == TokenKind::Star) {
                        i += 1;
                        continue;
                    }
                    // Check if this star is the first of a `**` pair
                    let next_real = tokens[i + 1..]
                        .iter()
                        .find(|t| t.kind != TokenKind::Whitespace);
                    if next_real.is_some_and(|t| t.kind == TokenKind::Star) {
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
            // Preserve the file's existing line ending style.
            // Detect CRLF by finding the first \n in the source and checking if it's
            // preceded by \r. This avoids false-positives from literal \r\n in strings
            // (which would not appear at a raw line boundary).
            let line_ending = {
                let bytes = ctx.source.as_bytes();
                let has_crlf = bytes
                    .iter()
                    .position(|&b| b == b'\n')
                    .is_some_and(|pos| pos > 0 && bytes[pos - 1] == b'\r');
                if has_crlf {
                    "\r\n"
                } else {
                    "\n"
                }
            };
            let fixed = format!("{}{}", last_line_content, line_ending);
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
        // Track a stack of block-opening keywords so we know whether an `end` closes a `def`.
        // Only fire when a method-closing `end` is immediately followed by `def` with no
        // blank line in between.
        {
            // Build a set of lines that have a method-closing `end` by scanning tokens
            // with a simple keyword stack (def/class/module/do/if/unless/while/until/for/begin/case).
            let mut block_stack: Vec<TokenKind> = Vec::new();
            let mut method_end_lines = std::collections::HashSet::new();
            for tok in tokens {
                match tok.kind {
                    TokenKind::Def
                    | TokenKind::Class
                    | TokenKind::Module
                    | TokenKind::Do
                    | TokenKind::If
                    | TokenKind::Unless
                    | TokenKind::While
                    | TokenKind::Until
                    | TokenKind::For
                    | TokenKind::Begin
                    | TokenKind::Case => {
                        // Only push if the keyword starts a block. For if/unless/while/until,
                        // modifier forms (e.g. `x = 1 if cond`) don't have a matching `end`,
                        // so only push when the keyword is the first non-whitespace token on
                        // the line.
                        let is_modifier_capable = matches!(
                            tok.kind,
                            TokenKind::If | TokenKind::Unless | TokenKind::While | TokenKind::Until
                        );
                        if is_modifier_capable {
                            let line_idx = tok.line.saturating_sub(1);
                            let line = ctx.lines.get(line_idx).copied().unwrap_or("");
                            let first_non_ws = line.trim_start();
                            // Check if this keyword is at the start of the line
                            if !first_non_ws.starts_with(tok.text.as_str()) {
                                // Modifier form — skip pushing
                                continue;
                            }
                        }
                        block_stack.push(tok.kind.clone());
                    }
                    TokenKind::End => {
                        if let Some(opener) = block_stack.pop() {
                            if opener == TokenKind::Def {
                                method_end_lines.insert(tok.line);
                            }
                        }
                    }
                    _ => {}
                }
            }

            for i in 0..ctx.lines.len().saturating_sub(1) {
                let line_no = i + 1; // 1-based
                if !method_end_lines.contains(&line_no) {
                    continue;
                }
                let current = ctx.lines[i];
                let next = ctx.lines[i + 1];
                let current_trimmed = current.trim();
                let next_trimmed = next.trim();
                if (current_trimmed == "end"
                    || current_trimmed.starts_with("end ")
                    || current_trimmed.starts_with("end#"))
                    && (next_trimmed.starts_with("def ") || next_trimmed == "def")
                {
                    let end_indent = leading_whitespace(current);
                    let def_indent = leading_whitespace(next);
                    if end_indent != def_indent {
                        continue;
                    }
                    diags.push(
                        Diagnostic::new(
                            ctx.file,
                            i + 2,
                            1,
                            "R026",
                            "Missing blank line between method definitions",
                            Severity::Warning,
                        )
                        // Empty string = blank line (the fixer inserts it as a new line
                        // in the lines vector, producing a blank line in the output).
                        .with_insert_before_fix(String::new()),
                    );
                }
            }
        }

        diags
    }
}

/// Heuristic: add missing spaces around binary operators in a line of Ruby code.
/// This handles simple cases like `x==y` → `x == y`.
///
/// Uses a single left-to-right pass, matching the longest operator first at each
/// position. This avoids the quadratic cost of repeated `is_inside_string` calls
/// and prevents multi-pass corruption of operators.
fn fix_operator_spacing(line: &str) -> String {
    // Operators to fix, ordered longest-first to avoid partial matches.
    let ops: &[&str] = &[
        "<=>", "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/",
    ];

    // Multi-character operators that should NOT be touched by the fixer.
    // If we see one of these at position i, skip past it without adding spaces.
    // This prevents corrupting `=>`, `**`, `<<`, `>>`, `+=`, `-=`, `*=`, `/=`.
    let skip_ops: &[&str] = &[
        "**", "=>", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=",
    ];

    let mut out = String::with_capacity(line.len() + 8);
    let bytes = line.as_bytes();
    let mut i = 0; // byte index
    let mut in_string = false;
    let mut quote_char = b'"';
    let mut escaped = false;

    while i < bytes.len() {
        let b = bytes[i];

        // Helper: decode one UTF-8 char at position i and push it to out
        let push_char = |out: &mut String, pos: usize| -> usize {
            let ch = &line[pos..];
            if let Some(c) = ch.chars().next() {
                out.push(c);
                c.len_utf8()
            } else {
                1
            }
        };

        // Track string state
        if escaped {
            escaped = false;
            i += push_char(&mut out, i);
            continue;
        }
        if in_string {
            if b == b'\\' {
                escaped = true;
            } else if b == quote_char {
                in_string = false;
            }
            i += push_char(&mut out, i);
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_string = true;
            quote_char = b;
            out.push(b as char);
            i += 1;
            continue;
        }

        // First, check if we're at a multi-char operator that should be skipped entirely
        // (e.g. =>, **, <<, >>). If so, copy it verbatim and advance past it.
        let mut skipped = false;
        for skip in skip_ops {
            let sb = skip.as_bytes();
            if i + sb.len() <= bytes.len() && &bytes[i..i + sb.len()] == sb {
                out.push_str(skip);
                i += sb.len();
                skipped = true;
                break;
            }
        }
        if skipped {
            continue;
        }

        // Try to match the longest operator at position i
        let mut matched_op: Option<&str> = None;
        for op in ops {
            let op_bytes = op.as_bytes();
            if i + op_bytes.len() <= bytes.len() && &bytes[i..i + op_bytes.len()] == op_bytes {
                matched_op = Some(op);
                break; // ops are longest-first, so first match is longest
            }
        }

        if let Some(op) = matched_op {
            let op_len = op.len();
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
            // Non-ASCII safe: decode one UTF-8 character
            i += push_char(&mut out, i);
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
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
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
    fn no_fix_for_semicolon() {
        // R020 does not provide auto-fix (multi-line fix not yet supported)
        let diags = check("x = 1; y = 2");
        assert!(fix_for_rule(&diags, "R020").is_none());
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
        assert_eq!(fix, "x == y");
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
        // The fix field is set (`"<delete line>"` marker for DeleteLine)
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
    fn no_violation_r026_after_non_method_end() {
        // `end` closing an `if` block should not trigger R026
        let source = "if true\n  1\nend\ndef bar\n  2\nend\n";
        let diags = check(source);
        assert!(!has_rule(&diags, "R026"), "{diags:?}");
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
