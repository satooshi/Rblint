use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Instant;

use rayon::prelude::*;

use rblint::cache::{hash_content, Cache};
use rblint::config::Config;
use rblint::diagnostic::{Diagnostic, Severity};
use rblint::linter::Linter;
use rblint::reporter::Reporter;

/// Lint a set of files and apply rule filters, returning (path, diagnostics) pairs.
/// When `cache` is `Some`, check the cache before linting and populate it after.
pub fn lint_files(
    files: &[String],
    linter: &Linter,
    effective_select: &Option<Vec<String>>,
    effective_ignore: &Option<Vec<String>>,
    errors_only: bool,
    cache: Option<&RwLock<Cache>>,
    config_hash: u64,
) -> Vec<(String, Vec<Diagnostic>)> {
    files
        .par_iter()
        .filter_map(|path| {
            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Warning: could not read {}: {}", path, e);
                    return None;
                }
            };
            let content_hash = hash_content(&source);

            // Attempt cache lookup (read lock for lookup, write lock only on miss)
            let raw_diags: Vec<Diagnostic> = if let Some(cache_rw) = cache {
                let hit = {
                    let c = cache_rw.read().unwrap();
                    c.lookup(std::path::Path::new(path), content_hash, config_hash)
                };
                if let Some(cached) = hit {
                    cached
                } else {
                    let fresh = linter.lint_file(path, &source);
                    {
                        let mut c = cache_rw.write().unwrap();
                        c.store(PathBuf::from(path), content_hash, config_hash, &fresh);
                    }
                    fresh
                }
            } else {
                linter.lint_file(path, &source)
            };

            let mut diags = raw_diags;
            diags.retain(|d| {
                if let Some(sel) = effective_select {
                    if !sel.iter().any(|r| d.rule.starts_with(r.as_str())) {
                        return false;
                    }
                }
                if let Some(ign) = effective_ignore {
                    if ign.iter().any(|r| d.rule.starts_with(r.as_str())) {
                        return false;
                    }
                }
                if errors_only && d.severity != Severity::Error {
                    return false;
                }
                true
            });
            Some((path.clone(), diags))
        })
        .collect()
}

/// Run one full lint pass and print results.  Returns whether any errors were found.
#[allow(clippy::too_many_arguments)]
pub fn run_lint_pass(
    files: &[String],
    linter: &Linter,
    reporter: &Reporter,
    effective_select: &Option<Vec<String>>,
    effective_ignore: &Option<Vec<String>>,
    cli_errors_only: bool,
    cli_fix: bool,
    cli_statistics: bool,
    cache: Option<&RwLock<Cache>>,
    config_hash: u64,
) -> bool {
    let start = Instant::now();

    // First lint pass (without errors_only filter when --fix is active)
    let all_diags = lint_files(
        files,
        linter,
        effective_select,
        effective_ignore,
        if cli_fix { false } else { cli_errors_only },
        cache,
        config_hash,
    );

    // Apply fixes when --fix is requested
    let mut total_fixed = 0usize;
    let mut fixed_files: Vec<String> = Vec::new();
    if cli_fix {
        for (path, diags) in &all_diags {
            match rblint::fixer::fix_file(path, diags) {
                Ok(0) => {}
                Ok(n) => {
                    total_fixed += n;
                    fixed_files.push(path.clone());
                }
                Err(e) => eprintln!("Warning: could not fix {}: {}", path, e),
            }
        }
    }

    // Re-lint only the files that were actually modified, then merge with unchanged results.
    let display_diags = if !fixed_files.is_empty() {
        let relinted = lint_files(
            &fixed_files,
            linter,
            effective_select,
            effective_ignore,
            cli_errors_only,
            cache,
            config_hash,
        );
        let relinted_set: HashSet<&str> = fixed_files.iter().map(|s| s.as_str()).collect();
        let mut merged: Vec<(String, Vec<Diagnostic>)> = all_diags
            .into_iter()
            .filter(|(p, _)| !relinted_set.contains(p.as_str()))
            .collect();
        merged.extend(relinted);
        merged
    } else {
        all_diags
    };

    let mut flat_diags: Vec<Diagnostic> = display_diags
        .iter()
        .flat_map(|(_, d)| d.iter())
        .filter(|d| !cli_errors_only || d.severity == Severity::Error)
        .cloned()
        .collect();
    flat_diags.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });

    reporter.print(&flat_diags);

    let elapsed = start.elapsed().as_millis();
    reporter.print_summary(&flat_diags, display_diags.len(), elapsed);

    if cli_fix && total_fixed > 0 {
        eprintln!("Fixed {} violation(s).", total_fixed);
    }

    if cli_statistics {
        print_statistics(&flat_diags);
    }

    flat_diags.iter().any(|d| d.severity == Severity::Error)
}

pub fn print_statistics(diags: &[Diagnostic]) {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for d in diags {
        *counts.entry(d.rule).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!("\nStatistics:");
    for (rule, count) in sorted {
        eprintln!("  {:>5}  {}", count, rule);
    }
}

/// Derive effective select/ignore from a config, applying CLI overrides.
pub fn compute_effective_select(
    config: &Config,
    cli_select: &Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut effective = cli_select.clone().or_else(|| {
        if config.select.is_empty() {
            None
        } else {
            Some(config.select.clone())
        }
    });
    if !config.extend_select.is_empty() {
        let sel = effective.get_or_insert_with(Vec::new);
        sel.extend(config.extend_select.iter().cloned());
        sel.sort_unstable();
        sel.dedup();
    }
    effective
}

pub fn compute_effective_ignore(config: &Config) -> Option<Vec<String>> {
    if config.ignore.is_empty() {
        None
    } else {
        Some(config.ignore.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rblint::config::Config;

    #[test]
    fn extend_select_alone_returns_those_rules() {
        let mut config = Config::default();
        config.extend_select = vec!["R001".to_string(), "R002".to_string()];
        let result = compute_effective_select(&config, &None);
        assert_eq!(result, Some(vec!["R001".to_string(), "R002".to_string()]));
    }

    #[test]
    fn extend_select_appends_to_base_select() {
        let mut config = Config::default();
        config.select = vec!["R001".to_string()];
        config.extend_select = vec!["R002".to_string()];
        let result = compute_effective_select(&config, &None);
        assert_eq!(result, Some(vec!["R001".to_string(), "R002".to_string()]));
    }

    #[test]
    fn extend_select_empty_with_no_select_returns_none() {
        let config = Config::default();
        let result = compute_effective_select(&config, &None);
        assert_eq!(result, None);
    }
}
