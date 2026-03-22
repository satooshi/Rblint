use crate::diagnostic::{Diagnostic, Severity};
use colored::Colorize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
    Github, // GitHub Actions annotation format
    Sarif,  // SARIF v2.1.0 for GitHub Code Scanning
}

pub struct Reporter {
    pub format: OutputFormat,
    pub show_fixes: bool,
}

impl Reporter {
    pub fn print(&self, diags: &[Diagnostic]) {
        match self.format {
            OutputFormat::Text => self.print_text(diags),
            OutputFormat::Json => self.print_json(diags),
            OutputFormat::Github => self.print_github(diags),
            OutputFormat::Sarif => self.print_sarif(diags),
        }
    }

    fn print_text(&self, diags: &[Diagnostic]) {
        let mut current_file = "";
        for d in diags {
            if d.file != current_file {
                println!("\n{}", d.file.bold().underline());
                current_file = &d.file;
            }

            let loc = format!("{}:{}", d.line, d.col);
            let rule = format!("[{}]", d.rule).dimmed();
            let msg = match d.severity {
                Severity::Error => d.message.as_str().red().bold().to_string(),
                Severity::Warning => d.message.as_str().yellow().to_string(),
                Severity::Info => d.message.as_str().cyan().to_string(),
            };
            let sev = match d.severity {
                Severity::Error => "error  ".red().bold().to_string(),
                Severity::Warning => "warning".yellow().to_string(),
                Severity::Info => "info   ".cyan().to_string(),
            };

            println!("  {} {} {} {}", loc.dimmed(), sev, msg, rule);

            if self.show_fixes {
                if let Some(fix) = &d.fix {
                    println!("    {} {}", "fix:".green().bold(), fix.dimmed());
                }
            }
        }
    }

    fn print_json(&self, diags: &[Diagnostic]) {
        let json = serde_json::to_string_pretty(diags).unwrap_or_else(|_| "[]".to_string());
        println!("{}", json);
    }

    fn print_github(&self, diags: &[Diagnostic]) {
        for d in diags {
            let level = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "notice",
            };
            println!(
                "::{} file={},line={},col={},title={}::{}",
                level, d.file, d.line, d.col, d.rule, d.message
            );
        }
    }

    fn print_sarif(&self, diags: &[Diagnostic]) {
        // Collect unique rules in stable order (BTreeMap sorts by key)
        let mut rules_map: BTreeMap<&str, &str> = BTreeMap::new();
        for d in diags {
            rules_map.entry(d.rule).or_insert_with(|| rule_name(d.rule));
        }

        let rules: Vec<serde_json::Value> = rules_map
            .iter()
            .map(|(id, name)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "shortDescription": { "text": rule_short_description(id) },
                    "helpUri": format!(
                        "https://github.com/your-repo/rblint/blob/main/docs/rules/{}.md",
                        id
                    ),
                    "defaultConfiguration": {
                        "level": rule_default_level(id)
                    }
                })
            })
            .collect();

        let results: Vec<serde_json::Value> = diags
            .iter()
            .map(|d| {
                let level = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "note",
                };
                // SARIF requires forward slashes in URIs
                let uri = d.file.replace('\\', "/");
                serde_json::json!({
                    "ruleId": d.rule,
                    "level": level,
                    "message": { "text": d.message },
                    "locations": [
                        {
                            "physicalLocation": {
                                "artifactLocation": {
                                    "uri": uri,
                                    "uriBaseId": "%SRCROOT%"
                                },
                                "region": {
                                    "startLine": d.line,
                                    "startColumn": d.col
                                }
                            }
                        }
                    ]
                })
            })
            .collect();

        let sarif = serde_json::json!({
            "$schema": "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0-rtm.5.json",
            "version": "2.1.0",
            "runs": [
                {
                    "tool": {
                        "driver": {
                            "name": "rblint",
                            "version": env!("CARGO_PKG_VERSION"),
                            "informationUri": "https://github.com/your-repo/rblint",
                            "rules": rules
                        }
                    },
                    "results": results
                }
            ]
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&sarif).unwrap_or_else(|_| "{}".to_string())
        );
    }

    pub fn print_summary(&self, diags: &[Diagnostic], files_checked: usize, elapsed_ms: u128) {
        if self.format != OutputFormat::Text {
            return;
        }

        let errors = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        let infos = diags
            .iter()
            .filter(|d| d.severity == Severity::Info)
            .count();

        println!();
        println!(
            "{} {} {} in {} {} ({} ms)",
            format!("{} error{}", errors, if errors == 1 { "" } else { "s" })
                .red()
                .bold(),
            format!(
                "{} warning{}",
                warnings,
                if warnings == 1 { "" } else { "s" }
            )
            .yellow(),
            format!("{} info", infos).cyan(),
            files_checked.to_string().bold(),
            if files_checked == 1 { "file" } else { "files" },
            elapsed_ms,
        );

        if diags.is_empty() {
            println!("{}", "All checks passed!".green().bold());
        }
    }
}

/// Return a CamelCase name for a rule code.
fn rule_name(code: &str) -> &'static str {
    match code {
        "R001" => "LineTooLong",
        "R002" => "TrailingWhitespace",
        "R003" => "MissingFrozenStringLiteral",
        "R010" => "MethodNameNotSnakeCase",
        "R011" => "ConstantNotUppercase",
        "R012" => "VariableCamelCase",
        "R020" => "SemicolonSeparatedStatements",
        "R021" => "MissingSpaceAroundOperator",
        "R022" => "TrailingCommaBeforeClosingParen",
        "R023" => "TooManyConsecutiveBlankLines",
        "R024" => "UsePutsInsteadOfPNil",
        "R025" => "MissingFinalNewline",
        "R026" => "MissingBlankLineBetweenMethods",
        "R030" => "UnbalancedBrackets",
        "R031" => "MissingEnd",
        "R032" => "RedundantReturn",
        "R040" => "MethodTooLong",
        "R041" => "ClassTooLong",
        "R042" => "HighCyclomaticComplexity",
        _ => "UnknownRule",
    }
}

/// Return a short human-readable description for a rule code.
fn rule_short_description(code: &str) -> &'static str {
    match code {
        "R001" => "Line too long",
        "R002" => "Trailing whitespace",
        "R003" => "Missing frozen_string_literal magic comment",
        "R010" => "Method name not in snake_case",
        "R011" => "Constant not starting with uppercase",
        "R012" => "Variable using camelCase instead of snake_case",
        "R020" => "Semicolon used to separate statements",
        "R021" => "Missing space around operator",
        "R022" => "Trailing comma before closing parenthesis",
        "R023" => "Too many consecutive blank lines",
        "R024" => "Use `puts` instead of `p nil`",
        "R025" => "Missing final newline at end of file",
        "R026" => "Missing blank line between method definitions",
        "R030" => "Unbalanced brackets/parentheses/braces",
        "R031" => "Missing `end` for block",
        "R032" => "Redundant `return` on last line of method",
        "R040" => "Method too long",
        "R041" => "Class too long",
        "R042" => "High cyclomatic complexity",
        _ => "Unknown rule",
    }
}

/// Return the default SARIF level for a rule code.
fn rule_default_level(code: &str) -> &'static str {
    match code {
        // Errors
        "R030" | "R031" => "error",
        // Notes / info
        "R003" | "R025" | "R032" => "note",
        // Everything else is a warning
        _ => "warning",
    }
}
