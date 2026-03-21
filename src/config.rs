use serde::Deserialize;
use std::path::Path;

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

impl Config {
    /// Walk up from `start_dir` looking for `.rlint.toml`.
    /// Returns default config if none found.
    pub fn load(start_dir: &Path) -> Self {
        // Canonicalize so that parent() traversal works reliably with relative paths
        // like "." where parent() would otherwise return None immediately.
        let canonical =
            std::fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
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
            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }
        Config::default()
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
}
