use crate::diagnostic::{Diagnostic, Severity};
use colored::Colorize;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
    Github, // GitHub Actions annotation format
    Sarif,  // SARIF v2.1.0 for GitHub Code Scanning
}

/// Static list of all known rules: (code, short description).
const ALL_RULES: &[(&str, &str)] = &[
    ("R001", "Line too long"),
    ("R002", "Trailing whitespace"),
    ("R003", "Missing frozen_string_literal magic comment"),
    ("R010", "Method name not in snake_case"),
    ("R011", "Constant not starting with uppercase"),
    ("R012", "Variable using camelCase instead of snake_case"),
    ("R013", "Class/module name not CamelCase"),
    ("R020", "Semicolon used to separate statements"),
    ("R021", "Missing space around operator"),
    ("R022", "Trailing comma before closing parenthesis"),
    ("R023", "Too many consecutive blank lines"),
    ("R024", "Use puts instead of p nil"),
    ("R025", "Missing final newline at end of file"),
    ("R026", "Missing blank line between method definitions"),
    ("R027", "Empty method body"),
    ("R028", "Use unless instead of if !condition"),
    ("R029", "Double negation"),
    ("R033", "Redundant self. on method call"),
    ("R030", "Unbalanced brackets/parentheses/braces"),
    ("R031", "Missing end for block"),
    ("R032", "Redundant return on last line of method"),
    ("R034", "Empty rescue body"),
    ("R035", "Unreachable code after return/raise"),
    ("R040", "Method too long"),
    ("R041", "Class too long"),
    ("R042", "High cyclomatic complexity"),
    ("R043", "Too many method parameters"),
    ("R050", "Usage of eval with string argument"),
    ("R051", "Hardcoded credentials"),
    ("R052", "Dynamic send/public_send"),
    ("R053", "Shell injection risk"),
    ("R054", "Unsafe deserialization"),
];

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
        // Determine the current working directory as a file:// URI with trailing slash
        let cwd_uri = std::env::current_dir()
            .map(|p| {
                let mut s = p.to_string_lossy().replace('\\', "/");
                if !s.starts_with('/') {
                    s.insert(0, '/');
                }
                if !s.ends_with('/') {
                    s.push('/');
                }
                format!("file://{}", s)
            })
            .unwrap_or_else(|_| "file:///".to_string());
        // Build driver.rules from the full static rule list
        let rules: Vec<serde_json::Value> = ALL_RULES
            .iter()
            .map(|(id, _desc)| {
                serde_json::json!({
                    "id": id,
                    "name": rule_name(id),
                    "shortDescription": { "text": rule_short_description(id) },
                    "helpUri": format!(
                        "https://github.com/satooshi/Rblint/blob/main/docs/rules/{}.md",
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
                            "informationUri": "https://github.com/satooshi/Rblint",
                            "rules": rules
                        }
                    },
                    "originalUriBaseIds": {
                        "%SRCROOT%": {
                            "uri": cwd_uri
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
        "R013" => "ClassModuleNameNotCamelCase",
        "R020" => "SemicolonSeparatedStatements",
        "R021" => "MissingSpaceAroundOperator",
        "R022" => "TrailingCommaBeforeClosingParen",
        "R023" => "TooManyConsecutiveBlankLines",
        "R024" => "UsePutsInsteadOfPNil",
        "R025" => "MissingFinalNewline",
        "R026" => "MissingBlankLineBetweenMethods",
        "R027" => "EmptyMethodBody",
        "R028" => "UseUnlessInsteadOfIfNot",
        "R029" => "DoubleNegation",
        "R030" => "UnbalancedBrackets",
        "R031" => "MissingEnd",
        "R032" => "RedundantReturn",
        "R033" => "RedundantSelf",
        "R034" => "EmptyRescueBody",
        "R035" => "UnreachableCode",
        "R040" => "MethodTooLong",
        "R041" => "ClassTooLong",
        "R042" => "HighCyclomaticComplexity",
        "R043" => "TooManyMethodParameters",
        "R050" => "EvalWithStringArgument",
        "R051" => "HardcodedCredentials",
        "R052" => "DynamicSend",
        "R053" => "ShellInjectionRisk",
        "R054" => "UnsafeDeserialization",
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
        "R013" => "Class/module name not CamelCase",
        "R020" => "Semicolon used to separate statements",
        "R021" => "Missing space around operator",
        "R022" => "Trailing comma before closing parenthesis",
        "R023" => "Too many consecutive blank lines",
        "R024" => "Use `puts` instead of `p nil`",
        "R025" => "Missing final newline at end of file",
        "R026" => "Missing blank line between method definitions",
        "R027" => "Empty method body",
        "R028" => "Use unless instead of if !condition",
        "R029" => "Double negation",
        "R030" => "Unbalanced brackets/parentheses/braces",
        "R031" => "Missing `end` for block",
        "R032" => "Redundant `return` on last line of method",
        "R033" => "Redundant self. on method call",
        "R034" => "Empty rescue body",
        "R035" => "Unreachable code after return/raise",
        "R040" => "Method too long",
        "R041" => "Class too long",
        "R042" => "High cyclomatic complexity",
        "R043" => "Too many method parameters",
        "R050" => "Usage of eval with string argument",
        "R051" => "Hardcoded credentials",
        "R052" => "Dynamic send/public_send",
        "R053" => "Shell injection risk",
        "R054" => "Unsafe deserialization",
        _ => "Unknown rule",
    }
}

/// Return the default SARIF level for a rule code.
fn rule_default_level(code: &str) -> &'static str {
    match code {
        // Errors
        "R030" | "R031" | "R050" | "R051" | "R052" | "R053" | "R054" => "error",
        // Notes / info
        "R003" | "R025" | "R027" | "R032" | "R033" => "note",
        // Everything else is a warning
        _ => "warning",
    }
}
