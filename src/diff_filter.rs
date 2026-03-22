/// Unified diff parser — maps file paths to changed line numbers.
///
/// Parses a unified diff (e.g. from `git diff`) and returns a map of
/// file path → set of added/modified line numbers in the *new* file.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Parse a unified diff string.
/// Returns a map of file path to the set of changed (added) line numbers.
pub fn parse_diff(diff: &str) -> HashMap<PathBuf, HashSet<usize>> {
    let mut result: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    let mut current_file: Option<PathBuf> = None;
    let mut current_new_line: usize = 0;

    for line in diff.lines() {
        if let Some(path_str) = line.strip_prefix("+++ ") {
            // New file path: `+++ b/path/to/file` or `+++ /dev/null`
            if path_str == "/dev/null" {
                current_file = None;
            } else {
                // Strip leading `b/` prefix (git diff format)
                let stripped = path_str.strip_prefix("b/").unwrap_or(path_str);
                current_file = Some(PathBuf::from(stripped));
            }
        } else if line.starts_with("@@ ") {
            // Hunk header: `@@ -old_start,old_count +new_start,new_count @@`
            // Extract new_start from the `+new_start` part
            if let Some(new_start) = parse_hunk_new_start(line) {
                current_new_line = new_start;
            }
        } else if let Some(ref file) = current_file {
            if line.starts_with('+') && !line.starts_with("+++") {
                // Added line in the new file
                result
                    .entry(file.clone())
                    .or_default()
                    .insert(current_new_line);
                current_new_line += 1;
            } else if line.starts_with(' ') {
                // Context line — counts toward new file line numbers
                current_new_line += 1;
            }
            // Lines starting with `-` are removed from old file, don't advance new line counter
        }
    }

    result
}

/// Parse the new-file start line from a hunk header like `@@ -1,5 +3,10 @@`.
fn parse_hunk_new_start(line: &str) -> Option<usize> {
    // Find the `+` after `@@ -old `
    let after_at = line.strip_prefix("@@ ")?;
    let plus_pos = after_at.find(" +")?;
    let rest = &after_at[plus_pos + 2..];
    // rest is like `3,10 @@ ...` or `3 @@ ...`
    let end = rest.find([',', ' ']).unwrap_or(rest.len());
    rest[..end].parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = "\
diff --git a/app/foo.rb b/app/foo.rb
--- a/app/foo.rb
+++ b/app/foo.rb
@@ -1,5 +1,7 @@
 # frozen_string_literal: true
+
 class Foo
-  def bar
+  def bar(x)
+    x * 2
   end
 end
";

    #[test]
    fn parses_added_lines() {
        let map = parse_diff(SAMPLE_DIFF);
        let file = PathBuf::from("app/foo.rb");
        let lines = map.get(&file).expect("file not found");
        // Line 2 is blank added line, line 4 is `def bar(x)`, line 5 is `x * 2`
        assert!(lines.contains(&2), "line 2 should be changed: {lines:?}");
        assert!(lines.contains(&4), "line 4 should be changed: {lines:?}");
        assert!(lines.contains(&5), "line 5 should be changed: {lines:?}");
    }

    #[test]
    fn unchanged_lines_not_included() {
        let map = parse_diff(SAMPLE_DIFF);
        let file = PathBuf::from("app/foo.rb");
        let lines = map.get(&file).expect("file not found");
        // Line 1 is context (unchanged)
        assert!(
            !lines.contains(&1),
            "line 1 should not be changed: {lines:?}"
        );
    }

    #[test]
    fn empty_diff_returns_empty_map() {
        assert!(parse_diff("").is_empty());
    }

    #[test]
    fn strips_b_prefix() {
        let diff = "--- a/lib/foo.rb\n+++ b/lib/foo.rb\n@@ -1 +1,2 @@\n line\n+added\n";
        let map = parse_diff(diff);
        assert!(map.contains_key(&PathBuf::from("lib/foo.rb")));
    }
}
