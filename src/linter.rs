use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::lexer::{Lexer, TokenKind};
use crate::rules::{all_rules, LintContext};

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

        // Apply inline suppression directives (# rlint:disable-next-line, # rlint:disable)
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

/// Suppression entry: (optional rule list, start line, end line inclusive)
struct Suppression {
    rules: Option<Vec<String>>,
    start: usize,
    end: usize,
}

impl Suppression {
    fn suppresses(&self, diag: &Diagnostic) -> bool {
        if diag.line < self.start || diag.line > self.end {
            return false;
        }
        match &self.rules {
            None => true,
            Some(rs) => rs.iter().any(|r| diag.rule.starts_with(r.as_str())),
        }
    }
}

fn apply_suppressions(diags: &mut Vec<Diagnostic>, tokens: &[crate::lexer::Token]) {
    // Fast path: skip full parsing when no rlint directives exist in the file.
    let has_directives = tokens
        .iter()
        .any(|t| t.kind == TokenKind::Comment && t.text.contains("rlint:"));
    if !has_directives {
        return;
    }

    let mut suppressions: Vec<Suppression> = Vec::new();
    // Active disable block: (rules, start_line)
    let mut active: Option<(Option<Vec<String>>, usize)> = None;

    for token in tokens {
        if token.kind != TokenKind::Comment {
            continue;
        }
        // Strip leading `#` and whitespace
        let text = token.text.trim_start_matches('#').trim();

        if let Some(rest) = text.strip_prefix("rlint:disable-next-line") {
            let rules = parse_rule_list(rest.trim());
            let next_line = token.line + 1;
            suppressions.push(Suppression {
                rules,
                start: next_line,
                end: next_line,
            });
        } else if let Some(rest) = text.strip_prefix("rlint:enable") {
            let enable_rules = parse_rule_list(rest.trim());
            if let Some((active_rules, start)) = active.take() {
                let end = token.line.saturating_sub(1);
                match (enable_rules, active_rules) {
                    // rlint:enable (no rules) → close the entire active block
                    (None, ar) => {
                        suppressions.push(Suppression {
                            rules: ar,
                            start,
                            end,
                        });
                    }
                    // rlint:enable Rxx after rlint:disable (all rules): close the
                    // entire block. The current rule structure cannot represent
                    // "suppress all except Rxx", so a targeted enable after a global
                    // disable re-enables all rules. Document this in user-facing help.
                    (Some(_), None) => {
                        suppressions.push(Suppression {
                            rules: None,
                            start,
                            end,
                        });
                    }
                    // rlint:enable R001 with rlint:disable R001,R002 → partial close
                    (Some(en_rules), Some(ac_rules)) => {
                        // Close the entire original block up to the enable line.
                        suppressions.push(Suppression {
                            rules: Some(ac_rules.clone()),
                            start,
                            end,
                        });
                        // Re-open only the rules that were NOT enabled.
                        // Use the same prefix semantics as Suppression::suppresses():
                        // an ac_rule is considered enabled if any en_rule is a prefix of it.
                        let remaining: Vec<String> = ac_rules
                            .iter()
                            .filter(|r| !en_rules.iter().any(|en| r.starts_with(en.as_str())))
                            .cloned()
                            .collect();
                        if !remaining.is_empty() {
                            active = Some((Some(remaining), token.line + 1));
                        }
                    }
                }
            }
        } else if let Some(rest) = text.strip_prefix("rlint:disable") {
            // Close any previously open block before starting a new one
            if let Some((rules, start)) = active.take() {
                suppressions.push(Suppression {
                    rules,
                    start,
                    end: token.line.saturating_sub(1),
                });
            }
            let rules = parse_rule_list(rest.trim());
            active = Some((rules, token.line));
        }
    }

    // Close any still-open suppression at end of file
    if let Some((rules, start)) = active {
        suppressions.push(Suppression {
            rules,
            start,
            end: usize::MAX,
        });
    }

    if suppressions.is_empty() {
        return;
    }

    diags.retain(|d| !suppressions.iter().any(|s| s.suppresses(d)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_next_line_suppresses_rule() {
        // R002: trailing whitespace on line 3
        // line 2 has disable-next-line R002
        let source = "# frozen_string_literal: true\n# rlint:disable-next-line R002\nx = 1   \n";
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
            "# frozen_string_literal: true\n# rlint:disable-next-line R002\n{}   \n",
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
            "# rlint:disable R002\n",
            "x = 1   \n",
            "y = 2   \n",
            "# rlint:enable R002\n",
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
                "# rlint:disable R001,R002\n",
                "{line}   \n", // line 3: both suppressed
                "# rlint:enable R002\n",
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
        let source = "# frozen_string_literal: true\n# rlint:disable\nx = 1   \n";
        let diags = Linter::new().lint_file("test.rb", source);
        assert!(
            !diags.iter().any(|d| d.rule == "R002"),
            "all rules disabled: {diags:?}"
        );
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
