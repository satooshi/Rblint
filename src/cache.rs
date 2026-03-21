use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

use crate::config::Config;
use crate::diagnostic::{Diagnostic, FixKind, Severity};

// ── serialisable types ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CachedFix {
    text: String,
    insert_before: bool,
}

#[derive(Serialize, Deserialize)]
struct CachedDiagnostic {
    rule: String,
    message: String,
    line: usize,
    col: usize,
    /// 0 = Error, 1 = Warning, 2 = Info
    severity: u8,
    fix: Option<CachedFix>,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    content_hash: u64,
    config_hash: u64,
    diagnostics: Vec<CachedDiagnostic>,
}

// ── conversion helpers ────────────────────────────────────────────────────────

fn severity_to_u8(s: &Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn u8_to_severity(v: u8) -> Severity {
    match v {
        0 => Severity::Error,
        1 => Severity::Warning,
        _ => Severity::Info,
    }
}

fn diagnostic_to_cached(d: &Diagnostic) -> CachedDiagnostic {
    CachedDiagnostic {
        rule: d.rule.to_string(),
        message: d.message.clone(),
        line: d.line,
        col: d.col,
        severity: severity_to_u8(&d.severity),
        fix: d.fix.as_ref().map(|text| CachedFix {
            text: text.clone(),
            insert_before: d.fix_kind == FixKind::InsertBefore,
        }),
    }
}

fn cached_to_diagnostic(file: &str, c: CachedDiagnostic) -> Diagnostic {
    // Rule codes are &'static str in Diagnostic.  We store them in the cache as
    // String and must map them back.  The simplest approach is to leak the
    // allocation; for a CLI tool this is fine — the number of unique rule codes
    // is tiny and bounded.
    let rule: &'static str = Box::leak(c.rule.into_boxed_str());
    let mut d = Diagnostic::new(
        file,
        c.line,
        c.col,
        rule,
        c.message,
        u8_to_severity(c.severity),
    );
    if let Some(fix) = c.fix {
        if fix.insert_before {
            d = d.with_insert_before_fix(fix.text);
        } else {
            d = d.with_fix(fix.text);
        }
    }
    d
}

// ── hashing helpers ───────────────────────────────────────────────────────────

/// xxh3 hash of a UTF-8 string (used for file content).
pub fn hash_content(content: &str) -> u64 {
    xxh3_64(content.as_bytes())
}

/// Deterministic hash of the config settings that affect lint results.
/// We serialise the relevant fields to a byte string and hash that.
pub fn hash_config(config: &Config) -> u64 {
    // Build a compact, stable key from every config field that affects output.
    let key = format!(
        "ll={},mml={},mcl={},mc={},sel={:?},ign={:?},esel={:?}",
        config.line_length,
        config.max_method_lines,
        config.max_class_lines,
        config.max_complexity,
        config.select,
        config.ignore,
        config.extend_select,
    );
    xxh3_64(key.as_bytes())
}

// ── Cache ─────────────────────────────────────────────────────────────────────

pub struct Cache {
    entries: HashMap<PathBuf, CacheEntry>,
    path: PathBuf,
}

impl Cache {
    /// Load cache from `cache_path`.  Returns an empty cache on any error
    /// (missing file, corrupted data, etc.).
    pub fn load(cache_path: &Path) -> Self {
        let entries: HashMap<PathBuf, CacheEntry> = std::fs::read(cache_path)
            .ok()
            .and_then(|bytes| bincode::deserialize(&bytes).ok())
            .unwrap_or_default();
        Cache {
            entries,
            path: cache_path.to_path_buf(),
        }
    }

    /// Serialise the cache to disk.  Errors are silently ignored so that a
    /// read-only filesystem does not break normal linting.
    pub fn save(&self) {
        if let Ok(bytes) = bincode::serialize(&self.entries) {
            let _ = std::fs::write(&self.path, bytes);
        }
    }

    /// Return cached diagnostics when both hashes match, otherwise `None`.
    pub fn lookup(
        &self,
        file: &Path,
        content_hash: u64,
        config_hash: u64,
    ) -> Option<Vec<Diagnostic>> {
        let entry = self.entries.get(file)?;
        if entry.content_hash != content_hash || entry.config_hash != config_hash {
            return None;
        }
        let file_str = file.to_string_lossy();
        let diags = entry
            .diagnostics
            .iter()
            .map(|c| {
                cached_to_diagnostic(
                    &file_str,
                    CachedDiagnostic {
                        rule: c.rule.clone(),
                        message: c.message.clone(),
                        line: c.line,
                        col: c.col,
                        severity: c.severity,
                        fix: c.fix.as_ref().map(|f| CachedFix {
                            text: f.text.clone(),
                            insert_before: f.insert_before,
                        }),
                    },
                )
            })
            .collect();
        Some(diags)
    }

    /// Store the lint result for a file.
    pub fn store(
        &mut self,
        file: PathBuf,
        content_hash: u64,
        config_hash: u64,
        diagnostics: &[Diagnostic],
    ) {
        let cached = diagnostics.iter().map(diagnostic_to_cached).collect();
        self.entries.insert(
            file,
            CacheEntry {
                content_hash,
                config_hash,
                diagnostics: cached,
            },
        );
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Severity;
    use tempfile::tempdir;

    fn make_diag(rule: &'static str) -> Diagnostic {
        Diagnostic::new("test.rb", 1, 0, rule, "test message", Severity::Warning)
    }

    fn make_diag_with_fix(rule: &'static str) -> Diagnostic {
        make_diag(rule).with_fix("fixed line")
    }

    fn make_diag_insert(rule: &'static str) -> Diagnostic {
        make_diag(rule).with_insert_before_fix("# inserted")
    }

    #[test]
    fn cache_miss_on_empty() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(".rblint_cache");
        let cache = Cache::load(&cache_path);
        let result = cache.lookup(
            std::path::Path::new("test.rb"),
            hash_content("hello"),
            hash_config(&Config::default()),
        );
        assert!(result.is_none());
    }

    #[test]
    fn cache_hit_after_store() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(".rblint_cache");
        let mut cache = Cache::load(&cache_path);

        let file = PathBuf::from("test.rb");
        let content_hash = hash_content("puts 'hello'");
        let config_hash = hash_config(&Config::default());
        let diags = vec![make_diag("R001")];

        cache.store(file.clone(), content_hash, config_hash, &diags);
        let result = cache.lookup(&file, content_hash, config_hash);
        assert!(result.is_some());
        let returned = result.unwrap();
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].rule, "R001");
    }

    #[test]
    fn cache_miss_on_content_change() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(".rblint_cache");
        let mut cache = Cache::load(&cache_path);

        let file = PathBuf::from("test.rb");
        let config_hash = hash_config(&Config::default());
        let old_hash = hash_content("old content");
        let new_hash = hash_content("new content");
        let diags = vec![make_diag("R002")];

        cache.store(file.clone(), old_hash, config_hash, &diags);
        // Lookup with different content hash → miss
        let result = cache.lookup(&file, new_hash, config_hash);
        assert!(result.is_none());
    }

    #[test]
    fn cache_miss_on_config_change() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(".rblint_cache");
        let mut cache = Cache::load(&cache_path);

        let file = PathBuf::from("test.rb");
        let content_hash = hash_content("puts 'hi'");
        let config1 = Config::default();
        let mut config2 = Config::default();
        config2.line_length = 80;

        let diags = vec![make_diag("R001")];
        cache.store(file.clone(), content_hash, hash_config(&config1), &diags);

        // Different config → miss
        let result = cache.lookup(&file, content_hash, hash_config(&config2));
        assert!(result.is_none());
    }

    #[test]
    fn cache_persists_across_save_load() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join(".rblint_cache");

        let file = PathBuf::from("test.rb");
        let content_hash = hash_content("puts 42");
        let config_hash = hash_config(&Config::default());
        let diags = vec![make_diag("R010")];

        {
            let mut cache = Cache::load(&cache_path);
            cache.store(file.clone(), content_hash, config_hash, &diags);
            cache.save();
        }

        // Load fresh instance
        let cache2 = Cache::load(&cache_path);
        let result = cache2.lookup(&file, content_hash, config_hash);
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0].rule, "R010");
    }

    #[test]
    fn roundtrip_diagnostic_with_fix() {
        let d = make_diag_with_fix("R002");
        let cached = diagnostic_to_cached(&d);
        let restored = cached_to_diagnostic("test.rb", cached);
        assert_eq!(restored.rule, "R002");
        assert_eq!(restored.fix.as_deref(), Some("fixed line"));
        assert_eq!(restored.fix_kind, FixKind::ReplaceLine);
    }

    #[test]
    fn roundtrip_diagnostic_insert_before() {
        let d = make_diag_insert("R003");
        let cached = diagnostic_to_cached(&d);
        let restored = cached_to_diagnostic("test.rb", cached);
        assert_eq!(restored.rule, "R003");
        assert_eq!(restored.fix.as_deref(), Some("# inserted"));
        assert_eq!(restored.fix_kind, FixKind::InsertBefore);
    }

    #[test]
    fn roundtrip_severity_all_variants() {
        for (sev, expected) in [
            (Severity::Error, 0u8),
            (Severity::Warning, 1u8),
            (Severity::Info, 2u8),
        ] {
            assert_eq!(severity_to_u8(&sev), expected);
            assert_eq!(u8_to_severity(expected), sev);
        }
    }
}
