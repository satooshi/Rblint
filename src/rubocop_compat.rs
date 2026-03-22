//! RuboCop compatibility layer.
//!
//! Parses `.rubocop.yml` and converts it into an Rblint [`Config`].
//! Also provides `generate_rblint_toml` to emit an equivalent `.rblint.toml`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::config::Config;

// ---------------------------------------------------------------------------
// Cop → Rule mapping
// ---------------------------------------------------------------------------

/// Map from RuboCop cop name to Rblint rule code.
pub fn cop_to_rule(cop: &str) -> Option<&'static str> {
    match cop {
        "Layout/LineLength" => Some("R001"),
        "Layout/TrailingWhitespace" => Some("R002"),
        "Style/FrozenStringLiteralComment" => Some("R003"),
        "Naming/MethodName" => Some("R010"),
        "Naming/ConstantName" => Some("R011"),
        "Style/Semicolon" => Some("R020"),
        "Layout/SpaceAroundOperators" => Some("R021"),
        "Style/TrailingCommaInArguments" => Some("R022"),
        "Layout/EmptyLines" => Some("R023"),
        "Metrics/MethodLength" => Some("R040"),
        "Metrics/ClassLength" => Some("R041"),
        "Metrics/CyclomaticComplexity" => Some("R042"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Serde types for .rubocop.yml
// ---------------------------------------------------------------------------

/// A single cop's configuration block.
#[derive(Debug, Default)]
struct CopConfig {
    enabled: Option<bool>,
    /// Generic `Max` threshold used by several cops (e.g. LineLength, MethodLength).
    max: Option<u64>,
}

/// Top-level `.rubocop.yml` structure.
///
/// The file is a YAML mapping of cop names (strings) to their config blocks.
/// `serde(flatten)` captures **all** top-level keys into the `cops` map —
/// including non-cop keys like `AllCops` and `inherit_from`.  These are
/// handled explicitly in parsing/conversion; unrecognised keys simply do not
/// match any entry in `cop_to_rule` and are therefore ignored.
#[derive(Debug, Deserialize, Default)]
pub struct RuboCopConfig {
    /// All cop sections, keyed by cop name.
    ///
    /// We capture everything as a loose map; non-cop keys will simply not match
    /// any entry in `cop_to_rule`.
    #[serde(flatten)]
    pub cops: HashMap<String, serde_yml::Value>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Load and parse a single `.rubocop.yml` file from `path` (no inherit_from).
fn load_rubocop_yml_raw(path: &Path) -> Option<RuboCopConfig> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: Failed to read {}: {}", path.display(), e);
            return None;
        }
    };
    match serde_yml::from_str::<RuboCopConfig>(&content) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
            None
        }
    }
}

/// Deep-merge `overlay` into `base`, following these rules:
/// - Mappings: recursively merge key-by-key (overlay wins for conflicting scalars).
/// - Sequences: concatenate (`base` entries first, then `overlay` entries).
/// - Scalars / mismatched types: overlay wins.
fn deep_merge_value(base: &mut serde_yml::Value, overlay: serde_yml::Value) {
    match (base, overlay) {
        (serde_yml::Value::Mapping(base_map), serde_yml::Value::Mapping(overlay_map)) => {
            for (k, v) in overlay_map {
                match base_map.get_mut(&k) {
                    Some(existing) => deep_merge_value(existing, v),
                    None => {
                        base_map.insert(k, v);
                    }
                }
            }
        }
        (serde_yml::Value::Sequence(base_seq), serde_yml::Value::Sequence(overlay_seq)) => {
            base_seq.extend(overlay_seq);
        }
        (base, overlay) => *base = overlay,
    }
}

/// Load and parse a `.rubocop.yml` file from `path`, resolving `inherit_from`
/// references transitively.  Inherited files are merged in order (later files
/// override earlier ones); the main file then wins on all conflicts.
///
/// Mappings (e.g. `AllCops`, cop blocks) are merged at the field level rather
/// than replaced wholesale.  Sequences (e.g. `AllCops: Exclude`) are
/// concatenated so that neither file's entries are lost.
///
/// Returns `None` only when the main file itself cannot be read or parsed.
/// Errors in inherited files are skipped with a warning (best-effort merge).
pub fn load_rubocop_yml(path: &Path) -> Option<RuboCopConfig> {
    let base_dir = path.parent().unwrap_or(Path::new("."));

    // Parse the main file first so we can inspect `inherit_from`.
    let main = load_rubocop_yml_raw(path)?;

    // Collect inherit_from entries (string or array of strings).
    let inherited_paths: Vec<std::path::PathBuf> = main
        .cops
        .get("inherit_from")
        .map(|v| match v {
            serde_yml::Value::String(s) => vec![base_dir.join(s)],
            serde_yml::Value::Sequence(seq) => seq
                .iter()
                .filter_map(|item| {
                    if let serde_yml::Value::String(s) = item {
                        Some(base_dir.join(s))
                    } else {
                        None
                    }
                })
                .collect(),
            _ => vec![],
        })
        .unwrap_or_default();

    if inherited_paths.is_empty() {
        return Some(main);
    }

    // Merge: start with inherited files (resolved transitively, in order),
    // then overlay main.  Deep-merge so that nested mappings and sequences
    // are combined rather than replaced.
    let mut merged = RuboCopConfig::default();
    for inh_path in &inherited_paths {
        // Recursive call resolves each inherited file's own `inherit_from`.
        if let Some(inh_cfg) = load_rubocop_yml(inh_path) {
            for (k, v) in inh_cfg.cops {
                match merged.cops.get_mut(&k) {
                    Some(existing) => deep_merge_value(existing, v),
                    None => {
                        merged.cops.insert(k, v);
                    }
                }
            }
        }
    }
    // Main file wins — deep-merge so that e.g. AllCops keys from inherited
    // files are preserved unless explicitly overridden.
    for (k, v) in main.cops {
        match merged.cops.get_mut(&k) {
            Some(existing) => deep_merge_value(existing, v),
            None => {
                merged.cops.insert(k, v);
            }
        }
    }

    Some(merged)
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Extract a `CopConfig` from a raw `serde_yml::Value`.
///
/// Returns `CopConfig::default()` when the value is not a mapping or when
/// individual fields are absent / have unexpected types.
fn value_to_cop_config(val: &serde_yml::Value) -> CopConfig {
    let mut cfg = CopConfig::default();
    if let serde_yml::Value::Mapping(map) = val {
        for (k, v) in map {
            if let serde_yml::Value::String(key) = k {
                match key.as_str() {
                    "Enabled" => {
                        if let serde_yml::Value::Bool(b) = v {
                            cfg.enabled = Some(*b);
                        }
                    }
                    "Max" => {
                        if let serde_yml::Value::Number(n) = v {
                            cfg.max = n.as_u64();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    cfg
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a parsed [`RuboCopConfig`] into an Rblint [`Config`].
///
/// Rules whose cop has `Enabled: false` are added to `config.ignore`.
/// Known `Max` thresholds are mapped to the corresponding Rblint setting.
/// `AllCops: Exclude` patterns are mapped to `config.exclude`.
pub fn convert_to_config(rubocop: &RuboCopConfig) -> Config {
    let mut config = Config::default();

    // Extract AllCops.Exclude patterns into config.exclude
    if let Some(serde_yml::Value::Mapping(all_cops_map)) = rubocop.cops.get("AllCops") {
        for (k, v) in all_cops_map {
            if let serde_yml::Value::String(key) = k {
                if key == "Exclude" {
                    if let serde_yml::Value::Sequence(seq) = v {
                        for item in seq {
                            if let serde_yml::Value::String(pattern) = item {
                                config.exclude.push(pattern.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    for (cop_name, raw_val) in &rubocop.cops {
        let cop_cfg = value_to_cop_config(raw_val);

        // Only process cops we know about
        let Some(rule_code) = cop_to_rule(cop_name) else {
            continue;
        };

        // Disabled cop → add rule to ignore list
        if cop_cfg.enabled == Some(false) {
            config.ignore.push(rule_code.to_string());
        }

        // Extract threshold values
        if let Some(max) = cop_cfg.max {
            let Some(max) = usize::try_from(max).ok() else {
                eprintln!(
                    "Warning: Max value {} for {} is out of range, ignoring",
                    max, cop_name
                );
                continue;
            };
            match cop_name.as_str() {
                "Layout/LineLength" => config.line_length = max,
                "Metrics/MethodLength" => config.max_method_lines = max,
                "Metrics/ClassLength" => config.max_class_lines = max,
                "Metrics/CyclomaticComplexity" => config.max_complexity = max,
                _ => {}
            }
        }
    }

    // Sort ignore list for deterministic output
    config.ignore.sort_unstable();

    config
}

/// Serialise an Rblint [`Config`] as a `.rblint.toml`-formatted string.
///
/// Only non-default values are emitted so the output is as minimal as possible.
pub fn generate_rblint_toml(config: &Config) -> String {
    let defaults = Config::default();
    let mut lines: Vec<String> = Vec::new();

    if config.line_length != defaults.line_length {
        lines.push(format!("line-length = {}", config.line_length));
    }
    if config.max_method_lines != defaults.max_method_lines {
        lines.push(format!("max-method-lines = {}", config.max_method_lines));
    }
    if config.max_class_lines != defaults.max_class_lines {
        lines.push(format!("max-class-lines = {}", config.max_class_lines));
    }
    if config.max_complexity != defaults.max_complexity {
        lines.push(format!("max-complexity = {}", config.max_complexity));
    }

    let escape_toml_str = |s: &str| -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0C}' => out.push_str("\\f"),
                c if c.is_control() => {
                    // TOML uses \uXXXX for other control characters
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        out.push_str(&format!("\\u{:04X}", unit));
                    }
                }
                c => out.push(c),
            }
        }
        out
    };
    let fmt_list = |v: &[String]| -> String {
        v.iter()
            .map(|r| format!("\"{}\"", escape_toml_str(r)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !config.ignore.is_empty() {
        lines.push(format!("ignore = [{}]", fmt_list(&config.ignore)));
    }
    if !config.select.is_empty() {
        lines.push(format!("select = [{}]", fmt_list(&config.select)));
    }
    if !config.extend_select.is_empty() {
        lines.push(format!(
            "extend-select = [{}]",
            fmt_list(&config.extend_select)
        ));
    }
    if !config.exclude.is_empty() {
        lines.push(format!("exclude = [{}]", fmt_list(&config.exclude)));
    }

    if lines.is_empty() {
        "# All settings are at their defaults — no overrides needed.\n".to_string()
    } else {
        lines.join("\n") + "\n"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- cop_to_rule mapping ---

    #[test]
    fn known_cops_map_to_rules() {
        assert_eq!(cop_to_rule("Layout/LineLength"), Some("R001"));
        assert_eq!(cop_to_rule("Layout/TrailingWhitespace"), Some("R002"));
        assert_eq!(
            cop_to_rule("Style/FrozenStringLiteralComment"),
            Some("R003")
        );
        assert_eq!(cop_to_rule("Naming/MethodName"), Some("R010"));
        assert_eq!(cop_to_rule("Naming/ConstantName"), Some("R011"));
        assert_eq!(cop_to_rule("Style/Semicolon"), Some("R020"));
        assert_eq!(cop_to_rule("Layout/SpaceAroundOperators"), Some("R021"));
        assert_eq!(cop_to_rule("Style/TrailingCommaInArguments"), Some("R022"));
        assert_eq!(cop_to_rule("Layout/EmptyLines"), Some("R023"));
        assert_eq!(cop_to_rule("Metrics/MethodLength"), Some("R040"));
        assert_eq!(cop_to_rule("Metrics/ClassLength"), Some("R041"));
        assert_eq!(cop_to_rule("Metrics/CyclomaticComplexity"), Some("R042"));
    }

    #[test]
    fn unknown_cop_returns_none() {
        assert_eq!(cop_to_rule("Style/NonExistentCop"), None);
        assert_eq!(cop_to_rule(""), None);
        assert_eq!(cop_to_rule("Metrics/Foo"), None);
    }

    // --- convert_to_config ---

    fn parse_yaml_config(yaml: &str) -> Config {
        let rubocop: RuboCopConfig = serde_yml::from_str(yaml).expect("valid YAML");
        convert_to_config(&rubocop)
    }

    #[test]
    fn disabled_cop_adds_to_ignore() {
        let yaml = r#"
Layout/TrailingWhitespace:
  Enabled: false
"#;
        let cfg = parse_yaml_config(yaml);
        assert!(cfg.ignore.contains(&"R002".to_string()));
    }

    #[test]
    fn enabled_true_cop_not_ignored() {
        let yaml = r#"
Layout/TrailingWhitespace:
  Enabled: true
"#;
        let cfg = parse_yaml_config(yaml);
        assert!(!cfg.ignore.contains(&"R002".to_string()));
    }

    #[test]
    fn line_length_max_propagates() {
        let yaml = r#"
Layout/LineLength:
  Max: 100
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.line_length, 100);
    }

    #[test]
    fn method_length_max_propagates() {
        let yaml = r#"
Metrics/MethodLength:
  Max: 50
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.max_method_lines, 50);
    }

    #[test]
    fn class_length_max_propagates() {
        let yaml = r#"
Metrics/ClassLength:
  Max: 200
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.max_class_lines, 200);
    }

    #[test]
    fn cyclomatic_complexity_max_propagates() {
        let yaml = r#"
Metrics/CyclomaticComplexity:
  Max: 7
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.max_complexity, 7);
    }

    #[test]
    fn unknown_cops_are_silently_ignored() {
        let yaml = r#"
SomeUnknown/Cop:
  Enabled: false
  Max: 5
"#;
        let cfg = parse_yaml_config(yaml);
        assert!(cfg.ignore.is_empty());
    }

    #[test]
    fn multiple_cops_combined() {
        let yaml = r#"
Layout/LineLength:
  Max: 80
Naming/MethodName:
  Enabled: false
Metrics/MethodLength:
  Max: 20
  Enabled: false
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.line_length, 80);
        assert_eq!(cfg.max_method_lines, 20);
        assert!(cfg.ignore.contains(&"R010".to_string()));
        assert!(cfg.ignore.contains(&"R040".to_string()));
    }

    // --- generate_rblint_toml ---

    #[test]
    fn generate_toml_emits_non_default_values() {
        let mut cfg = Config::default();
        cfg.line_length = 80;
        cfg.ignore = vec!["R003".to_string()];
        let toml = generate_rblint_toml(&cfg);
        assert!(toml.contains("line-length = 80"), "got: {toml}");
        assert!(toml.contains("ignore = [\"R003\"]"), "got: {toml}");
        assert!(!toml.contains("max-method-lines"), "got: {toml}");
    }

    #[test]
    fn generate_toml_all_defaults_emits_comment() {
        let cfg = Config::default();
        let toml = generate_rblint_toml(&cfg);
        assert!(toml.contains("defaults"), "got: {toml}");
    }

    // --- invalid YAML ---

    #[test]
    fn invalid_yaml_returns_none() {
        // load_rubocop_yml is hard to test without a real file; test the
        // serde parse path directly instead.
        let result = serde_yml::from_str::<RuboCopConfig>(": !!invalid yaml [[[");
        assert!(result.is_err());
    }

    // --- integration: full sample .rubocop.yml ---

    #[test]
    fn full_sample_rubocop_yml() {
        let yaml = r#"
AllCops:
  NewCops: enable
  TargetRubyVersion: 3.1

inherit_from: .rubocop_todo.yml

Layout/LineLength:
  Max: 120

Layout/TrailingWhitespace:
  Enabled: true

Style/FrozenStringLiteralComment:
  Enabled: false

Naming/MethodName:
  Enabled: true

Metrics/MethodLength:
  Max: 40
  Enabled: true

Metrics/CyclomaticComplexity:
  Max: 8

SomeGem/CustomCop:
  Enabled: false
"#;
        let cfg = parse_yaml_config(yaml);

        // Thresholds
        assert_eq!(cfg.line_length, 120);
        assert_eq!(cfg.max_method_lines, 40);
        assert_eq!(cfg.max_complexity, 8);

        // Disabled cops → ignored rules
        assert!(
            cfg.ignore.contains(&"R003".to_string()),
            "R003 should be ignored"
        );

        // Enabled cops should NOT be in ignore
        assert!(!cfg.ignore.contains(&"R002".to_string()));
        assert!(!cfg.ignore.contains(&"R010".to_string()));
        assert!(!cfg.ignore.contains(&"R040".to_string()));

        // Unknown cops should not appear in ignore
        assert_eq!(cfg.ignore.len(), 1, "only one rule should be ignored");
    }

    // --- AllCops: Exclude mapping ---

    #[test]
    fn allcops_exclude_maps_to_config_exclude() {
        let yaml = r#"
AllCops:
  Exclude:
    - "vendor/**/*"
    - "db/schema.rb"
    - "tmp/**/*"
"#;
        let cfg = parse_yaml_config(yaml);
        assert_eq!(cfg.exclude.len(), 3);
        assert!(cfg.exclude.contains(&"vendor/**/*".to_string()));
        assert!(cfg.exclude.contains(&"db/schema.rb".to_string()));
        assert!(cfg.exclude.contains(&"tmp/**/*".to_string()));
    }

    // --- inherit_from resolution ---

    #[test]
    fn inherit_from_resolves_single_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parent_yaml = r#"
Layout/LineLength:
  Max: 80
"#;
        let main_yaml = r#"
inherit_from: parent.yml

Style/FrozenStringLiteralComment:
  Enabled: false
"#;
        std::fs::write(dir.path().join("parent.yml"), parent_yaml).unwrap();
        std::fs::write(dir.path().join(".rubocop.yml"), main_yaml).unwrap();

        let cfg_raw = load_rubocop_yml(&dir.path().join(".rubocop.yml")).expect("should parse");
        let cfg = convert_to_config(&cfg_raw);

        // Inherited value
        assert_eq!(cfg.line_length, 80);
        // Main file value
        assert!(cfg.ignore.contains(&"R003".to_string()));
    }

    #[test]
    fn inherit_from_later_file_overrides_earlier() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("first.yml"),
            "Layout/LineLength:\n  Max: 80\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("second.yml"),
            "Layout/LineLength:\n  Max: 100\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".rubocop.yml"),
            "inherit_from:\n  - first.yml\n  - second.yml\n",
        )
        .unwrap();

        let cfg_raw = load_rubocop_yml(&dir.path().join(".rubocop.yml")).expect("should parse");
        let cfg = convert_to_config(&cfg_raw);

        // second.yml should override first.yml
        assert_eq!(cfg.line_length, 100);
    }

    #[test]
    fn inherit_from_main_overrides_inherited() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("parent.yml"),
            "Layout/LineLength:\n  Max: 80\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".rubocop.yml"),
            "inherit_from: parent.yml\nLayout/LineLength:\n  Max: 120\n",
        )
        .unwrap();

        let cfg_raw = load_rubocop_yml(&dir.path().join(".rubocop.yml")).expect("should parse");
        let cfg = convert_to_config(&cfg_raw);

        // Main file wins
        assert_eq!(cfg.line_length, 120);
    }

    #[test]
    fn inherit_from_missing_file_is_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(".rubocop.yml"),
            "inherit_from: nonexistent.yml\nLayout/LineLength:\n  Max: 90\n",
        )
        .unwrap();

        let cfg_raw = load_rubocop_yml(&dir.path().join(".rubocop.yml")).expect("should parse");
        let cfg = convert_to_config(&cfg_raw);

        assert_eq!(cfg.line_length, 90);
    }

    // --- generate_rblint_toml: extend-select ---

    #[test]
    fn generate_toml_emits_extend_select() {
        let mut cfg = Config::default();
        cfg.extend_select = vec!["R003".to_string(), "R010".to_string()];
        let toml = generate_rblint_toml(&cfg);
        assert!(
            toml.contains("extend-select"),
            "expected extend-select in output, got: {toml}"
        );
        assert!(toml.contains("R003"), "got: {toml}");
        assert!(toml.contains("R010"), "got: {toml}");
    }

    // --- TOML escaping ---

    #[test]
    fn toml_escaping_handles_special_chars() {
        let mut cfg = Config::default();
        cfg.ignore = vec!["has\"quote".to_string(), "has\\slash".to_string()];
        cfg.exclude = vec!["has\nnewline".to_string(), "has\ttab".to_string()];
        let toml = generate_rblint_toml(&cfg);
        assert!(
            toml.contains(r#"has\"quote"#),
            "double quote should be escaped"
        );
        assert!(
            toml.contains(r#"has\\slash"#),
            "backslash should be escaped"
        );
        assert!(
            toml.contains(r#"has\nnewline"#),
            "newline should be escaped"
        );
        assert!(toml.contains(r#"has\ttab"#), "tab should be escaped");
    }
}
