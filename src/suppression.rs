use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind};
use crate::linter::parse_rule_list;

/// Suppression entry: (optional rule list, start line, end line inclusive)
pub(crate) struct Suppression {
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

pub(crate) fn apply_suppressions(diags: &mut Vec<Diagnostic>, tokens: &[Token]) {
    // Fast path: skip full parsing when no rblint directives exist in the file.
    let has_directives = tokens
        .iter()
        .any(|t| t.kind == TokenKind::Comment && t.text.contains("rblint:"));
    if !has_directives {
        return;
    }

    let mut suppressions: Vec<Suppression> = Vec::new();
    // Active disable blocks: each entry is (rules, start_line).
    // Multiple concurrent blocks are supported so that a second rblint:disable
    // does not cancel an earlier one.
    let mut active: Vec<(Option<Vec<String>>, usize)> = Vec::new();

    for token in tokens {
        if token.kind != TokenKind::Comment {
            continue;
        }
        // Strip leading `#` and whitespace
        let text = token.text.trim_start_matches('#').trim();

        if let Some(rest) = text.strip_prefix("rblint:disable-next-line") {
            let rules = parse_rule_list(rest.trim());
            let next_line = token.line + 1;
            suppressions.push(Suppression {
                rules,
                start: next_line,
                end: next_line,
            });
        } else if let Some(rest) = text.strip_prefix("rblint:enable") {
            let enable_rules = parse_rule_list(rest.trim());
            let end = token.line.saturating_sub(1);
            match enable_rules {
                // rblint:enable (no rules) → close ALL active blocks
                None => {
                    for (rules, start) in active.drain(..) {
                        suppressions.push(Suppression { rules, start, end });
                    }
                }
                // rblint:enable Rxx → scan every active block and close/partial-close it
                Some(en_rules) => {
                    // If any active block is a global disable (None rules), close ALL blocks.
                    // A targeted enable cannot represent "suppress all except Rxx", so once a
                    // global disable is in scope the entire set is re-enabled together.
                    let has_global = active.iter().any(|(r, _)| r.is_none());
                    if has_global {
                        for (rules, start) in active.drain(..) {
                            suppressions.push(Suppression { rules, start, end });
                        }
                    } else {
                        let mut new_active: Vec<(Option<Vec<String>>, usize)> = Vec::new();
                        for (ac_rules, start) in active.drain(..) {
                            // SAFETY: has_global is false, so every entry has Some rules.
                            let ac_list = ac_rules.expect("no global block");
                            // Close the entire original block up to the enable line.
                            suppressions.push(Suppression {
                                rules: Some(ac_list.clone()),
                                start,
                                end,
                            });
                            // Re-open only the rules that were NOT enabled.
                            // Use the same prefix semantics as Suppression::suppresses():
                            // an ac_rule is considered enabled if any en_rule is a prefix of it.
                            let remaining: Vec<String> = ac_list
                                .iter()
                                .filter(|r| !en_rules.iter().any(|en| r.starts_with(en.as_str())))
                                .cloned()
                                .collect();
                            if !remaining.is_empty() {
                                new_active.push((Some(remaining), token.line + 1));
                            }
                        }
                        active = new_active;
                    }
                }
            }
        } else if let Some(rest) = text.strip_prefix("rblint:disable") {
            // Push a new block without closing existing ones so that concurrent
            // disable blocks can coexist independently.
            let rules = parse_rule_list(rest.trim());
            active.push((rules, token.line));
        }
    }

    // Close any still-open suppressions at end of file
    for (rules, start) in active.drain(..) {
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
