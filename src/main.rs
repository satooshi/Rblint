use std::time::Instant;
use clap::{Parser, ValueEnum};
use rayon::prelude::*;
use walkdir::WalkDir;

use rlint::linter::Linter;
use rlint::reporter::{OutputFormat, Reporter};

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Text,
    Json,
    Github,
}

#[derive(Parser)]
#[command(
    name = "rlint",
    about = "A fast Ruby linter written in Rust",
    version = env!("CARGO_PKG_VERSION"),
    long_about = "
Rlint — Ruff for Ruby

A fast, opinionated Ruby linter inspired by Ruff (Python).
Checks your Ruby code for style issues, naming conventions,
complexity problems, and common mistakes.

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

    /// Show auto-fix suggestions
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

fn collect_ruby_files(paths: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for path in paths {
        let meta = std::fs::metadata(path);
        if let Ok(m) = meta {
            if m.is_file() {
                files.push(path.clone());
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
                        if ext == "rb" || name == "Gemfile" || name == "Rakefile"
                            || name.ends_with(".gemspec") || name == "Guardfile"
                        {
                            files.push(p.to_string_lossy().into_owned());
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

fn main() {
    let cli = Cli::parse();
    let start = Instant::now();

    let format = match cli.format {
        Format::Text   => OutputFormat::Text,
        Format::Json   => OutputFormat::Json,
        Format::Github => OutputFormat::Github,
    };

    let reporter = Reporter { format, show_fixes: cli.fix };
    let linter = Linter::new();

    let selected: Option<Vec<String>> = cli.select.as_deref()
        .map(|s| s.split(',').map(|r| r.to_string()).collect());
    let ignored: Option<Vec<String>> = cli.ignore.as_deref()
        .map(|s| s.split(',').map(|r| r.to_string()).collect());

    let files = collect_ruby_files(&cli.paths);
    if files.is_empty() {
        eprintln!("No Ruby files found.");
        return;
    }

    // Lint files in parallel
    let all_diags: Vec<(String, Vec<rlint::diagnostic::Diagnostic>)> = files.par_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let mut diags = linter.lint_file(path, &source);

            // Apply rule filters
            diags.retain(|d| {
                if let Some(sel) = &selected {
                    if !sel.iter().any(|r| d.rule.starts_with(r.as_str())) { return false; }
                }
                if let Some(ign) = &ignored {
                    if ign.iter().any(|r| d.rule.starts_with(r.as_str())) { return false; }
                }
                if cli.errors_only && d.severity != rlint::diagnostic::Severity::Error {
                    return false;
                }
                true
            });

            Some((path.clone(), diags))
        })
        .collect();

    let mut flat_diags: Vec<rlint::diagnostic::Diagnostic> = all_diags.iter()
        .flat_map(|(_, d)| d.iter())
        .cloned()
        .collect();
    flat_diags.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    reporter.print(&flat_diags);

    let elapsed = start.elapsed().as_millis();
    reporter.print_summary(&flat_diags, files.len(), elapsed);

    if cli.statistics {
        print_statistics(&flat_diags);
    }

    if !cli.no_fail && flat_diags.iter().any(|d| d.severity == rlint::diagnostic::Severity::Error) {
        std::process::exit(1);
    }
}

fn print_statistics(diags: &[rlint::diagnostic::Diagnostic]) {
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
