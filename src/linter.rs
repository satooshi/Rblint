use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::lexer::Lexer;
use crate::rules::{all_rules, LintContext};
use crate::suppression::apply_suppressions;

pub struct Linter {
    rules: Vec<Box<dyn crate::rules::Rule + Send + Sync>>,
}

impl Default for Linter {
    fn default() -> Self {
        Self::new()
    }
}

impl Linter {
    pub fn new() -> Self {
        Linter {
            rules: all_rules(&Config::default()),
        }
    }

    pub fn with_config(config: &Config) -> Self {
        Linter {
            rules: all_rules(config),
        }
    }

    pub fn lint_file(&self, path: &str, source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();

        let ctx = LintContext::new(path, source, &lines, &tokens);

        let mut diags: Vec<Diagnostic> = self
            .rules
            .iter()
            .flat_map(|rule| rule.check(&ctx))
            .collect();

        // Apply inline suppression directives (# rblint:disable-next-line, # rblint:disable)
        apply_suppressions(&mut diags, &tokens);

        diags.sort_by(|a, b| a.line.cmp(&b.line).then(a.col.cmp(&b.col)));
        diags
    }
}

/// Parse optional comma-separated rule list from a directive string.
/// Returns `None` to mean "suppress all rules", or `Some(rules)` for specific rules.
pub fn parse_rule_list(s: &str) -> Option<Vec<String>> {
    if s.is_empty() {
        None
    } else {
        let rules: Vec<String> = s
            .split(',')
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect();
        if rules.is_empty() {
            None
        } else {
            Some(rules)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_next_line_suppresses_rule() {
        // R002: trailing whitespace on line 3
        // line 2 has disable-next-line R002
        let source = "# frozen_string_literal: true\n# rblint:disable-next-line R002\nx = 1   \n";
        let diags = Linter::new().lint_file("test.rb", source);
        assert!(
            !diags.iter().any(|d| d.rule == "R002"),
            "R002 should be suppressed: {diags:?}"
        );
    }

    #[test]
    fn disable_next_line_only_suppresses_specified_rule() {
        // R002 disabled, but R001 should still fire
        let long_line = "x".repeat(130);
        let source = format!(
            "# frozen_string_literal: true\n# rblint:disable-next-line R002\n{}   \n",
            long_line
        );
        let diags = Linter::new().lint_file("test.rb", &source);
        assert!(
            !diags.iter().any(|d| d.rule == "R002"),
            "R002 should be suppressed"
        );
        assert!(
            diags.iter().any(|d| d.rule == "R001"),
            "R001 should still fire"
        );
    }

    #[test]
    fn disable_block_suppresses_range() {
        let source = concat!(
            "# frozen_string_literal: true\n",
            "# rblint:disable R002\n",
            "x = 1   \n",
            "y = 2   \n",
            "# rblint:enable R002\n",
            "z = 3   \n",
        );
        let diags = Linter::new().lint_file("test.rb", source);
        // Lines 3 and 4 suppressed; line 6 should fire
        let r002: Vec<_> = diags.iter().filter(|d| d.rule == "R002").collect();
        assert_eq!(r002.len(), 1, "only line 6 should have R002: {r002:?}");
        assert_eq!(r002[0].line, 6);
    }

    #[test]
    fn enable_selective_rules_keeps_others_suppressed() {
        // disable R001 and R002, then enable only R002
        // → R002 fires after enable; R001 remains suppressed throughout
        let long_line = "x".repeat(130);
        let source = format!(
            concat!(
                "# frozen_string_literal: true\n",
                "# rblint:disable R001,R002\n",
                "{line}   \n", // line 3: both suppressed
                "# rblint:enable R002\n",
                "{line}   \n", // line 5: R002 fires, R001 still suppressed
            ),
            line = long_line
        );
        let diags = Linter::new().lint_file("test.rb", &source);
        let r002: Vec<_> = diags.iter().filter(|d| d.rule == "R002").collect();
        let r001: Vec<_> = diags.iter().filter(|d| d.rule == "R001").collect();
        assert_eq!(r002.len(), 1, "R002 should fire only on line 5: {r002:?}");
        assert_eq!(r002[0].line, 5);
        assert_eq!(r001.len(), 0, "R001 should remain suppressed: {r001:?}");
    }

    #[test]
    fn disable_all_rules_no_rule_list() {
        let source = "# frozen_string_literal: true\n# rblint:disable\nx = 1   \n";
        let diags = Linter::new().lint_file("test.rb", source);
        assert!(
            !diags.iter().any(|d| d.rule == "R002"),
            "all rules disabled: {diags:?}"
        );
    }

    #[test]
    fn nested_disable_blocks_independent() {
        // Regression for issue #12: a second rblint:disable must not cancel the first.
        // disable R001, then disable R002, then enable R002 → R001 still suppressed.
        let long_line = "x".repeat(130);
        let source = format!(
            concat!(
                "# frozen_string_literal: true\n",
                "# rblint:disable R001\n",
                "{line}\n", // line 3: R001 suppressed
                "# rblint:disable R002\n",
                "{line}   \n", // line 5: R001+R002 suppressed
                "# rblint:enable R002\n",
                "{line}   \n", // line 7: R001 still suppressed, R002 fires
            ),
            line = long_line
        );
        let diags = Linter::new().lint_file("test.rb", &source);
        let r001: Vec<_> = diags.iter().filter(|d| d.rule == "R001").collect();
        let r002: Vec<_> = diags.iter().filter(|d| d.rule == "R002").collect();
        assert_eq!(r001.len(), 0, "R001 should remain suppressed: {r001:?}");
        assert_eq!(r002.len(), 1, "R002 should fire only on line 7: {r002:?}");
        assert_eq!(r002[0].line, 7);
    }

    #[test]
    fn targeted_enable_with_global_block_closes_all() {
        // When a targeted enable encounters a global-disable block, ALL active blocks
        // are closed (cannot represent "suppress all except Rxx").
        // disable R001 (specific) + disable (global) + enable R002 → everything re-enabled.
        let long_line = "x".repeat(130);
        let source = format!(
            concat!(
                "# frozen_string_literal: true\n",
                "# rblint:disable R001\n",
                "{line}\n", // line 3: R001 suppressed
                "# rblint:disable\n",
                "{line}   \n", // line 5: all suppressed
                "# rblint:enable R002\n",
                "{line}   \n", // line 7: global block triggered close-all → R001 fires too
            ),
            line = long_line
        );
        let diags = Linter::new().lint_file("test.rb", &source);
        let r001: Vec<_> = diags.iter().filter(|d| d.rule == "R001").collect();
        let r002: Vec<_> = diags.iter().filter(|d| d.rule == "R002").collect();
        assert_eq!(r001.len(), 1, "R001 should fire on line 7: {r001:?}");
        assert_eq!(r001[0].line, 7);
        assert_eq!(r002.len(), 1, "R002 should fire on line 7: {r002:?}");
        assert_eq!(r002[0].line, 7);
    }

    #[test]
    fn nested_disable_blocks_global_enable() {
        // Global enable closes all concurrent disable blocks.
        let long_line = "x".repeat(130);
        let source = format!(
            concat!(
                "# frozen_string_literal: true\n",
                "# rblint:disable R001\n",
                "{line}\n", // line 3: R001 suppressed
                "# rblint:disable R002\n",
                "{line}   \n", // line 5: R001+R002 suppressed
                "# rblint:enable\n",
                "{line}   \n", // line 7: both fire
            ),
            line = long_line
        );
        let diags = Linter::new().lint_file("test.rb", &source);
        let r001: Vec<_> = diags.iter().filter(|d| d.rule == "R001").collect();
        let r002: Vec<_> = diags.iter().filter(|d| d.rule == "R002").collect();
        assert_eq!(r001.len(), 1, "R001 should fire on line 7: {r001:?}");
        assert_eq!(r001[0].line, 7);
        assert_eq!(r002.len(), 1, "R002 should fire on line 7: {r002:?}");
        assert_eq!(r002[0].line, 7);
    }

    #[test]
    fn with_config_uses_custom_line_length() {
        let mut config = Config::default();
        config.line_length = 50;
        let linter = Linter::with_config(&config);
        let line = "# frozen_string_literal: true\n".to_string() + &"x".repeat(51) + "\n";
        let diags = linter.lint_file("test.rb", &line);
        assert!(diags.iter().any(|d| d.rule == "R001"), "R001 expected");
    }

    #[test]
    fn with_config_no_r001_under_custom_limit() {
        let mut config = Config::default();
        config.line_length = 200;
        let linter = Linter::with_config(&config);
        // 121-char line: would trigger default R001 but not with limit=200
        let line = "# frozen_string_literal: true\n".to_string() + &"x".repeat(121) + "\n";
        let diags = linter.lint_file("test.rb", &line);
        assert!(
            !diags.iter().any(|d| d.rule == "R001"),
            "R001 should not fire with limit=200"
        );
    }
}
