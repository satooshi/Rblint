use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;

/// Syntax-level rules
pub struct SyntaxRule;

impl Rule for SyntaxRule {
    fn name(&self) -> &'static str {
        "R030"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // R030: Unbalanced brackets/parens/braces
        let mut paren_stack: Vec<(char, usize, usize)> = Vec::new();
        for tok in tokens {
            match tok.kind {
                TokenKind::LParen => paren_stack.push(('(', tok.line, tok.col)),
                TokenKind::LBracket => paren_stack.push(('[', tok.line, tok.col)),
                TokenKind::LBrace => paren_stack.push(('{', tok.line, tok.col)),
                TokenKind::RParen => {
                    if paren_stack.last().map(|p| p.0) == Some('(') {
                        paren_stack.pop();
                    } else {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R030",
                            "Unexpected `)` — no matching `(`",
                            Severity::Error,
                        ));
                    }
                }
                TokenKind::RBracket => {
                    if paren_stack.last().map(|p| p.0) == Some('[') {
                        paren_stack.pop();
                    } else {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R030",
                            "Unexpected `]` — no matching `[`",
                            Severity::Error,
                        ));
                    }
                }
                TokenKind::RBrace => {
                    if paren_stack.last().map(|p| p.0) == Some('{') {
                        paren_stack.pop();
                    } else {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R030",
                            "Unexpected `}` — no matching `{`",
                            Severity::Error,
                        ));
                    }
                }
                _ => {}
            }
        }
        for (ch, line, col) in paren_stack {
            diags.push(Diagnostic::new(
                ctx.file,
                line,
                col,
                "R030",
                format!("Unclosed `{ch}` — missing closing bracket"),
                Severity::Error,
            ));
        }

        // R031: `end` without matching `def`/`class`/`module`/`do`/`if`
        let mut block_stack: Vec<(&'static str, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let tok = &tokens[i];
            match tok.kind {
                TokenKind::Def
                | TokenKind::Class
                | TokenKind::Module
                | TokenKind::Do
                | TokenKind::Begin => {
                    let label: &'static str = match tok.kind {
                        TokenKind::Def => "def",
                        TokenKind::Class => "class",
                        TokenKind::Module => "module",
                        TokenKind::Do => "do",
                        TokenKind::Begin => "begin",
                        _ => unreachable!(),
                    };
                    block_stack.push((label, tok.line, tok.col));
                }
                // inline if/unless don't need `end` — only block form does
                // This is a heuristic: if `if` is at start of expression (preceded by newline/nothing)
                TokenKind::If
                | TokenKind::Unless
                | TokenKind::While
                | TokenKind::Until
                | TokenKind::For => {
                    // Block form: the previous non-whitespace token is a newline (or start of file)
                    let prev_non_ws = (0..i)
                        .rev()
                        .find(|&j| tokens[j].kind != TokenKind::Whitespace)
                        .map(|j| &tokens[j]);
                    let at_statement_start = match prev_non_ws {
                        None => true,
                        Some(p) => matches!(p.kind, TokenKind::Newline),
                    };
                    if at_statement_start {
                        let label: &'static str = match tok.kind {
                            TokenKind::If => "if",
                            TokenKind::Unless => "unless",
                            TokenKind::While => "while",
                            TokenKind::Until => "until",
                            TokenKind::For => "for",
                            _ => unreachable!(),
                        };
                        block_stack.push((label, tok.line, tok.col));
                    }
                }
                TokenKind::End => {
                    if block_stack.pop().is_none() {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            tok.line,
                            tok.col,
                            "R031",
                            "Unexpected `end` — no matching block opener",
                            Severity::Error,
                        ));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        for (label, line, col) in block_stack {
            diags.push(Diagnostic::new(
                ctx.file,
                line,
                col,
                "R031",
                format!("Missing `end` for `{label}` block"),
                Severity::Error,
            ));
        }

        // R032: Redundant `return` on last line of method
        // Heuristic: `return expr` immediately before `end`
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].kind == TokenKind::Return {
                // Look ahead: skip whitespace/newline, find `end`
                let mut j = i + 1;
                while j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                {
                    j += 1;
                }
                // Skip the return expression (everything until newline)
                while j < tokens.len() && tokens[j].kind != TokenKind::Newline {
                    j += 1;
                }
                // Skip more blank lines
                while j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                {
                    j += 1;
                }
                if j < tokens.len() && tokens[j].kind == TokenKind::End {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        tokens[i].line,
                        tokens[i].col,
                        "R032",
                        "Redundant `return` on last line of method (Ruby returns the last expression implicitly)",
                        Severity::Info,
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
        SyntaxRule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    // --- R030: bracket balance ---

    #[test]
    fn no_violation_balanced_parens() {
        assert!(!has_rule(&check("foo(a, b)"), "R030"));
    }

    #[test]
    fn no_violation_balanced_brackets() {
        assert!(!has_rule(&check("arr[0]"), "R030"));
    }

    #[test]
    fn no_violation_balanced_braces() {
        assert!(!has_rule(&check("h = {a: 1}"), "R030"));
    }

    #[test]
    fn violation_extra_closing_paren() {
        let diags = check("foo())");
        assert!(has_rule(&diags, "R030"), "{diags:?}");
    }

    #[test]
    fn violation_unclosed_paren() {
        let diags = check("foo(a, b");
        assert!(has_rule(&diags, "R030"), "{diags:?}");
    }

    #[test]
    fn violation_extra_closing_bracket() {
        let diags = check("a]");
        assert!(has_rule(&diags, "R030"), "{diags:?}");
    }

    #[test]
    fn violation_extra_closing_brace() {
        let diags = check("x }");
        assert!(has_rule(&diags, "R030"), "{diags:?}");
    }

    #[test]
    fn no_violation_nested_balanced() {
        assert!(!has_rule(&check("foo([1, 2], {a: 3})"), "R030"));
    }

    // --- R031: end matching ---

    #[test]
    fn no_violation_def_end() {
        assert!(!has_rule(&check("def foo\n  1\nend"), "R031"));
    }

    #[test]
    fn no_violation_class_end() {
        assert!(!has_rule(&check("class Foo\nend"), "R031"));
    }

    #[test]
    fn no_violation_if_end() {
        assert!(!has_rule(&check("if true\n  1\nend"), "R031"));
    }

    #[test]
    fn violation_extra_end() {
        let diags = check("def foo\nend\nend");
        assert!(has_rule(&diags, "R031"), "{diags:?}");
    }

    #[test]
    fn violation_missing_end() {
        let diags = check("def foo\n  1");
        assert!(has_rule(&diags, "R031"), "{diags:?}");
    }

    #[test]
    fn no_violation_inline_if_modifier() {
        // `puts x if condition` — inline if, no `end` needed
        let diags = check("puts x if condition");
        assert!(!has_rule(&diags, "R031"), "{diags:?}");
    }

    #[test]
    fn no_violation_inline_unless_modifier() {
        let diags = check("return if done\nreturn unless ready");
        assert!(!has_rule(&diags, "R031"), "{diags:?}");
    }

    // --- R032: redundant return ---

    #[test]
    fn violation_return_on_last_line() {
        let src = "def foo\n  return 42\nend";
        let diags = check(src);
        assert!(has_rule(&diags, "R032"), "{diags:?}");
    }

    #[test]
    fn no_violation_early_return() {
        let src = "def foo\n  return if done\n  do_work\nend";
        let diags = check(src);
        assert!(!has_rule(&diags, "R032"), "{diags:?}");
    }

    #[test]
    fn no_violation_implicit_return() {
        let src = "def foo\n  42\nend";
        assert!(!has_rule(&check(src), "R032"));
    }
}
