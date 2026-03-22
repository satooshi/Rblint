use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{Token, TokenKind};

/// Returns the text of the first non-whitespace token after `start_idx`.
fn next_name(tokens: &[Token], start_idx: usize) -> String {
    tokens
        .iter()
        .skip(start_idx + 1)
        .find(|t| !matches!(t.kind, TokenKind::Whitespace))
        .map(|t| t.text.clone())
        .unwrap_or_else(|| "<unknown>".into())
}

/// Complexity rules
pub struct ComplexityRule {
    pub(crate) max_method_lines: usize,
    pub(crate) max_class_lines: usize,
    pub(crate) max_complexity: usize,
    pub(crate) max_parameters: usize,
}

impl Default for ComplexityRule {
    fn default() -> Self {
        let config = crate::config::Config::default();
        ComplexityRule {
            max_method_lines: config.max_method_lines,
            max_class_lines: config.max_class_lines,
            max_complexity: config.max_complexity,
            max_parameters: config.max_parameters,
        }
    }
}

impl Rule for ComplexityRule {
    fn name(&self) -> &'static str {
        "R040"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // R040: Method too long
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind == TokenKind::Def {
                let def_line = tokens[i].line;
                let name = next_name(tokens, i);

                let mut depth = 1usize;
                let mut j = i + 1;
                while j < tokens.len() && depth > 0 {
                    match tokens[j].kind {
                        TokenKind::Def
                        | TokenKind::Class
                        | TokenKind::Module
                        | TokenKind::Do
                        | TokenKind::Begin
                        | TokenKind::If
                        | TokenKind::Unless
                        | TokenKind::While
                        | TokenKind::Until
                        | TokenKind::For => depth += 1,
                        TokenKind::End => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }

                if j <= tokens.len() {
                    let end_line = tokens[j.saturating_sub(1)].line;
                    let method_lines = end_line.saturating_sub(def_line) + 1;
                    if method_lines > self.max_method_lines {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            def_line,
                            tokens[i].col,
                            "R040",
                            format!(
                                "Method `{}` is too long ({} lines, max {})",
                                name, method_lines, self.max_method_lines
                            ),
                            Severity::Warning,
                        ));
                    }
                }
            }
            i += 1;
        }

        // R041: Class too long
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind == TokenKind::Class {
                let class_line = tokens[i].line;
                let name = next_name(tokens, i);

                let mut depth = 1usize;
                let mut j = i + 1;
                while j < tokens.len() && depth > 0 {
                    match tokens[j].kind {
                        TokenKind::Def
                        | TokenKind::Class
                        | TokenKind::Module
                        | TokenKind::Do
                        | TokenKind::Begin => depth += 1,
                        TokenKind::End => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }

                if j <= tokens.len() {
                    let end_line = tokens[j.saturating_sub(1)].line;
                    let class_lines = end_line.saturating_sub(class_line) + 1;
                    if class_lines > self.max_class_lines {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            class_line,
                            tokens[i].col,
                            "R041",
                            format!(
                                "Class `{}` is too long ({} lines, max {})",
                                name, class_lines, self.max_class_lines
                            ),
                            Severity::Warning,
                        ));
                    }
                }
            }
            i += 1;
        }

        // R042: Cyclomatic complexity (count branching keywords)
        // Per method: count if/unless/elsif/while/until/for/rescue/when
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind == TokenKind::Def {
                let def_line = tokens[i].line;
                let name = next_name(tokens, i);
                let mut complexity = 1usize;
                let mut depth = 1usize;
                let mut j = i + 1;
                while j < tokens.len() && depth > 0 {
                    match tokens[j].kind {
                        // These all open a new block that needs a matching `end`
                        TokenKind::Def
                        | TokenKind::Class
                        | TokenKind::Module
                        | TokenKind::Do
                        | TokenKind::Begin
                        | TokenKind::If
                        | TokenKind::Unless
                        | TokenKind::While
                        | TokenKind::Until
                        | TokenKind::For => depth += 1,
                        TokenKind::End => depth -= 1,
                        // Count decision points (branching keywords)
                        TokenKind::Elsif | TokenKind::Rescue | TokenKind::When => {
                            complexity += 1;
                        }
                        TokenKind::And2 | TokenKind::Or2 => {
                            complexity += 1;
                        }
                        _ => {}
                    }
                    if matches!(
                        tokens[j].kind,
                        TokenKind::If
                            | TokenKind::Unless
                            | TokenKind::While
                            | TokenKind::Until
                            | TokenKind::For
                    ) {
                        complexity += 1;
                    }
                    j += 1;
                }

                if complexity > self.max_complexity {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        def_line,
                        tokens[i].col,
                        "R042",
                        format!(
                            "Method `{}` has high cyclomatic complexity ({}, max {})",
                            name, complexity, self.max_complexity
                        ),
                        Severity::Warning,
                    ));
                }
            }
            i += 1;
        }

        // R043: Too many method parameters
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind == TokenKind::Def {
                let def_line = tokens[i].line;
                let def_col = tokens[i].col;
                let name = next_name(tokens, i);

                // Find opening paren of parameter list
                let mut j = i + 1;
                // Skip whitespace
                while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                    j += 1;
                }
                // Skip the method name token (could be `self`, then `.`, then name)
                if j < tokens.len() {
                    j += 1;
                }
                // Handle singleton methods: `def self.foo` — skip `.` and next ident
                while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                    j += 1;
                }
                if j < tokens.len() && tokens[j].kind == TokenKind::Dot {
                    j += 1; // skip dot
                    while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                        j += 1;
                    }
                    // skip actual method name
                    if j < tokens.len() {
                        j += 1;
                    }
                }
                // Skip whitespace between name and ( or first param
                while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                    j += 1;
                }

                let param_count = if j < tokens.len() && tokens[j].kind == TokenKind::LParen {
                    // Paren form: count commas at top level only (paren_depth==1,
                    // bracket_depth==0, brace_depth==0) to avoid counting commas
                    // inside default-value arrays/hashes like `def foo(a = [1, 2], b)`.
                    j += 1; // skip (
                    let mut count = 0usize;
                    let mut paren_depth = 1usize;
                    let mut bracket_depth = 0usize;
                    let mut brace_depth = 0usize;
                    let mut found_param = false;

                    while j < tokens.len() && paren_depth > 0 {
                        match tokens[j].kind {
                            TokenKind::LParen => {
                                paren_depth += 1;
                                found_param = true;
                            }
                            TokenKind::RParen => {
                                paren_depth -= 1;
                                if paren_depth == 0 && found_param {
                                    count += 1;
                                }
                            }
                            TokenKind::LBracket => {
                                bracket_depth += 1;
                                found_param = true;
                            }
                            TokenKind::RBracket => {
                                bracket_depth = bracket_depth.saturating_sub(1);
                            }
                            TokenKind::LBrace => {
                                brace_depth += 1;
                                found_param = true;
                            }
                            TokenKind::RBrace => {
                                brace_depth = brace_depth.saturating_sub(1);
                            }
                            TokenKind::Comma
                                if paren_depth == 1 && bracket_depth == 0 && brace_depth == 0 =>
                            {
                                count += 1;
                                found_param = false;
                            }
                            TokenKind::Whitespace | TokenKind::Newline => {}
                            _ => {
                                found_param = true;
                            }
                        }
                        j += 1;
                    }
                    count
                } else if j < tokens.len()
                    && !matches!(
                        tokens[j].kind,
                        TokenKind::Newline | TokenKind::End | TokenKind::Semicolon
                    )
                {
                    // Paren-less form: count commas on the rest of the line + 1
                    let mut count = 1usize; // at least one param if we reach here
                    while j < tokens.len() && tokens[j].kind != TokenKind::Newline {
                        if tokens[j].kind == TokenKind::Comma {
                            count += 1;
                        }
                        j += 1;
                    }
                    count
                } else {
                    0
                };

                if param_count > self.max_parameters {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        def_line,
                        def_col,
                        "R043",
                        format!(
                            "Method `{}` has too many parameters ({}, max {})",
                            name, param_count, self.max_parameters
                        ),
                        Severity::Warning,
                    ));
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
        ComplexityRule::default().check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    fn make_method(body_lines: usize) -> String {
        let mut s = String::from("def foo\n");
        for i in 0..body_lines {
            s.push_str(&format!("  x = {}\n", i));
        }
        s.push_str("end\n");
        s
    }

    // --- R040: method length ---

    #[test]
    fn no_violation_short_method() {
        let src = make_method(5);
        assert!(!has_rule(&check(&src), "R040"));
    }

    #[test]
    fn no_violation_exactly_30_lines() {
        // def + 28 body lines + end = 30 lines
        let src = make_method(28);
        assert!(
            !has_rule(&check(&src), "R040"),
            "28 body lines should be ok"
        );
    }

    #[test]
    fn violation_method_31_lines() {
        // def + 30 body lines + end = 32 lines total → triggers
        let src = make_method(31);
        assert!(
            has_rule(&check(&src), "R040"),
            "method > 30 lines should trigger R040"
        );
    }

    #[test]
    fn violation_includes_method_name() {
        let src = make_method(35);
        let diags = check(&src);
        let r040 = diags
            .iter()
            .find(|d| d.rule == "R040")
            .expect("R040 expected");
        assert!(r040.message.contains("foo"));
    }

    // --- R042: cyclomatic complexity ---

    #[test]
    fn no_violation_simple_method() {
        let src = "def foo\n  x = 1\nend";
        assert!(!has_rule(&check(src), "R042"));
    }

    #[test]
    fn no_violation_complexity_10() {
        // baseline 1 + 9 branches = 10 (no violation)
        let branches: String = (0..9)
            .map(|i| format!("  if x == {}\n    y\n  end\n", i))
            .collect();
        let src = format!("def foo\n{}end", branches);
        assert!(
            !has_rule(&check(&src), "R042"),
            "complexity 10 should not trigger"
        );
    }

    #[test]
    fn violation_complexity_11() {
        // baseline 1 + 10 branches = 11 (violation)
        let branches: String = (0..10)
            .map(|i| format!("  if x == {}\n    y\n  end\n", i))
            .collect();
        let src = format!("def foo\n{}end", branches);
        assert!(
            has_rule(&check(&src), "R042"),
            "complexity 11 should trigger R042"
        );
    }

    #[test]
    fn violation_includes_complexity_value() {
        let branches: String = (0..10)
            .map(|i| format!("  if x == {}\n    y\n  end\n", i))
            .collect();
        let src = format!("def foo\n{}end", branches);
        let diags = check(&src);
        let r042 = diags
            .iter()
            .find(|d| d.rule == "R042")
            .expect("R042 expected");
        assert!(r042.message.contains("foo"));
        assert!(r042.message.contains("11"));
    }

    // --- R043: too many parameters ---

    #[test]
    fn no_violation_few_params() {
        let src = "def foo(a, b, c)\nend\n";
        assert!(!has_rule(&check(src), "R043"));
    }

    #[test]
    fn no_violation_exactly_5_params() {
        let src = "def foo(a, b, c, d, e)\nend\n";
        assert!(!has_rule(&check(src), "R043"));
    }

    #[test]
    fn violation_6_params() {
        let src = "def foo(a, b, c, d, e, f)\nend\n";
        assert!(has_rule(&check(src), "R043"), "{:?}", check(src));
    }

    #[test]
    fn violation_includes_method_name_and_count() {
        let src = "def bar(a, b, c, d, e, f)\nend\n";
        let diags = check(src);
        let r043 = diags
            .iter()
            .find(|d| d.rule == "R043")
            .expect("R043 expected");
        assert!(r043.message.contains("bar"));
        assert!(r043.message.contains('6'));
    }

    #[test]
    fn no_violation_default_array_param() {
        // `def foo(a = [1, 2], b, c, d, e)` has 5 real params, not 6
        let src = "def foo(a = [1, 2], b, c, d, e)\nend\n";
        assert!(!has_rule(&check(src), "R043"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_default_hash_param() {
        // Default hash value commas must not be counted
        let src = "def foo(a = {x: 1, y: 2}, b, c, d, e)\nend\n";
        assert!(!has_rule(&check(src), "R043"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_no_params() {
        let src = "def foo\nend\n";
        assert!(!has_rule(&check(src), "R043"));
    }

    #[test]
    fn violation_singleton_method_too_many_params() {
        let src = "def self.foo(a, b, c, d, e, f)\nend\n";
        assert!(has_rule(&check(src), "R043"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_singleton_method_few_params() {
        let src = "def self.foo(a, b)\nend\n";
        assert!(!has_rule(&check(src), "R043"));
    }

    #[test]
    fn violation_paren_less_too_many_params() {
        let src = "def foo a, b, c, d, e, f\nend\n";
        assert!(has_rule(&check(src), "R043"), "{:?}", check(src));
    }

    #[test]
    fn no_violation_paren_less_few_params() {
        let src = "def foo a, b\nend\n";
        assert!(!has_rule(&check(src), "R043"));
    }
}
