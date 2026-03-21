# Rblint — Ruff for Ruby

A fast Ruby linter written in Rust, inspired by [Ruff](https://github.com/astral-sh/ruff).

## Features

- **Fast**: Parallel file processing with Rayon
- **Multiple output formats**: text (colored), JSON, GitHub Actions annotations
- **Auto-fix suggestions**: Shows suggested fixes for applicable rules
- **Rule filtering**: Select or ignore specific rules with `--select` / `--ignore`
- **Statistics**: Rule violation counts with `--statistics`

## Installation

```sh
cargo build --release
# Binary at: target/release/rblint
```

## Usage

```sh
# Lint current directory
rblint

# Lint specific files or directories
rblint app/ lib/ spec/

# Show fix suggestions
rblint --fix app/

# JSON output (for editors/CI)
rblint --format json app/

# GitHub Actions annotations
rblint --format github app/

# Only errors, no warnings
rblint --errors-only app/

# Select specific rules
rblint --select R001,R002,R003 app/

# Ignore rules
rblint --ignore R003,R021 app/

# Show statistics
rblint --statistics app/
```

## Rules

| Code | Description |
|------|-------------|
| R001 | Line too long (default max: 120 characters) |
| R002 | Trailing whitespace |
| R003 | Missing `# frozen_string_literal: true` magic comment |
| R010 | Method name not in snake_case |
| R011 | Constant not starting with uppercase |
| R012 | Variable using camelCase instead of snake_case |
| R020 | Semicolon used to separate statements |
| R021 | Missing space around binary operator |
| R022 | Trailing comma before closing parenthesis |
| R023 | Too many consecutive blank lines (max 2) |
| R024 | Use `puts` instead of `p nil` |
| R030 | Unbalanced brackets/parentheses/braces |
| R031 | Missing `end` for block opener |
| R032 | Redundant `return` on last line of method |
| R040 | Method too long (> 30 lines) |
| R041 | Class too long (> 300 lines) |
| R042 | High cyclomatic complexity (> 10) |

## Example Output

```
examples/sample.rb
  1:1  warning  Missing `# frozen_string_literal: true` magic comment [R003]
    fix: # frozen_string_literal: true
  6:7  warning  Method name `badMethodName` should use snake_case [R010]
  7:15 warning  Missing space before `+` [R021]
  8:5  warning  Variable `myVar` should use snake_case instead of camelCase [R012]
  9:5  info     Redundant `return` on last line of method [R032]

0 errors 4 warnings 1 info in 1 file (2 ms)
```

## Architecture

```
src/
├── main.rs          CLI entry point (clap)
├── lexer.rs         Ruby tokenizer
├── linter.rs        Core linting engine
├── diagnostic.rs    Diagnostic types
├── reporter.rs      Output formatters
└── rules/
    ├── mod.rs       Rule trait + registry
    ├── line_length.rs
    ├── trailing_whitespace.rs
    ├── frozen_string_literal.rs
    ├── naming.rs
    ├── style.rs
    ├── syntax.rs
    └── complexity.rs
```
