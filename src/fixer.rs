use crate::diagnostic::{Diagnostic, FixKind};

/// Apply all fixable diagnostics to `source` and return the patched content.
///
/// Rules:
/// - `FixKind::ReplaceLine`: replace the text of the diagnostic's line (1-based) with the fix string
/// - `FixKind::InsertBefore`: insert the fix string as a new line before the diagnostic's line
///
/// Multiple fixes on the same line are deduplicated (only the first fix per line is applied).
/// Fixes are applied in reverse-line order to preserve 1-based line numbers during insertions.
pub fn apply_fixes(source: &str, diags: &[Diagnostic]) -> String {
    let ends_with_newline = source.ends_with('\n');
    let mut lines: Vec<String> = source.lines().map(|l| l.to_string()).collect();

    // Separate into replace and insert kinds, deduplicate by line number (first-wins)
    let mut replace_fixes: Vec<&Diagnostic> = Vec::new();
    let mut insert_fixes: Vec<&Diagnostic> = Vec::new();
    let mut seen_replace: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut seen_insert: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for diag in diags {
        if diag.fix.is_none() {
            continue;
        }
        match diag.fix_kind {
            FixKind::ReplaceLine => {
                if seen_replace.insert(diag.line) {
                    replace_fixes.push(diag);
                }
            }
            FixKind::InsertBefore => {
                if seen_insert.insert(diag.line) {
                    insert_fixes.push(diag);
                }
            }
        }
    }

    // Apply ReplaceLine in reverse order (stable — no index shifting for same-kind)
    replace_fixes.sort_by(|a, b| b.line.cmp(&a.line));
    for diag in &replace_fixes {
        let idx = diag.line.saturating_sub(1);
        if idx < lines.len() {
            lines[idx] = diag.fix.clone().unwrap_or_default();
        }
    }

    // Apply InsertBefore in reverse order to preserve line numbers
    insert_fixes.sort_by(|a, b| b.line.cmp(&a.line));
    for diag in &insert_fixes {
        let idx = diag.line.saturating_sub(1);
        // Insert at idx (before current line); idx == lines.len() appends at end
        let insert_at = idx.min(lines.len());
        lines.insert(insert_at, diag.fix.clone().unwrap_or_default());
    }

    let mut result = lines.join("\n");
    if ends_with_newline {
        result.push('\n');
    }
    result
}

/// Apply fixes to a file on disk atomically (write to tmp, then rename).
/// Returns the number of fixes applied, or 0 if the file was unchanged.
pub fn fix_file(path: &str, diags: &[Diagnostic]) -> std::io::Result<usize> {
    let fixable: Vec<&Diagnostic> = diags.iter().filter(|d| d.fix.is_some()).collect();
    if fixable.is_empty() {
        return Ok(0);
    }

    let source = std::fs::read_to_string(path)?;
    let fixed = apply_fixes(&source, diags);

    if fixed == source {
        return Ok(0);
    }

    let tmp_path = format!("{}.rlint_tmp", path);
    std::fs::write(&tmp_path, &fixed)?;
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(fixable.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{FixKind, Severity};

    fn make_diag(line: usize, rule: &'static str, fix: &str, kind: FixKind) -> Diagnostic {
        let mut d = Diagnostic::new("test.rb", line, 1, rule, "msg", Severity::Warning);
        d.fix = Some(fix.to_string());
        d.fix_kind = kind;
        d
    }

    #[test]
    fn replace_line_fixes_trailing_whitespace() {
        let source = "x = 1   \ny = 2\n";
        let diag = make_diag(1, "R002", "x = 1", FixKind::ReplaceLine);
        let result = apply_fixes(source, &[diag]);
        assert_eq!(result, "x = 1\ny = 2\n");
    }

    #[test]
    fn insert_before_prepends_line() {
        let source = "x = 1\n";
        let diag = make_diag(
            1,
            "R003",
            "# frozen_string_literal: true",
            FixKind::InsertBefore,
        );
        let result = apply_fixes(source, &[diag]);
        assert_eq!(result, "# frozen_string_literal: true\nx = 1\n");
    }

    #[test]
    fn multiple_replace_fixes_applied() {
        let source = "a   \nb   \nc\n";
        let d1 = make_diag(1, "R002", "a", FixKind::ReplaceLine);
        let d2 = make_diag(2, "R002", "b", FixKind::ReplaceLine);
        let result = apply_fixes(source, &[d1, d2]);
        assert_eq!(result, "a\nb\nc\n");
    }

    #[test]
    fn no_fixable_diags_returns_source_unchanged() {
        let source = "x = 1\n";
        let mut d = Diagnostic::new("test.rb", 1, 1, "R001", "msg", Severity::Warning);
        d.fix = None;
        let result = apply_fixes(source, &[d]);
        assert_eq!(result, source);
    }

    #[test]
    fn preserves_trailing_newline() {
        let source = "x = 1\n";
        let result = apply_fixes(source, &[]);
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn insert_and_replace_together() {
        // R003: insert frozen comment, R002: fix trailing whitespace
        let source = "x = 1   \n";
        let d_insert = make_diag(
            1,
            "R003",
            "# frozen_string_literal: true",
            FixKind::InsertBefore,
        );
        let d_replace = make_diag(1, "R002", "x = 1", FixKind::ReplaceLine);
        let result = apply_fixes(source, &[d_insert, d_replace]);
        assert_eq!(result, "# frozen_string_literal: true\nx = 1\n");
    }
}
