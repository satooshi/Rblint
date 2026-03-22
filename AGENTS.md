# Rblint Development Guide

A fast Ruby linter written in Rust, inspired by Ruff (Python).

## Build & Test

```bash
cargo build              # Debug build
cargo build --release     # Release build (LTO enabled)
cargo test                # Run all tests
cargo fmt --check         # Check formatting
cargo clippy -- -D warnings  # Lint (warnings treated as errors)
```

## Architecture

```
src/
├── main.rs          # CLI (clap derive) — file collection, parallel processing, output
├── lib.rs           # Library entry point
├── lexer.rs         # Ruby tokenizer
├── linter.rs        # Core engine — rule execution, suppression comment handling
├── config.rs        # .rblint.toml parser (walks up directory tree)
├── diagnostic.rs    # Diagnostic / Severity / FixKind type definitions
├── reporter.rs      # Output formatters (Text / JSON / GitHub Actions)
├── fixer.rs         # Auto-fix engine (line-level replace & insert)
└── rules/
    ├── mod.rs       # Rule trait + all_rules() registry
    ├── naming.rs    # R010-R012: snake_case, constants, variable naming
    ├── line_length.rs          # R001: line length limit
    ├── trailing_whitespace.rs  # R002: trailing whitespace
    ├── frozen_string_literal.rs # R003: frozen_string_literal magic comment
    ├── style.rs     # R020-R024: semicolons, operator spacing, trailing commas, blank lines, p nil
    ├── syntax.rs    # R030-R031: bracket balance, missing end
    └── complexity.rs # R040-R042: method length, class length, cyclomatic complexity
```

## Conventions

- Add new rules in `src/rules/` and register them in `all_rules()` in `mod.rs`
- Rules implement the `Rule` trait: `fn check(&self, ctx: &LintContext) -> Vec<Diagnostic>`
- Rule code scheme: R0xx (naming), R02x (style), R03x (syntax), R04x (complexity)
- CI runs `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`
- Test fixtures go in `tests/fixtures/`
- Config file name is `.rblint.toml`
- Inline suppression comments: `# rblint:disable` / `# rblint:disable-next-line R001`
