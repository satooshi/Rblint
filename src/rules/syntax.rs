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

        // R034: Empty rescue body (swallowed exception)
        {
            let mut i = 0;
            while i < tokens.len() {
                if tokens[i].kind == TokenKind::Rescue {
                    // Distinguish clause rescue from modifier rescue.
                    // A clause rescue follows a newline (or is at file start);
                    // a modifier rescue (`expr rescue fallback`) follows an expression.
                    let prev_non_ws = (0..i)
                        .rev()
                        .find(|&k| tokens[k].kind != TokenKind::Whitespace)
                        .map(|k| &tokens[k]);
                    let is_clause = match prev_non_ws {
                        None => true,
                        Some(p) => matches!(p.kind, TokenKind::Newline),
                    };
                    if !is_clause {
                        i += 1;
                        continue;
                    }
                    let rescue_line = tokens[i].line;
                    // Skip past the rescue line (exception class list, etc.)
                    let mut j = i + 1;
                    while j < tokens.len() && tokens[j].kind != TokenKind::Newline {
                        j += 1;
                    }
                    // Skip newline
                    if j < tokens.len() {
                        j += 1;
                    }
                    // Skip whitespace/newlines — if the next non-ws token is
                    // `end`, `rescue`, or `ensure`, the body is empty
                    while j < tokens.len()
                        && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                    {
                        j += 1;
                    }
                    if j < tokens.len()
                        && matches!(
                            tokens[j].kind,
                            TokenKind::End | TokenKind::Rescue | TokenKind::Ensure
                        )
                    {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            rescue_line,
                            tokens[i].col,
                            "R034",
                            "Empty `rescue` body suppresses exceptions silently — add error handling or logging",
                            Severity::Warning,
                        ));
                    }
                }
                i += 1;
            }
        }

        // R035: Unreachable code after `return`/`raise`/`break`/`next`
        {
            let mut i = 0;
            while i < tokens.len() {
                let is_terminator = matches!(tokens[i].kind, TokenKind::Return | TokenKind::Raise);
                if !is_terminator {
                    i += 1;
                    continue;
                }

                let term_line = tokens[i].line;

                // Check if there's a postfix modifier on the same line.
                // Only `if`/`unless`/`while`/`until` make return/raise conditional.
                // Logical operators (`||`, `&&`, `or`, `and`) are part of the
                // returned/raised expression and do NOT make the terminator optional.
                let mut has_inline_condition = false;
                let mut j = i + 1;
                while j < tokens.len() && tokens[j].kind != TokenKind::Newline {
                    if matches!(
                        tokens[j].kind,
                        TokenKind::If | TokenKind::Unless | TokenKind::While | TokenKind::Until
                    ) {
                        has_inline_condition = true;
                        break;
                    }
                    j += 1;
                }
                if has_inline_condition {
                    i += 1;
                    continue;
                }

                // Skip to end of current line
                let mut j = i + 1;
                while j < tokens.len() && tokens[j].kind != TokenKind::Newline {
                    j += 1;
                }
                // Skip the newline
                if j < tokens.len() {
                    j += 1;
                }

                // Check the next non-blank line
                // Skip blank lines
                while j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                {
                    j += 1;
                }

                // If the next meaningful token is not `end`/`rescue`/`ensure`/`else`/`elsif`/`when`,
                // it's potentially unreachable code
                if j < tokens.len() {
                    let next_kind = &tokens[j].kind;
                    let next_line = tokens[j].line;
                    if !matches!(
                        next_kind,
                        TokenKind::End
                            | TokenKind::Rescue
                            | TokenKind::Ensure
                            | TokenKind::Else
                            | TokenKind::Elsif
                            | TokenKind::When
                    ) && next_line > term_line
                    {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            next_line,
                            tokens[j].col,
                            "R035",
                            "Unreachable code after `return`/`raise`",
                            Severity::Warning,
                        ));
                    }
                }

                i += 1;
            }
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

    // Bug: %x[...] shell literals must not produce R030 "Unclosed `[`"
    #[test]
    fn no_violation_percent_x_bracket() {
        assert!(
            !has_rule(&check("out = %x[git show]"), "R030"),
            "{:?}",
            check("out = %x[git show]")
        );
    }

    #[test]
    fn no_violation_percent_x_bracket_with_interpolation() {
        assert!(
            !has_rule(&check("out = %x[git show #{ref}]"), "R030"),
            "{:?}",
            check("out = %x[git show #{ref}]")
        );
    }

    #[test]
    fn no_violation_percent_x_angle() {
        assert!(
            !has_rule(&check("out = %x<ls #{dir}>"), "R030"),
            "{:?}",
            check("out = %x<ls #{dir}>")
        );
    }

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

    // --- R035: unreachable code ---

    #[test]
    fn no_violation_return_if_modifier() {
        // `return if condition` is a modifier form — code after it is NOT unreachable
        let src = "def foo\n  return if done?\n  do_work\nend";
        let diags = check(src);
        assert!(!has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn no_violation_raise_unless_modifier() {
        let src = "def foo\n  raise unless valid?\n  do_work\nend";
        let diags = check(src);
        assert!(!has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn violation_unconditional_return() {
        let src = "def foo\n  return 42\n  do_work\nend";
        let diags = check(src);
        assert!(has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn violation_unconditional_raise() {
        let src = "def foo\n  raise \"err\"\n  do_work\nend";
        let diags = check(src);
        assert!(has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn violation_return_with_logical_or() {
        // `return foo || default` is still unconditional — `||` only affects the value
        let src = "def foo\n  return value || fallback\n  do_work\nend";
        let diags = check(src);
        assert!(has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn violation_return_with_logical_and() {
        let src = "def foo\n  return a && b\n  do_work\nend";
        let diags = check(src);
        assert!(has_rule(&diags, "R035"), "{diags:?}");
    }

    #[test]
    fn no_violation_modifier_rescue() {
        // `expr rescue fallback` is a modifier form — NOT an empty rescue clause
        let src = "def foo\n  result = danger rescue nil\nend\n";
        let diags = check(src);
        assert!(!has_rule(&diags, "R034"), "{diags:?}");
    }

    #[test]
    fn violation_empty_rescue_clause() {
        let src = "begin\n  danger\nrescue\nend\n";
        let diags = check(src);
        assert!(has_rule(&diags, "R034"), "{diags:?}");
    }
}
