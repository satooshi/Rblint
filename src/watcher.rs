use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use rblint::cache::{hash_config, Cache};
use rblint::config::Config;
use rblint::linter::Linter;
use rblint::reporter::Reporter;

use crate::file_collector::{collect_ruby_files, compile_exclude_patterns};
use crate::runner::{compute_effective_ignore, compute_effective_select, run_lint_pass};

#[allow(clippy::too_many_arguments)]
pub fn run_watch_mode(
    watch_paths: &[String],
    initial_files: &[String],
    initial_exclude_patterns: &[glob::Pattern],
    initial_config: &Config,
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

    // Mutable state that may be updated when .rblint.toml changes
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

                // Check if .rblint.toml changed — if so, reload config and rebuild linter
                let config_changed = event.paths.iter().any(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == ".rblint.toml")
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
                        let s = crate::file_collector::normalize_path(&raw);
                        return !crate::file_collector::is_excluded(s, &current_exclude_patterns);
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

                // Reload config and rebuild linter when .rblint.toml changes,
                // recomputing effective_select, effective_ignore, and exclude patterns.
                if config_changed {
                    eprintln!("[.rblint.toml changed, reloading config]");
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
