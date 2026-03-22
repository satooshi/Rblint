use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum Format {
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
  Create a .rblint.toml in your project root to customize settings:

    line-length = 100
    max-method-lines = 40
    ignore = [\"R003\"]

Inline suppression:
  # rblint:disable-next-line R001   (suppress specific rules on next line)
  # rblint:disable R001,R002        (disable specific rules until re-enabled)
  # rblint:disable                  (disable all rules until re-enabled)
  # rblint:enable R001              (re-enable specific rules disabled individually)
  # rblint:enable                   (re-enable all rules)
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
pub struct Cli {
    /// Files or directories to lint
    #[arg(default_value = ".")]
    pub paths: Vec<String>,

    /// Output format
    #[arg(long, short, value_enum, default_value = "text")]
    pub format: Format,

    /// Apply auto-fix suggestions to files
    #[arg(long)]
    pub fix: bool,

    /// Only show errors (hide warnings and info)
    #[arg(long, short)]
    pub errors_only: bool,

    /// Exit with code 0 even if issues found
    #[arg(long)]
    pub no_fail: bool,

    /// Select specific rules (comma-separated, e.g. R001,R002)
    #[arg(long)]
    pub select: Option<String>,

    /// Ignore specific rules (comma-separated)
    #[arg(long)]
    pub ignore: Option<String>,

    /// Show statistics about rule violations
    #[arg(long)]
    pub statistics: bool,

    /// Watch files for changes and re-lint automatically
    #[arg(long)]
    pub watch: bool,

    /// Disable result caching
    #[arg(long)]
    pub no_cache: bool,

    /// Read .rubocop.yml and print an equivalent .rblint.toml to stdout
    #[arg(long)]
    pub migrate_config: bool,
}
