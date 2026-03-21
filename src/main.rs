use clap::{Parser, ValueEnum};
use rayon::prelude::*;
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
    cache: Option<&std::sync::Mutex<Cache>>,
    config_hash: u64,
) -> Vec<(String, Vec<Diagnostic>)> {
    files
        .par_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let content_hash = hash_content(&source);

            // Attempt cache lookup
            let raw_diags: Vec<Diagnostic> = if let Some(cache_mutex) = cache {
                let hit = {
                    let c = cache_mutex.lock().unwrap();
                    c.lookup(std::path::Path::new(path), content_hash, config_hash)
                };
                if let Some(cached) = hit {
                    cached
                } else {
                    let fresh = linter.lint_file(path, &source);
                    {
                        let mut c = cache_mutex.lock().unwrap();
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
    cache: Option<&std::sync::Mutex<Cache>>,
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
        let relinted_set: std::collections::HashSet<&str> =
            fixed_files.iter().map(|s| s.as_str()).collect();
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
    flat_diags.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

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

    // Load config from .rlint.toml (walk up from CWD)
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut config = Config::load(&cwd);

    // .rblint_cache is always excluded so it never gets linted
    config.exclude.push(".rblint_cache".to_string());

    // CLI flags override config file values
    let selected = cli
        .select
        .as_deref()
        .and_then(rblint::linter::parse_rule_list);

    // --ignore appends to config.ignore
    if let Some(ign_str) = &cli.ignore {
        if let Some(extra) = rblint::linter::parse_rule_list(ign_str) {
            config.ignore.extend(extra);
        }
    }

    let mut effective_select = selected.or_else(|| {
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
    let cache_mutex: Option<std::sync::Mutex<Cache>> = if cli.no_cache {
        None
    } else {
        Some(std::sync::Mutex::new(Cache::load(&cache_path)))
    };
    let cache_ref = cache_mutex.as_ref();

    if cli.watch {
        run_watch_mode(
            &cli.paths,
            &files,
            &exclude_patterns,
            &linter,
            &reporter,
            &effective_select,
            &effective_ignore,
            cli.errors_only,
            cli.fix,
            cli.statistics,
            cache_ref,
            config_hash,
            &cache_path,
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
            cache_ref,
            config_hash,
        );

        // Save cache
        if let Some(c) = &cache_mutex {
            c.lock().unwrap().save();
        }

        if !cli.no_fail && has_errors {
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_watch_mode(
    watch_paths: &[String],
    initial_files: &[String],
    exclude_patterns: &[glob::Pattern],
    linter: &Linter,
    reporter: &Reporter,
    effective_select: &Option<Vec<String>>,
    effective_ignore: &Option<Vec<String>>,
    errors_only: bool,
    fix: bool,
    statistics: bool,
    cache: Option<&std::sync::Mutex<Cache>>,
    config_hash: u64,
    cache_path: &std::path::Path,
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

    // Initial lint
    run_lint_pass(
        initial_files,
        linter,
        reporter,
        effective_select,
        effective_ignore,
        errors_only,
        fix,
        statistics,
        cache,
        config_hash,
    );
    if let Some(c) = cache {
        c.lock().unwrap().save();
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

                // Determine which Ruby files were affected
                let changed: Vec<String> = event
                    .paths
                    .iter()
                    .filter_map(|p| {
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
                            if !is_excluded(s, exclude_patterns) {
                                return Some(s.to_string());
                            }
                        }
                        None
                    })
                    .collect();

                if changed.is_empty() {
                    continue;
                }

                // Clear terminal
                print!("\x1B[2J\x1B[1;1H");

                // Drain remaining queued events to avoid redundant re-lints
                while rx.try_recv().is_ok() {}

                // Re-collect full file list in case files were added/removed
                let files = collect_ruby_files(watch_paths, exclude_patterns);

                if files.is_empty() {
                    eprintln!("No Ruby files found.");
                } else {
                    // Only re-lint the changed files that exist in the full list
                    let relint_targets: Vec<String> =
                        changed.into_iter().filter(|f| files.contains(f)).collect();

                    let targets = if relint_targets.is_empty() {
                        // Removed file or new file — re-lint everything
                        files.clone()
                    } else {
                        relint_targets
                    };

                    run_lint_pass(
                        &targets,
                        linter,
                        reporter,
                        effective_select,
                        effective_ignore,
                        errors_only,
                        fix,
                        statistics,
                        cache,
                        config_hash,
                    );
                    if let Some(c) = cache {
                        c.lock().unwrap().save();
                    }
                }

                eprintln!("\n[watching for changes... press Ctrl+C to stop]");
            }
            Ok(Err(e)) => eprintln!("Watch error: {}", e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Save cache on exit
    if let Some(c) = cache {
        c.lock().unwrap().save();
    }

    eprintln!("\nStopped.");
    // Save the cache path to avoid an unused warning for the parameter
    let _ = cache_path;
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
