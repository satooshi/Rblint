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
}

impl Default for ComplexityRule {
    fn default() -> Self {
        let config = crate::config::Config::default();
        ComplexityRule {
            max_method_lines: config.max_method_lines,
            max_class_lines: config.max_class_lines,
            max_complexity: config.max_complexity,
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
}
