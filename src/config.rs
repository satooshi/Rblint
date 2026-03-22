use serde::Deserialize;
use std::path::Path;

use crate::rubocop_compat;

/// Configuration loaded from `.rlint.toml`
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Maximum line length (default: 120)
    #[serde(rename = "line-length")]
    pub line_length: usize,

    /// Maximum method length in lines (default: 30)
    #[serde(rename = "max-method-lines")]
    pub max_method_lines: usize,

    /// Maximum class length in lines (default: 300)
    #[serde(rename = "max-class-lines")]
    pub max_class_lines: usize,

    /// Maximum cyclomatic complexity (default: 10)
    #[serde(rename = "max-complexity")]
    pub max_complexity: usize,

    /// Select only these rules (empty = all rules)
    pub select: Vec<String>,

    /// Ignore these rules
    pub ignore: Vec<String>,

    /// Additional rules to enable on top of defaults
    #[serde(rename = "extend-select")]
    pub extend_select: Vec<String>,

    /// Glob patterns for files/directories to exclude
    pub exclude: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            line_length: 120,
            max_method_lines: 30,
            max_class_lines: 300,
            max_complexity: 10,
            select: vec![],
            ignore: vec![],
            extend_select: vec![],
            exclude: vec![],
        }
    }
}

/// Walk up from `start_dir` looking for a file named `filename`.
/// Returns the first matching path, or `None` if the filesystem root is
/// reached without finding one.
pub fn find_file_in_ancestors(start_dir: &Path, filename: &str) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
    let mut dir: &Path = &canonical;
    loop {
        let candidate = dir.join(filename);
        if candidate.exists() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return None,
        }
    }
}

impl Config {
    /// Walk up from `start_dir` looking for `.rlint.toml`.
    /// If no `.rlint.toml` is found anywhere in the hierarchy, falls back to
    /// the nearest `.rubocop.yml` found during the same traversal.
    /// Returns default config if neither is found.
    pub fn load(start_dir: &Path) -> Self {
        // Canonicalize so that parent() traversal works reliably with relative paths
        // like "." where parent() would otherwise return None immediately.
        let canonical =
            std::fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());

        // First pass: walk the full hierarchy looking for `.rlint.toml`.
        // Along the way, remember the nearest `.rubocop.yml` as a fallback.
        let mut nearest_rubocop: Option<std::path::PathBuf> = None;
        let mut dir: &Path = &canonical;
        loop {
            let config_path = dir.join(".rlint.toml");
            if config_path.exists() {
                match std::fs::read_to_string(&config_path) {
                    Ok(content) => match toml::from_str(&content) {
                        Ok(config) => return config,
                        Err(e) => {
                            eprintln!("Warning: Failed to parse {}: {}", config_path.display(), e);
                            return Config::default();
                        }
                    },
                    Err(e) => {
                        eprintln!("Warning: Failed to read {}: {}", config_path.display(), e);
                        return Config::default();
                    }
                }
            }

            // Record the nearest .rubocop.yml (but keep walking up for .rlint.toml)
            if nearest_rubocop.is_none() {
                let rubocop_path = dir.join(".rubocop.yml");
                if rubocop_path.exists() {
                    nearest_rubocop = Some(rubocop_path);
                }
            }

            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }

        // No .rlint.toml found anywhere — fall back to the nearest .rubocop.yml
        if let Some(rubocop_path) = nearest_rubocop {
            return Config::from_rubocop(&rubocop_path);
        }

        Config::default()
    }

    /// Load config from a `.rubocop.yml` file, converting known cops to Rblint settings.
    /// Returns default config on parse error.
    pub fn from_rubocop(path: &Path) -> Self {
        match rubocop_compat::load_rubocop_yml(path) {
            Some(rubocop) => rubocop_compat::convert_to_config(&rubocop),
            None => Config::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let c = Config::default();
        assert_eq!(c.line_length, 120);
        assert_eq!(c.max_method_lines, 30);
        assert_eq!(c.max_class_lines, 300);
        assert_eq!(c.max_complexity, 10);
        assert!(c.select.is_empty());
        assert!(c.ignore.is_empty());
        assert!(c.exclude.is_empty());
    }

    #[test]
    fn parse_toml_overrides() {
        let toml = r#"
line-length = 100
max-method-lines = 50
ignore = ["R003", "R010"]
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.line_length, 100);
        assert_eq!(c.max_method_lines, 50);
        assert_eq!(c.max_class_lines, 300); // default
        assert_eq!(c.ignore, vec!["R003", "R010"]);
    }

    #[test]
    fn parse_empty_toml_uses_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.line_length, 120);
    }

    /// Helper: create a temp dir, write files at given relative paths, return the tempdir.
    #[cfg(test)]
    fn write_temp_files(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel_path, content) in files {
            let full = dir.path().join(rel_path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create_dir_all");
            }
            std::fs::write(&full, content).expect("write");
        }
        dir
    }

    #[test]
    fn rlint_toml_in_parent_wins_over_rubocop_in_child() {
        // Layout: parent/.rlint.toml  and  parent/child/.rubocop.yml
        // Loading from child/ should find the parent .rlint.toml and NOT use
        // the child .rubocop.yml.
        let dir = write_temp_files(&[
            (".rlint.toml", "line-length = 77\n"),
            ("child/.rubocop.yml", "Layout/LineLength:\n  Max: 999\n"),
        ]);
        let child = dir.path().join("child");
        std::fs::create_dir_all(&child).ok();
        let cfg = Config::load(&child);
        assert_eq!(
            cfg.line_length, 77,
            ".rlint.toml in parent should win over .rubocop.yml in child"
        );
    }

    #[test]
    fn rubocop_fallback_only_when_no_rlint_toml() {
        // Layout: only a .rubocop.yml exists — no .rlint.toml anywhere
        let dir = write_temp_files(&[(".rubocop.yml", "Layout/LineLength:\n  Max: 88\n")]);
        let cfg = Config::load(dir.path());
        assert_eq!(
            cfg.line_length, 88,
            ".rubocop.yml should be used when no .rlint.toml is present"
        );
    }
}
