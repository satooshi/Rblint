use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
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
    version_hash: u64,
    diagnostics: Vec<CachedDiagnostic>,
}

// ── rule string interning ─────────────────────────────────────────────────────

/// Intern a rule code string so we only leak each unique code once.
/// With ~20 rule codes this is effectively zero overhead.
fn intern_rule(rule: String) -> &'static str {
    static INTERNED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut guard = INTERNED.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    // Check if we already interned this string
    if let Some(&existing) = set.get(rule.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(rule.into_boxed_str());
    set.insert(leaked);
    leaked
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

fn cached_to_diagnostic(file: &str, c: &CachedDiagnostic) -> Diagnostic {
    // Intern rule codes so we only leak each unique string once.
    let rule: &'static str = intern_rule(c.rule.clone());
    let mut d = Diagnostic::new(
        file,
        c.line,
        c.col,
        rule,
        c.message.clone(),
        u8_to_severity(c.severity),
    );
    if let Some(fix) = &c.fix {
        if fix.insert_before {
            d = d.with_insert_before_fix(fix.text.clone());
        } else {
            d = d.with_fix(fix.text.clone());
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
    // Sort vectors before hashing to make the hash order-independent.
    let stable_join = |mut v: Vec<&str>| -> String {
        v.sort_unstable();
        v.join(",")
    };

    let select_str = stable_join(config.select.iter().map(|s| s.as_str()).collect());
    let ignore_str = stable_join(config.ignore.iter().map(|s| s.as_str()).collect());
    let eselect_str = stable_join(config.extend_select.iter().map(|s| s.as_str()).collect());

    let key = format!(
        "ll={},mml={},mcl={},mc={},sel={},ign={},esel={}",
        config.line_length,
        config.max_method_lines,
        config.max_class_lines,
        config.max_complexity,
        select_str,
        ignore_str,
        eselect_str,
    );
    xxh3_64(key.as_bytes())
}

/// Hash of the rblint version string, used to invalidate old cache entries.
fn version_hash() -> u64 {
    xxh3_64(env!("CARGO_PKG_VERSION").as_bytes())
}

// ── Cache ─────────────────────────────────────────────────────────────────────

pub struct Cache {
    entries: HashMap<PathBuf, CacheEntry>,
    path: PathBuf,
    dirty: AtomicBool,
}

impl Cache {
    /// Maximum cache file size (50 MiB).  Files larger than this are treated
    /// as corrupt / malicious and silently ignored.
    const MAX_CACHE_SIZE: u64 = 50 * 1024 * 1024;

    /// Load cache from `cache_path`.  Returns an empty cache on any error
    /// (missing file, corrupted data, size exceeds limit, etc.).
    pub fn load(cache_path: &Path) -> Self {
        let entries: HashMap<PathBuf, CacheEntry> = std::fs::metadata(cache_path)
            .ok()
            .filter(|m| m.len() <= Self::MAX_CACHE_SIZE)
            .and_then(|_| std::fs::read(cache_path).ok())
            .and_then(|bytes| {
                use bincode::Options;
                bincode::DefaultOptions::new()
                    .with_limit(Self::MAX_CACHE_SIZE)
                    .deserialize(&bytes)
                    .ok()
            })
            .unwrap_or_default();
        Cache {
            entries,
            path: cache_path.to_path_buf(),
            dirty: AtomicBool::new(false),
        }
    }

    /// Serialise the cache to disk.  Errors are silently ignored so that a
    /// read-only filesystem does not break normal linting.
    pub fn save(&self) {
        if !self.dirty.load(Ordering::Relaxed) {
            return;
        }
        use bincode::Options;
        if let Ok(bytes) = bincode::DefaultOptions::new()
            .with_limit(Self::MAX_CACHE_SIZE)
            .serialize(&self.entries)
        {
            if std::fs::write(&self.path, bytes).is_ok() {
                self.dirty.store(false, Ordering::Relaxed);
            }
        }
    }

    /// Return cached diagnostics when content hash, config hash, and version
    /// hash all match, otherwise `None`.
    pub fn lookup(
        &self,
        file: &Path,
        content_hash: u64,
        config_hash: u64,
    ) -> Option<Vec<Diagnostic>> {
        let entry = self.entries.get(file)?;
        if entry.content_hash != content_hash
            || entry.config_hash != config_hash
            || entry.version_hash != version_hash()
        {
            return None;
        }
        let file_str = file.to_string_lossy();
        let diags = entry
            .diagnostics
            .iter()
            .map(|c| cached_to_diagnostic(&file_str, c))
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
        self.dirty.store(true, Ordering::Relaxed);
        self.entries.insert(
            file,
            CacheEntry {
                content_hash,
                config_hash,
                version_hash: version_hash(),
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
        let restored = cached_to_diagnostic("test.rb", &cached);
        assert_eq!(restored.rule, "R002");
        assert_eq!(restored.fix.as_deref(), Some("fixed line"));
        assert_eq!(restored.fix_kind, FixKind::ReplaceLine);
    }

    #[test]
    fn roundtrip_diagnostic_insert_before() {
        let d = make_diag_insert("R003");
        let cached = diagnostic_to_cached(&d);
        let restored = cached_to_diagnostic("test.rb", &cached);
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

    #[test]
    fn hash_config_order_independent() {
        let mut c1 = Config::default();
        c1.select = vec!["R001".to_string(), "R002".to_string()];
        c1.ignore = vec!["R003".to_string()];

        let mut c2 = Config::default();
        c2.select = vec!["R002".to_string(), "R001".to_string()];
        c2.ignore = vec!["R003".to_string()];

        assert_eq!(hash_config(&c1), hash_config(&c2));
    }

    #[test]
    fn intern_rule_returns_same_ptr() {
        let r1 = intern_rule("R001".to_string());
        let r2 = intern_rule("R001".to_string());
        // Same pointer — only leaked once
        assert!(std::ptr::eq(r1, r2));
    }
}
