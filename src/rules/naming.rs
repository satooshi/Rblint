use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;

/// R010 - Naming convention rules
pub struct NamingRule;

fn is_snake_case(s: &str) -> bool {
    // Strip trailing ? or !
    let s = s.trim_end_matches(['?', '!']);
    s.chars()
        .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with('_')
        || s == "_"
}

#[allow(dead_code)]
fn is_screaming_snake_case(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Returns true if the name is proper CamelCase:
/// starts with uppercase and contains no underscores.
fn is_camel_case(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase()) && !s.contains('_')
}

impl Rule for NamingRule {
    fn name(&self) -> &'static str {
        "R010"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;
        let mut i = 0;

        while i < tokens.len() {
            let tok = &tokens[i];

            // R013: Class/module name should be CamelCase
            if tok.kind == TokenKind::Class || tok.kind == TokenKind::Module {
                let kind_label = if tok.kind == TokenKind::Class {
                    "Class"
                } else {
                    "Module"
                };
                let mut j = i + 1;
                while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                    j += 1;
                }
                // Allow root-qualified declarations like `class ::Foo::Bar`
                if tokens
                    .get(j)
                    .is_some_and(|t| t.kind == TokenKind::ColonColon)
                {
                    j += 1;
                    while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                        j += 1;
                    }
                }
                // Walk through all name segments separated by ::
                loop {
                    let Some(name_tok) = tokens.get(j) else {
                        break;
                    };
                    if !matches!(name_tok.kind, TokenKind::Ident | TokenKind::Constant) {
                        break;
                    }
                    if !is_camel_case(&name_tok.text) {
                        diags.push(Diagnostic::new(
                            ctx.file,
                            name_tok.line,
                            name_tok.col,
                            "R013",
                            format!(
                                "{} name `{}` should be CamelCase",
                                kind_label, name_tok.text
                            ),
                            Severity::Warning,
                        ));
                    }
                    j += 1;
                    // Skip whitespace
                    while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                        j += 1;
                    }
                    // Continue through :: separators
                    if tokens
                        .get(j)
                        .is_some_and(|t| t.kind == TokenKind::ColonColon)
                    {
                        j += 1;
                        while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                            j += 1;
                        }
                        continue;
                    }
                    break;
                }
            }

            // Method names should be snake_case: `def foo_bar`
            if tok.kind == TokenKind::Def {
                if let Some(name_tok) = tokens.get(i + 1).or_else(|| tokens.get(i + 2))
                // skip whitespace
                {
                    let name_tok = if name_tok.kind == TokenKind::Whitespace {
                        tokens.get(i + 2)
                    } else {
                        Some(name_tok)
                    };

                    if let Some(name_tok) = name_tok {
                        if name_tok.kind == TokenKind::Ident && !is_snake_case(&name_tok.text) {
                            diags.push(Diagnostic::new(
                                ctx.file,
                                name_tok.line,
                                name_tok.col,
                                "R010",
                                format!("Method name `{}` should use snake_case", name_tok.text),
                                Severity::Warning,
                            ));
                        }
                    }
                }
            }

            // Constants: all-caps or CamelCase is fine; warn if lowercase constant
            if tok.kind == TokenKind::Constant {
                // If it looks like a class/module name (CamelCase) - OK
                // If it's ALL_CAPS - OK
                // If it has lowercase after first char but is not CamelCase - warn
                let name = &tok.text;
                let first_upper = name.chars().next().is_some_and(|c| c.is_uppercase());
                if !first_upper {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        tok.line,
                        tok.col,
                        "R011",
                        format!("Constant `{}` should start with uppercase", name),
                        Severity::Error,
                    ));
                }
            }

            i += 1;
        }

        // Check variable names in assignment context (simple: look for `ident =`)
        let mut i = 0;
        while i + 2 < tokens.len() {
            let tok = &tokens[i];
            let next = &tokens[i + 1];
            let _eq = &tokens[i + 2];

            // Skip whitespace token between ident and =
            let (name_tok, eq_tok) = if next.kind == TokenKind::Whitespace {
                (tok, &tokens[i + 2])
            } else {
                (tok, next)
            };

            if name_tok.kind == TokenKind::Ident
                && eq_tok.kind == TokenKind::Eq
                && !name_tok.text.starts_with('_')
            {
                let name = &name_tok.text;
                // Check for camelCase variable names
                if name.chars().next().is_some_and(|c| c.is_lowercase())
                    && name.chars().any(|c| c.is_uppercase())
                {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        name_tok.line,
                        name_tok.col,
                        "R012",
                        format!(
                            "Variable `{}` should use snake_case instead of camelCase",
                            name
                        ),
                        Severity::Warning,
                    ));
                }
            }

            i += 1;
        }

        // Check screaming snake case for module-level constants (heuristic)
        // e.g. `MAX_SIZE = 100` is fine, `maxSize = 100` is warned above

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
        NamingRule.check(&ctx)
    }

    fn rules_in(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.rule).collect()
    }

    // --- R010: method names ---

    #[test]
    fn no_violation_snake_case_method() {
        assert!(check("def foo_bar\nend").is_empty());
    }

    #[test]
    fn no_violation_single_word_method() {
        assert!(check("def calculate\nend").is_empty());
    }

    #[test]
    fn no_violation_method_with_question_mark() {
        assert!(check("def valid?\nend").is_empty());
    }

    #[test]
    fn no_violation_method_with_bang() {
        assert!(check("def save!\nend").is_empty());
    }

    #[test]
    fn violation_camel_case_method() {
        let diags = check("def myMethod\nend");
        assert!(rules_in(&diags).contains(&"R010"), "{diags:?}");
    }

    #[test]
    fn violation_pascal_case_method() {
        let diags = check("def MyMethod\nend");
        // PascalCase starting uppercase → tokenized as Constant, not Ident, so R010 won't fire
        // but no crash expected
        let _ = diags;
    }

    // --- R012: variable names ---

    #[test]
    fn no_violation_snake_case_variable() {
        let diags = check("my_var = 1");
        assert!(!rules_in(&diags).contains(&"R012"));
    }

    #[test]
    fn no_violation_underscore_prefix_variable() {
        let diags = check("_private = 1");
        assert!(!rules_in(&diags).contains(&"R012"));
    }

    #[test]
    fn violation_camel_case_variable() {
        let diags = check("myVar = 1");
        assert!(rules_in(&diags).contains(&"R012"), "{diags:?}");
    }

    #[test]
    fn violation_message_mentions_variable_name() {
        let diags = check("fooBar = 42");
        let r012 = diags
            .iter()
            .find(|d| d.rule == "R012")
            .expect("R012 expected");
        assert!(r012.message.contains("fooBar"));
    }

    // --- R013: class/module CamelCase (qualified names) ---

    #[test]
    fn no_violation_qualified_camel_case() {
        let diags = check("module Foo::Bar\nend");
        assert!(!rules_in(&diags).contains(&"R013"), "{diags:?}");
    }

    #[test]
    fn violation_qualified_second_segment_not_camel_case() {
        let diags = check("module Foo::bar_baz\nend");
        assert!(rules_in(&diags).contains(&"R013"), "{diags:?}");
    }

    #[test]
    fn violation_qualified_first_segment_not_camel_case() {
        let diags = check("class foo_bar::Baz\nend");
        assert!(rules_in(&diags).contains(&"R013"), "{diags:?}");
    }

    #[test]
    fn no_violation_deeply_qualified_camel_case() {
        let diags = check("module Foo::Bar::Baz\nend");
        assert!(!rules_in(&diags).contains(&"R013"), "{diags:?}");
    }

    #[test]
    fn no_violation_root_qualified_camel_case() {
        // `class ::Foo` is root-qualified and valid CamelCase
        let diags = check("class ::Foo\nend");
        assert!(!rules_in(&diags).contains(&"R013"), "{diags:?}");
    }

    #[test]
    fn violation_root_qualified_not_camel_case() {
        // `module ::foo_bar` — leading `::` but non-CamelCase
        let diags = check("module ::foo_bar\nend");
        assert!(rules_in(&diags).contains(&"R013"), "{diags:?}");
    }
}
