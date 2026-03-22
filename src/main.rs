mod cli;
mod diff_filter;
mod file_collector;
mod runner;
mod watcher;

use clap::Parser;
use std::sync::RwLock;

use rblint::cache::{hash_config, Cache};
use rblint::config::Config;
use rblint::linter::Linter;
use rblint::reporter::{OutputFormat, Reporter};
use rblint::rubocop_compat;

use cli::{Cli, Format};
use file_collector::{collect_ruby_files, compile_exclude_patterns};
use runner::{compute_effective_ignore, compute_effective_select, run_lint_pass};
use watcher::run_watch_mode;

fn main() {
    let cli = Cli::parse();

    // Handle --init: generate a default .rblint.toml in the current directory
    if cli.init {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let config_path = cwd.join(".rblint.toml");
        if config_path.exists() {
            eprintln!(
                "Error: .rblint.toml already exists at {}",
                config_path.display()
            );
            std::process::exit(1);
        }
        let content = generate_default_config();
        if let Err(e) = std::fs::write(&config_path, &content) {
            eprintln!("Error: Failed to write .rblint.toml: {e}");
            std::process::exit(1);
        }
        println!("Created {}", config_path.display());
        // Suggest migration if .rubocop.yml exists
        if cwd.join(".rubocop.yml").exists() {
            eprintln!(
                "Tip: A .rubocop.yml was found. Run `rblint --migrate-config` to convert it."
            );
        }
        return;
    }

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
                    print!("{}", rubocop_compat::generate_rblint_toml(&config));
                }
                None => {
                    eprintln!("Error: Failed to parse .rubocop.yml");
                    std::process::exit(1);
                }
            },
        }
        return;
    }

    // Validate incompatible flag combinations early
    if cli.diff && cli.watch {
        eprintln!("Error: --diff and --watch cannot be used together");
        std::process::exit(1);
    }
    if cli.diff && cli.fix {
        eprintln!("Error: --diff and --fix cannot be used together");
        std::process::exit(1);
    }

    // Load config from .rblint.toml (walk up from CWD)
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

    let effective_select = compute_effective_select(&config, &cli_select);
    let effective_ignore = compute_effective_ignore(&config);

    let format = match cli.format {
        Format::Text => OutputFormat::Text,
        Format::Json => OutputFormat::Json,
        Format::Github => OutputFormat::Github,
        Format::Sarif => OutputFormat::Sarif,
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
    // --diff mode: read unified diff from stdin, filter diagnostics to changed lines only
    let diff_changed = if cli.diff {
        use std::io::Read;
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
            eprintln!("Error reading diff from stdin: {e}");
            std::process::exit(1);
        }
        Some(diff_filter::parse_diff(&buf))
    } else {
        None
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
            diff_changed.as_ref(),
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

/// Generate the content for a default .rblint.toml with comments.
fn generate_default_config() -> String {
    r#"# Rblint configuration
# See https://github.com/satooshi/Rblint for documentation

# Maximum line length (default: 120)
line-length = 120

# Maximum method length in lines (default: 30)
max-method-lines = 30

# Maximum class length in lines (default: 300)
max-class-lines = 300

# Maximum cyclomatic complexity per method (default: 10)
max-complexity = 10

# Maximum number of method parameters (default: 5)
max-parameters = 5

# Rules to enable (empty = all rules enabled)
# select = ["R001", "R002"]

# Rules to disable
# ignore = ["R003"]

# Additional rules to enable beyond defaults
# extend-select = []

# Paths/globs to exclude from linting (default: empty)
# exclude = ["vendor/**", "node_modules/**"]
"#
    .to_string()
}
