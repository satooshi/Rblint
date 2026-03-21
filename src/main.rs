use clap::{Parser, ValueEnum};
use rayon::prelude::*;
use std::time::Instant;
use walkdir::WalkDir;

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
fn lint_files(
    files: &[String],
    linter: &Linter,
    effective_select: &Option<Vec<String>>,
    effective_ignore: &Option<Vec<String>>,
    errors_only: bool,
) -> Vec<(String, Vec<Diagnostic>)> {
    files
        .par_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let mut diags = linter.lint_file(path, &source);
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

fn main() {
    let cli = Cli::parse();
    let start = Instant::now();

    // Load config from .rlint.toml (walk up from CWD)
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut config = Config::load(&cwd);

    // CLI flags override config file values
    // --select overrides config.select entirely
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
    // extend-select adds rules on top of the selected set (no-op when all rules are active)
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
        show_fixes: !cli.fix, // when --fix is active, don't clutter output with fix hints
    };
    let linter = Linter::with_config(&config);

    let exclude_patterns = compile_exclude_patterns(&config.exclude);
    let files = collect_ruby_files(&cli.paths, &exclude_patterns);
    if files.is_empty() {
        eprintln!("No Ruby files found.");
        return;
    }

    // First lint pass (without errors_only filter when --fix is active, so that
    // fixable warnings like R002/R003 are included in the fix set).
    let all_diags = lint_files(
        &files,
        &linter,
        &effective_select,
        &effective_ignore,
        if cli.fix { false } else { cli.errors_only },
    );

    // Apply fixes when --fix is requested
    let mut total_fixed = 0usize;
    let mut fixed_files: Vec<String> = Vec::new();
    if cli.fix {
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
            &linter,
            &effective_select,
            &effective_ignore,
            cli.errors_only,
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
        .filter(|d| !cli.errors_only || d.severity == Severity::Error)
        .cloned()
        .collect();
    flat_diags.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    reporter.print(&flat_diags);

    let elapsed = start.elapsed().as_millis();
    reporter.print_summary(&flat_diags, files.len(), elapsed);

    if cli.fix && total_fixed > 0 {
        eprintln!("Fixed {} violation(s).", total_fixed);
    }

    if cli.statistics {
        print_statistics(&flat_diags);
    }

    if !cli.no_fail && flat_diags.iter().any(|d| d.severity == Severity::Error) {
        std::process::exit(1);
    }
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
