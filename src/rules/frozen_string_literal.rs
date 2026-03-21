use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};

/// R003 - Files should have `# frozen_string_literal: true` magic comment
pub struct FrozenStringLiteralRule;

impl Rule for FrozenStringLiteralRule {
    fn name(&self) -> &'static str {
        "R003"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        // Skip empty files
        if ctx.lines.is_empty() {
            return vec![];
        }

        // Check first three lines (allow shebang and encoding comment before frozen_string_literal)
        let has_frozen = ctx
            .lines
            .iter()
            .take(3)
            .any(|l| l.contains("frozen_string_literal: true"));

        if !has_frozen {
            // If line 1 is a shebang, insert after it to avoid moving the shebang
            // off the first line and breaking executability.
            // If line 2 is an encoding magic comment (# encoding:/# coding:), insert
            // after it as well so the encoding comment stays within Ruby's detection window.
            let has_shebang = ctx.lines.first().is_some_and(|l| l.starts_with("#!"));
            let insert_line = if has_shebang {
                let line2_is_encoding = ctx.lines.get(1).is_some_and(|l| {
                    let t = l.trim_start_matches('#').trim();
                    t.starts_with("encoding:") || t.starts_with("coding:")
                });
                if line2_is_encoding {
                    3
                } else {
                    2
                }
            } else {
                1
            };
            vec![Diagnostic::new(
                ctx.file,
                insert_line,
                1,
                "R003",
                "Missing `# frozen_string_literal: true` magic comment",
                Severity::Warning,
            )
            .with_insert_before_fix("# frozen_string_literal: true")]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        FrozenStringLiteralRule.check(&ctx)
    }

    #[test]
    fn no_violation_magic_comment_first_line() {
        assert!(check("# frozen_string_literal: true\nx = 1").is_empty());
    }

    #[test]
    fn no_violation_shebang_plus_magic_comment() {
        let src = "#!/usr/bin/env ruby\n# frozen_string_literal: true\nx = 1";
        assert!(check(src).is_empty());
    }

    #[test]
    fn no_violation_magic_comment_on_line_3() {
        let src = "# encoding: utf-8\n# typed: strict\n# frozen_string_literal: true";
        assert!(check(src).is_empty());
    }

    #[test]
    fn violation_no_magic_comment() {
        let diags = check("x = 1\nputs x");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R003");
        assert_eq!(diags[0].line, 1);
    }

    #[test]
    fn violation_fix_is_magic_comment() {
        let diags = check("x = 1");
        assert_eq!(
            diags[0].fix.as_deref(),
            Some("# frozen_string_literal: true")
        );
    }

    #[test]
    fn no_violation_empty_file() {
        assert!(check("").is_empty());
    }

    #[test]
    fn violation_magic_comment_on_line_4() {
        let src = "# a\n# b\n# c\n# frozen_string_literal: true";
        assert_eq!(check(src).len(), 1);
    }

    #[test]
    fn violation_shebang_and_encoding_inserts_after_encoding() {
        // File has shebang + encoding comment; fix should insert at line 3
        // to leave both shebang and encoding comment in place.
        let src = "#!/usr/bin/env ruby\n# encoding: utf-8\nx = 1\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R003");
        assert_eq!(
            diags[0].line, 3,
            "fix insertion point should be after encoding comment"
        );
    }

    #[test]
    fn violation_shebang_inserts_after_shebang() {
        // File has shebang but no frozen comment; fix should target line 2
        // so the shebang stays on line 1.
        let src = "#!/usr/bin/env ruby\nx = 1\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "R003");
        assert_eq!(
            diags[0].line, 2,
            "fix insertion point should be after the shebang"
        );
        assert_eq!(
            diags[0].fix.as_deref(),
            Some("# frozen_string_literal: true")
        );
    }
}
