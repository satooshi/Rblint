use clap::{Parser, ValueEnum};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use walkdir::WalkDir;

use rblint::cache::{hash_config, hash_content, Cache};
use rblint::config::Config;
use rblint::diagnostic::{Diagnostic, Severity};
use rblint::linter::Linter;
use rblint::reporter::{OutputFormat, Reporter};
use rblint::rubocop_compat;
use std::sync::RwLock;

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Text,
    Json,
    Github,
}

#[derive(Parser)]
#[command(
    name = "rblint",
    about = "A fast Ruby linter written in Rust",
    version = env!("CARGO_PKG_VERSION"),
    long_about = "
Rblint — Ruff for Ruby

A fast, opinionated Ruby linter inspired by Ruff (Python).
Checks your Ruby code for style issues, naming conventions,
complexity problems, and common mistakes.

Configuration:
  Create a .rlint.toml in your project root to customize settings:

    line-length = 100
    max-method-lines = 40
    ignore = [\"R003\"]

Inline suppression:
  # rlint:disable-next-line R001   (suppress specific rules on next line)
  # rlint:disable R001,R002        (disable specific rules until re-enabled)
  # rlint:disable                  (disable all rules until re-enabled)
  # rlint:enable R001              (re-enable specific rules disabled individually)
  # rlint:enable                   (re-enable all rules)
  Note: after a global disable, enable always re-enables all rules.

Rules:
  R001  Line too long
  R002  Trailing whitespace
  R003  Missing frozen_string_literal magic comment
  R010  Method name not in snake_case
  R011  Constant not starting with uppercase
  R012  Variable using camelCase instead of snake_case
  R020  Semicolon used to separate statements
  R021  Missing space around operator
  R022  Trailing comma before closing parenthesis
  R023  Too many consecutive blank lines
  R024  Use `puts` instead of `p nil`
  R025  Missing final newline at end of file
  R026  Missing blank line between method definitions
  R030  Unbalanced brackets/parentheses/braces
  R031  Missing `end` for block
  R032  Redundant `return` on last line of method
  R040  Method too long (> 30 lines)
  R041  Class too long (> 300 lines)
  R042  High cyclomatic complexity (> 10)
"
)]
struct Cli {
    /// Files or directories to lint
    #[arg(default_value = ".")]
    paths: Vec<String>,

    /// Output format
    #[arg(long, short, value_enum, default_value = "text")]
    format: Format,

    /// Apply auto-fix suggestions to files
    #[arg(long)]
    fix: bool,

    /// Only show errors (hide warnings and info)
    #[arg(long, short)]
    errors_only: bool,

    /// Exit with code 0 even if issues found
    #[arg(long)]
    no_fail: bool,

    /// Select specific rules (comma-separated, e.g. R001,R002)
    #[arg(long)]
    select: Option<String>,

    /// Ignore specific rules (comma-separated)
    #[arg(long)]
    ignore: Option<String>,

    /// Show statistics about rule violations
    #[arg(long)]
    statistics: bool,

    /// Watch files for changes and re-lint automatically
    #[arg(long)]
    watch: bool,

    /// Disable result caching
    #[arg(long)]
    no_cache: bool,

    /// Read .rubocop.yml and print an equivalent .rlint.toml to stdout
    #[arg(long)]
    migrate_config: bool,
}

/// Compile exclude glob patterns once, warning on invalid entries.
fn compile_exclude_patterns(raw: &[String]) -> Vec<glob::Pattern> {
    raw.iter()
        .filter_map(|s| match glob::Pattern::new(s) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("Warning: invalid exclude glob pattern '{}': {}", s, e);
                None
            }
        })
        .collect()
}

/// Strip the leading "./" prefix so that exclude glob patterns like
/// "vendor/**" match paths yielded as "./vendor/foo.rb".
fn normalize_path(raw: &str) -> &str {
    raw.strip_prefix("./").unwrap_or(raw)
}

fn collect_ruby_files(paths: &[String], exclude: &[glob::Pattern]) -> Vec<String> {
    let mut files = Vec::new();
    for path in paths {
        let meta = std::fs::metadata(path);
        if let Ok(m) = meta {
            if m.is_file() {
                let normalized = normalize_path(path);
                if !is_excluded(normalized, exclude) {
                    files.push(normalized.to_string());
                }
            } else {
                for entry in WalkDir::new(path)
                    .follow_links(true)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let p = entry.path();
                    if p.is_file() {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if ext == "rb"
                            || name == "Gemfile"
                            || name == "Rakefile"
                            || name.ends_with(".gemspec")
                            || name == "Guardfile"
                        {
                            let raw = p.to_string_lossy();
                            let path_str = normalize_path(&raw);
                            if !is_excluded(path_str, exclude) {
                                files.push(path_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Returns true if the path matches any pre-compiled exclude glob pattern.
fn is_excluded(path: &str, patterns: &[glob::Pattern]) -> bool {
    let path = std::path::Path::new(path);
    patterns.iter().any(|p| p.matches_path(path))
}

/// Lint a set of files and apply rule filters, returning (path, diagnostics) pairs.
/// When `cache` is `Some`, check the cache before linting and populate it after.
fn lint_files(
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
            let source = std::fs::read_to_string(path).ok()?;
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
fn run_lint_pass(
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
    reporter.print_summary(&flat_diags, files.len(), elapsed);

    if cli_fix && total_fixed > 0 {
        eprintln!("Fixed {} violation(s).", total_fixed);
    }

    if cli_statistics {
        print_statistics(&flat_diags);
    }

    flat_diags.iter().any(|d| d.severity == Severity::Error)
}

fn main() {
    let cli = Cli::parse();

    // Handle --migrate-config: find .rubocop.yml using provided path or CWD.
    // `cli.paths[0]` is the user-supplied path (defaults to ".").
    if cli.migrate_config {
        let start_dir = std::path::Path::new(&cli.paths[0])
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&cli.paths[0]));
        let rubocop_path = rblint::config::find_file_in_ancestors(&start_dir, ".rubocop.yml");
        match rubocop_path {
            None => {
                eprintln!(
                    "Error: .rubocop.yml not found in {} or any parent directory",
                    start_dir.display()
                );
                std::process::exit(1);
            }
            Some(path) => match rubocop_compat::load_rubocop_yml(&path) {
                Some(rubocop_cfg) => {
                    let config = rubocop_compat::convert_to_config(&rubocop_cfg);
                    print!("{}", rubocop_compat::generate_rlint_toml(&config));
                }
                None => {
                    eprintln!("Error: Failed to parse .rubocop.yml");
                    std::process::exit(1);
                }
            },
        }
        return;
    }

    // Load config from .rlint.toml (walk up from CWD)
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut config = Config::load(&cwd);

    // .rblint_cache is always excluded so it never gets linted
    config.exclude.push(".rblint_cache".to_string());

    // CLI flags override config file values
    let cli_select = cli
        .select
        .as_deref()
        .and_then(rblint::linter::parse_rule_list);

    // --ignore appends to config.ignore; also keep a copy for watch-mode reloads
    let cli_ignore: Vec<String> = cli
        .ignore
        .as_deref()
        .and_then(rblint::linter::parse_rule_list)
        .unwrap_or_default();
    config.ignore.extend(cli_ignore.clone());

    let mut effective_select = cli_select.clone().or_else(|| {
        if config.select.is_empty() {
            None
        } else {
            Some(config.select.clone())
        }
    });
    // extend-select adds rules on top of the selected set
    if !config.extend_select.is_empty() {
        if let Some(ref mut sel) = effective_select {
            sel.extend(config.extend_select.iter().cloned());
            sel.sort_unstable();
            sel.dedup();
        }
    }
    let effective_ignore = if config.ignore.is_empty() {
        None
    } else {
        Some(config.ignore.clone())
    };

    let format = match cli.format {
        Format::Text => OutputFormat::Text,
        Format::Json => OutputFormat::Json,
        Format::Github => OutputFormat::Github,
    };

    let reporter = Reporter {
        format,
        show_fixes: !cli.fix,
    };
    let linter = Linter::with_config(&config);

    let exclude_patterns = compile_exclude_patterns(&config.exclude);
    let files = collect_ruby_files(&cli.paths, &exclude_patterns);
    if files.is_empty() {
        eprintln!("No Ruby files found.");
        return;
    }

    // Compute config hash once (used for every cache lookup/store)
    let config_hash = hash_config(&config);

    // Optionally load cache
    let cache_path = cwd.join(".rblint_cache");
    let cache_lock: Option<RwLock<Cache>> = if cli.no_cache {
        None
    } else {
        Some(RwLock::new(Cache::load(&cache_path)))
    };
    if cli.watch {
        run_watch_mode(
            &cli.paths,
            &files,
            &exclude_patterns,
            &config,
            &reporter,
            &cli_select,
            &cli_ignore,
            &effective_select,
            &effective_ignore,
            cli.errors_only,
            cli.fix,
            cli.statistics,
            cache_lock.as_ref(),
            config_hash,
        );
    } else {
        let has_errors = run_lint_pass(
            &files,
            &linter,
            &reporter,
            &effective_select,
            &effective_ignore,
            cli.errors_only,
            cli.fix,
            cli.statistics,
            cache_lock.as_ref(),
            config_hash,
        );

        // Save cache
        if let Some(c) = &cache_lock {
            c.read().unwrap().save();
        }

        if !cli.no_fail && has_errors {
            std::process::exit(1);
        }
    }
}

/// Derive effective select/ignore from a config, applying CLI overrides.
fn compute_effective_select(
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
        if let Some(ref mut sel) = effective {
            sel.extend(config.extend_select.iter().cloned());
            sel.sort_unstable();
            sel.dedup();
        }
    }
    effective
}

fn compute_effective_ignore(config: &Config) -> Option<Vec<String>> {
    if config.ignore.is_empty() {
        None
    } else {
        Some(config.ignore.clone())
    }
}

#[allow(clippy::too_many_arguments)]
fn run_watch_mode(
    watch_paths: &[String],
    initial_files: &[String],
    initial_exclude_patterns: &[glob::Pattern],
    initial_config: &rblint::config::Config,
    reporter: &Reporter,
    cli_select: &Option<Vec<String>>,
    cli_ignore: &[String],
    initial_effective_select: &Option<Vec<String>>,
    initial_effective_ignore: &Option<Vec<String>>,
    errors_only: bool,
    fix: bool,
    statistics: bool,
    cache: Option<&RwLock<Cache>>,
    config_hash: u64,
) {
    use notify::event::{EventKind, ModifyKind};
    use notify::{Config as NConfig, EventHandler, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl+C handler");

    // Mutable state that may be updated when .rlint.toml changes
    let mut current_linter = Linter::with_config(initial_config);
    let mut current_config_hash = config_hash;
    let mut current_effective_select = initial_effective_select.clone();
    let mut current_effective_ignore = initial_effective_ignore.clone();
    let mut current_exclude_patterns = initial_exclude_patterns.to_vec();
    let mut cache_dirty = false;

    // Initial lint
    run_lint_pass(
        initial_files,
        &current_linter,
        reporter,
        &current_effective_select,
        &current_effective_ignore,
        errors_only,
        fix,
        statistics,
        cache,
        current_config_hash,
    );
    if let Some(c) = cache {
        c.read().unwrap().save();
    }
    eprintln!("\n[watching for changes... press Ctrl+C to stop]");

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();

    struct ChannelHandler(mpsc::Sender<notify::Result<notify::Event>>);
    impl EventHandler for ChannelHandler {
        fn handle_event(&mut self, event: notify::Result<notify::Event>) {
            let _ = self.0.send(event);
        }
    }

    let mut watcher = RecommendedWatcher::new(ChannelHandler(tx), NConfig::default())
        .expect("Failed to create file watcher");

    for path in watch_paths {
        let p = std::path::Path::new(path);
        if p.is_dir() {
            watcher
                .watch(p, RecursiveMode::Recursive)
                .unwrap_or_else(|e| eprintln!("Warning: cannot watch {}: {}", path, e));
        } else {
            watcher
                .watch(p, RecursiveMode::NonRecursive)
                .unwrap_or_else(|e| eprintln!("Warning: cannot watch {}: {}", path, e));
        }
    }

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Ok(event)) => {
                let is_relevant = matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(ModifyKind::Data(_))
                        | EventKind::Modify(ModifyKind::Name(_))
                        | EventKind::Remove(_)
                );
                if !is_relevant {
                    continue;
                }

                // Check if .rlint.toml changed — if so, reload config and rebuild linter
                let config_changed = event.paths.iter().any(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == ".rlint.toml")
                        .unwrap_or(false)
                });

                // Check whether any Ruby file was affected
                let ruby_changed = event.paths.iter().any(|p| {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if ext == "rb"
                        || name == "Gemfile"
                        || name == "Rakefile"
                        || name.ends_with(".gemspec")
                        || name == "Guardfile"
                    {
                        let raw = p.to_string_lossy();
                        let s = normalize_path(&raw);
                        return !is_excluded(s, &current_exclude_patterns);
                    }
                    false
                });

                if !config_changed && !ruby_changed {
                    continue;
                }

                // Clear terminal
                print!("\x1B[2J\x1B[1;1H");

                // Drain remaining queued events to avoid redundant re-lints
                while rx.try_recv().is_ok() {}

                // Reload config and rebuild linter when .rlint.toml changes,
                // recomputing effective_select, effective_ignore, and exclude patterns.
                if config_changed {
                    eprintln!("[.rlint.toml changed, reloading config]");
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    let mut new_config = rblint::config::Config::load(&cwd);
                    new_config.exclude.push(".rblint_cache".to_string());
                    // Re-apply CLI --ignore rules on top of the reloaded config
                    new_config.ignore.extend(cli_ignore.iter().cloned());
                    current_config_hash = hash_config(&new_config);
                    current_linter = Linter::with_config(&new_config);
                    current_effective_select = compute_effective_select(&new_config, cli_select);
                    current_effective_ignore = compute_effective_ignore(&new_config);
                    current_exclude_patterns = compile_exclude_patterns(&new_config.exclude);
                }

                // Re-collect full file list in case files were added/removed
                let files = collect_ruby_files(watch_paths, &current_exclude_patterns);

                if files.is_empty() {
                    eprintln!("No Ruby files found.");
                } else {
                    // Always re-lint all files so the full project state is shown
                    // (avoids other files' errors disappearing from output).
                    run_lint_pass(
                        &files,
                        &current_linter,
                        reporter,
                        &current_effective_select,
                        &current_effective_ignore,
                        errors_only,
                        fix,
                        statistics,
                        cache,
                        current_config_hash,
                    );
                    cache_dirty = true;
                }

                eprintln!("\n[watching for changes... press Ctrl+C to stop]");
            }
            Ok(Err(e)) => eprintln!("Watch error: {}", e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Save cache on exit only if modified
    if cache_dirty {
        if let Some(c) = cache {
            c.read().unwrap().save();
        }
    }

    eprintln!("\nStopped.");
}

fn print_statistics(diags: &[Diagnostic]) {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for d in diags {
        *counts.entry(d.rule).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    println!("\nStatistics:");
    for (rule, count) in sorted {
        println!("  {:>5}  {}", count, rule);
    }
}
