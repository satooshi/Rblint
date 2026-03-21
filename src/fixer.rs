use crate::diagnostic::{Diagnostic, FixKind};

/// Partition fixable diagnostics into (replace_fixes, insert_fixes), deduplicating
/// by line number within each kind (first-wins). Both kinds may apply to the same line.
fn split_deduplicated_fixes(diags: &[Diagnostic]) -> (Vec<&Diagnostic>, Vec<&Diagnostic>) {
    let mut replace_fixes: Vec<&Diagnostic> = Vec::new();
    let mut insert_fixes: Vec<&Diagnostic> = Vec::new();
    let mut seen_replace = std::collections::HashSet::new();
    let mut seen_insert = std::collections::HashSet::new();
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
    (replace_fixes, insert_fixes)
}

/// Apply all fixable diagnostics to `source` and return the patched content
/// along with the number of fixes applied.
///
/// Fixes are deduplicated per `FixKind`: at most one `ReplaceLine` and one `InsertBefore` fix
/// are applied per line (first-wins within each kind), so both kinds can apply to the same line.
/// Fixes are applied in reverse-line order to preserve 1-based line numbers during insertions.
/// Original line endings (LF or CRLF) are detected from the first newline and preserved.
pub fn apply_fixes(source: &str, diags: &[Diagnostic]) -> (String, usize) {
    let ends_with_newline = source.ends_with('\n');
    let uses_crlf = source
        .find('\n')
        .is_some_and(|i| source.as_bytes().get(i.wrapping_sub(1)) == Some(&b'\r'));
    let line_ending = if uses_crlf { "\r\n" } else { "\n" };
    // str::lines() strips both \n and \r\n, so line content is always clean.
    let mut lines: Vec<String> = source.lines().map(|l| l.to_string()).collect();

    let (mut replace_fixes, mut insert_fixes) = split_deduplicated_fixes(diags);
    let fix_count = replace_fixes.len() + insert_fixes.len();

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

    let mut result = lines.join(line_ending);
    if ends_with_newline {
        result.push_str(line_ending);
    }
    (result, fix_count)
}

/// Apply fixes to a file on disk atomically (write to tmp, then rename).
/// Returns the number of distinct fixes applied (after per-line deduplication), or 0 if unchanged.
pub fn fix_file(path: &str, diags: &[Diagnostic]) -> std::io::Result<usize> {
    let fixable: Vec<&Diagnostic> = diags.iter().filter(|d| d.fix.is_some()).collect();
    if fixable.is_empty() {
        return Ok(0);
    }

    let source = std::fs::read_to_string(path)?;
    let (fixed, fix_count) = apply_fixes(&source, diags);

    if fixed == source {
        return Ok(0);
    }

    // Use the process ID to make temp/backup names unique, avoiding conflicts
    // with existing files or concurrent rlint processes.
    let pid = std::process::id();
    let tmp_path = format!("{}.rlint_{}.tmp", path, pid);
    std::fs::write(&tmp_path, &fixed)?;
    // Preserve original file permissions on the temp file before renaming.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        // On Unix, rename() over an existing file is atomic and should succeed.
        // On Windows it fails with AlreadyExists. We fall back to a backup-and-replace
        // strategy: rename original to a .rlint_bak, then rename tmp into place.
        // This is not atomic on Windows (there is a brief gap between the two renames),
        // but restores the original if the second rename fails.
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            let bak_path = format!("{}.rlint_{}.bak", path, pid);
            if let Err(bak_err) = std::fs::rename(path, &bak_path) {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(bak_err);
            }
            if let Err(retry_err) = std::fs::rename(&tmp_path, path) {
                // Restore original from backup.
                let _ = std::fs::rename(&bak_path, path);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(retry_err);
            }
            let _ = std::fs::remove_file(&bak_path);
        } else {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    Ok(fix_count)
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
        let (result, _) = apply_fixes(source, &[diag]);
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
        let (result, _) = apply_fixes(source, &[diag]);
        assert_eq!(result, "# frozen_string_literal: true\nx = 1\n");
    }

    #[test]
    fn multiple_replace_fixes_applied() {
        let source = "a   \nb   \nc\n";
        let d1 = make_diag(1, "R002", "a", FixKind::ReplaceLine);
        let d2 = make_diag(2, "R002", "b", FixKind::ReplaceLine);
        let (result, _) = apply_fixes(source, &[d1, d2]);
        assert_eq!(result, "a\nb\nc\n");
    }

    #[test]
    fn no_fixable_diags_returns_source_unchanged() {
        let source = "x = 1\n";
        let mut d = Diagnostic::new("test.rb", 1, 1, "R001", "msg", Severity::Warning);
        d.fix = None;
        let (result, _) = apply_fixes(source, &[d]);
        assert_eq!(result, source);
    }

    #[test]
    fn preserves_trailing_newline() {
        let source = "x = 1\n";
        let (result, _) = apply_fixes(source, &[]);
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
        let (result, _) = apply_fixes(source, &[d_insert, d_replace]);
        assert_eq!(result, "# frozen_string_literal: true\nx = 1\n");
    }
}
