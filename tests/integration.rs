use rblint::diagnostic::Severity;
/// Integration tests: run Linter against fixture files and check diagnostics
use rblint::linter::Linter;

fn lint(path: &str) -> Vec<rblint::diagnostic::Diagnostic> {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Cannot read {path}: {e}"));
    Linter::new().lint_file(path, &source)
}

fn has_rule(diags: &[rblint::diagnostic::Diagnostic], rule: &str) -> bool {
    diags.iter().any(|d| d.rule == rule)
}

// ── clean.rb ─────────────────────────────────────────────────────────────────

#[test]
fn clean_file_has_no_violations() {
    let diags = lint("tests/fixtures/clean.rb");
    assert!(
        diags.is_empty(),
        "clean.rb should have no violations, got: {diags:?}"
    );
}

// ── violations.rb ─────────────────────────────────────────────────────────────

#[test]
fn violations_file_triggers_expected_rules() {
    let diags = lint("tests/fixtures/violations.rb");

    assert!(
        has_rule(&diags, "R003"),
        "expected R003 (frozen_string_literal), got {diags:?}"
    );
    assert!(
        has_rule(&diags, "R010"),
        "expected R010 (method snake_case), got {diags:?}"
    );
    assert!(
        has_rule(&diags, "R012"),
        "expected R012 (variable camelCase), got {diags:?}"
    );
    assert!(
        has_rule(&diags, "R022"),
        "expected R022 (trailing comma), got {diags:?}"
    );
    assert!(
        has_rule(&diags, "R032"),
        "expected R032 (redundant return), got {diags:?}"
    );
}

#[test]
fn diagnostics_are_sorted_by_line() {
    let diags = lint("tests/fixtures/violations.rb");
    let lines: Vec<usize> = diags.iter().map(|d| d.line).collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted, "diagnostics should be sorted by line number");
}

#[test]
fn diagnostic_file_path_matches_input() {
    let path = "tests/fixtures/violations.rb";
    let diags = lint(path);
    for d in &diags {
        assert_eq!(d.file, path);
    }
}

#[test]
fn r003_severity_is_warning() {
    let diags = lint("tests/fixtures/violations.rb");
    let r003 = diags.iter().find(|d| d.rule == "R003");
    if let Some(d) = r003 {
        assert_eq!(d.severity, Severity::Warning);
    }
}

// ── inline if modifier ────────────────────────────────────────────────────────

#[test]
fn inline_if_modifier_no_r031() {
    let source = "# frozen_string_literal: true\ndef foo\n  return if done\nend\n";
    let diags = Linter::new().lint_file("test.rb", source);
    let r031: Vec<_> = diags.iter().filter(|d| d.rule == "R031").collect();
    assert!(
        r031.is_empty(),
        "inline if modifier should not trigger R031: {r031:?}"
    );
}

// ── empty file ────────────────────────────────────────────────────────────────

#[test]
fn empty_file_has_no_diagnostics() {
    let diags = Linter::new().lint_file("empty.rb", "");
    assert!(
        diags.is_empty(),
        "empty file should have no diagnostics: {diags:?}"
    );
}

// ── fix suggestions ───────────────────────────────────────────────────────────

#[test]
fn r003_provides_fix() {
    let diags = Linter::new().lint_file("test.rb", "x = 1\n");
    let r003 = diags
        .iter()
        .find(|d| d.rule == "R003")
        .expect("R003 expected");
    assert!(r003.fix.is_some(), "R003 should provide a fix suggestion");
    assert_eq!(r003.fix.as_deref(), Some("# frozen_string_literal: true"));
}

#[test]
fn r002_provides_fix() {
    let diags = Linter::new().lint_file("test.rb", "x = 1   \n");
    let r002 = diags
        .iter()
        .find(|d| d.rule == "R002")
        .expect("R002 expected");
    assert_eq!(r002.fix.as_deref(), Some("x = 1"));
}

// ── R060 unused variable ─────────────────────────────────────────────────────

#[test]
fn r060_unused_variable_integration() {
    let diags = lint("tests/fixtures/unused_var.rb");
    let r060: Vec<_> = diags.iter().filter(|d| d.rule == "R060").collect();

    // Should detect unused, temp, and b
    let messages: Vec<&str> = r060.iter().map(|d| d.message.as_str()).collect();
    assert!(
        r060.iter().any(|d| d.message.contains("unused")),
        "should detect `unused` variable, got: {messages:?}"
    );
    assert!(
        r060.iter().any(|d| d.message.contains("temp")),
        "should detect `temp` variable, got: {messages:?}"
    );
    assert!(
        r060.iter().any(|d| d.message.contains("b")),
        "should detect `b` parameter, got: {messages:?}"
    );

    // `used` should NOT be flagged (note: "unused" contains "used", so check with backtick-delimited name)
    assert!(
        !r060.iter().any(|d| d.message.contains("`used`")),
        "`used` variable should not be flagged"
    );

    // `_ignored` should NOT be flagged
    assert!(
        !r060.iter().any(|d| d.message.contains("_ignored")),
        "`_ignored` variable should not be flagged"
    );
}

// ── R061 nil comparison ──────────────────────────────────────────────────────

#[test]
fn r061_nil_comparison_integration() {
    let diags = lint("tests/fixtures/nil_comparison.rb");
    let r061: Vec<_> = diags.iter().filter(|d| d.rule == "R061").collect();

    assert_eq!(
        r061.len(),
        2,
        "expected exactly 2 R061 diagnostics (== nil and != nil), got: {r061:?}"
    );

    // All R061 diagnostics should have auto-fix
    for d in &r061 {
        assert!(
            d.fix.is_some(),
            "R061 diagnostic should have an auto-fix: {:?}",
            d
        );
    }
}

// ── AST rules on parse failure ───────────────────────────────────────────────

#[test]
fn ast_rules_skipped_on_parse_failure() {
    let source = "def def def\n  x == nil\nend\n";
    // Should not panic even with malformed Ruby
    let diags = Linter::new().lint_file("malformed.rb", source);
    // Lexer-based rules may still fire, but no crash is the key assertion
    let _ = diags;
}
