use super::{LintContext, Rule};
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::TokenKind;

// ── R050: eval / instance_eval / class_eval ──────────────────────────────────

pub struct EvalUsageRule;

impl Rule for EvalUsageRule {
    fn name(&self) -> &'static str {
        "R050"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        const EVAL_NAMES: &[&str] = &["eval", "instance_eval", "class_eval", "module_eval"];

        for i in 0..tokens.len() {
            if tokens[i].kind != TokenKind::Ident {
                continue;
            }
            let name = tokens[i].text.as_str();
            if !EVAL_NAMES.contains(&name) {
                continue;
            }
            // Must be followed by `(`, a string literal, or an identifier (bare call)
            let j = i + 1;
            let next_meaningful = (j..tokens.len())
                .find(|&k| !matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline))
                .map(|k| &tokens[k]);
            if matches!(
                next_meaningful.map(|t| &t.kind),
                Some(TokenKind::LParen) | Some(TokenKind::StringLiteral) | Some(TokenKind::Ident)
            ) {
                diags.push(Diagnostic::new(
                    ctx.file,
                    tokens[i].line,
                    tokens[i].col,
                    "R050",
                    format!("`{name}` with a string argument is a security risk — avoid dynamic code evaluation"),
                    Severity::Warning,
                ));
            }
        }

        diags
    }
}

// ── R051: Hardcoded credentials ───────────────────────────────────────────────

pub struct HardcodedCredentialsRule;

/// Credential-related variable name fragments (case-insensitive).
const CRED_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "access_key",
    "auth_token",
    "token",
    "credential",
    "private_key",
    "database_url",
];

fn looks_like_credential_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    CRED_PATTERNS.iter().any(|p| {
        // Require word-boundary match: pattern must be preceded by start-of-string or `_`,
        // and followed by end-of-string or `_`. This prevents false positives such as
        // `tokenizer` matching `token` or `secretary` matching `secret`.
        let bytes = lower.as_bytes();
        let pat = p.as_bytes();
        let mut start = 0;
        while let Some(pos) = lower[start..].find(p) {
            let abs = start + pos;
            let end = abs + pat.len();
            let before_ok = abs == 0 || bytes[abs - 1] == b'_';
            let after_ok = end == bytes.len() || bytes[end] == b'_';
            if before_ok && after_ok {
                return true;
            }
            start = abs + 1;
        }
        false
    })
}

impl Rule for HardcodedCredentialsRule {
    fn name(&self) -> &'static str {
        "R051"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // Look for: `ident =` (or `ident=`) where ident matches credential names
        // and the RHS is a non-empty string literal
        let mut i = 0;
        while i + 2 < tokens.len() {
            if tokens[i].kind != TokenKind::Ident
                && tokens[i].kind != TokenKind::InstanceVar
                && tokens[i].kind != TokenKind::Constant
                && tokens[i].kind != TokenKind::ClassVar
            {
                i += 1;
                continue;
            }

            // Skip whitespace/newline to find `=`
            let mut j = i + 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len() || tokens[j].kind != TokenKind::Eq {
                i += 1;
                continue;
            }
            // Skip whitespace/newline to find RHS
            let mut k = j + 1;
            while k < tokens.len()
                && matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                k += 1;
            }
            if k >= tokens.len() || tokens[k].kind != TokenKind::StringLiteral {
                i += 1;
                continue;
            }
            // RHS is a string literal — check if non-empty
            let rhs_text = &tokens[k].text;
            // Skip empty strings and strings that look like placeholders
            if rhs_text == "\"\"" || rhs_text == "''" || rhs_text.len() <= 2 {
                i += 1;
                continue;
            }

            let var_name = tokens[i]
                .text
                .trim_start_matches('@')
                .trim_start_matches("@@");
            if looks_like_credential_name(var_name) {
                diags.push(Diagnostic::new(
                    ctx.file,
                    tokens[i].line,
                    tokens[i].col,
                    "R051",
                    format!("Possible hardcoded credential in `{var_name}` — use environment variables or a secrets manager instead"),
                    Severity::Warning,
                ));
            }
            i += 1;
        }

        diags
    }
}

// ── R052: send / public_send with dynamic argument ───────────────────────────

pub struct DynamicSendRule;

impl Rule for DynamicSendRule {
    fn name(&self) -> &'static str {
        "R052"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        for i in 0..tokens.len() {
            if tokens[i].kind != TokenKind::Ident {
                continue;
            }
            let name = tokens[i].text.as_str();
            if name != "send" && name != "public_send" {
                continue;
            }

            // Look at the first argument (with or without a receiver dot)
            let mut j = i + 1;
            while j < tokens.len() && tokens[j].kind == TokenKind::Whitespace {
                j += 1;
            }

            if j >= tokens.len() {
                continue;
            }

            if tokens[j].kind == TokenKind::LParen {
                // paren form: send(expr) or obj.send(expr)
                j += 1;
                while j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                {
                    j += 1;
                }
                // If the first argument is NOT a symbol (`:name`), it's dynamic
                let is_static = j < tokens.len() && tokens[j].kind == TokenKind::Symbol;
                if !is_static && j < tokens.len() && tokens[j].kind != TokenKind::RParen {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        tokens[i].line,
                        tokens[i].col,
                        "R052",
                        format!("`{name}` with a dynamic method name is a security risk — prefer explicit method calls"),
                        Severity::Warning,
                    ));
                }
            } else if tokens[j].kind != TokenKind::Newline && tokens[j].kind != TokenKind::Dot {
                // no-paren form: send expr or obj.send expr
                let is_static = tokens[j].kind == TokenKind::Symbol;
                if !is_static {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        tokens[i].line,
                        tokens[i].col,
                        "R052",
                        format!("`{name}` with a dynamic method name is a security risk — prefer explicit method calls"),
                        Severity::Warning,
                    ));
                }
            }
        }

        diags
    }
}

// ── R053: Shell injection via system() / backticks with interpolation ─────────

pub struct ShellInjectionRule;

impl Rule for ShellInjectionRule {
    fn name(&self) -> &'static str {
        "R053"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // Line-based detection for backtick, %x, IO.popen, Open3 (always single-string contexts)
        for (idx, line) in ctx.lines.iter().enumerate() {
            let line_no = idx + 1;
            let trimmed = line.trim();

            // Skip comment lines
            if trimmed.starts_with('#') {
                continue;
            }

            if !line.contains("#{") {
                continue;
            }

            // Backtick strings with interpolation
            if line.contains('`') {
                diags.push(Diagnostic::new(
                    ctx.file,
                    line_no,
                    1,
                    "R053",
                    "Backtick command with string interpolation is a shell injection risk — use array form of `system()` instead",
                    Severity::Warning,
                ));
                continue;
            }

            // `%x{...}` or `%x(...)` with interpolation
            if line.contains("%x{") || line.contains("%x(") {
                diags.push(Diagnostic::new(
                    ctx.file,
                    line_no,
                    1,
                    "R053",
                    "`%x{...}` command with string interpolation is a shell injection risk",
                    Severity::Warning,
                ));
                continue;
            }
        }

        // Token-based detection for system/exec/spawn/IO.popen/Open3.xxx:
        // handles both paren and no-paren forms, and correctly ignores array arguments
        // (only flags when the first string argument contains #{}).
        const SHELL_METHODS: &[&str] = &["system", "exec", "spawn"];
        for i in 0..tokens.len() {
            // ── Simple method calls: system, exec, spawn ──────────────────────
            if tokens[i].kind == TokenKind::Ident
                && SHELL_METHODS.contains(&tokens[i].text.as_str())
            {
                let name = tokens[i].text.as_str();
                if first_arg_has_interpolation(tokens, i + 1) {
                    diags.push(Diagnostic::new(
                        ctx.file,
                        tokens[i].line,
                        tokens[i].col,
                        "R053",
                        format!("`{name}` with interpolated string argument is a shell injection risk — use array form to avoid shell expansion"),
                        Severity::Warning,
                    ));
                }
                continue;
            }

            // ── Constant.method calls: IO.popen / Open3.xxx ───────────────────
            let is_io = tokens[i].kind == TokenKind::Constant && tokens[i].text == "IO";
            let is_open3 = tokens[i].kind == TokenKind::Constant && tokens[i].text == "Open3";
            if !is_io && !is_open3 {
                continue;
            }

            let mut j = i + 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len() || tokens[j].kind != TokenKind::Dot {
                continue;
            }

            j += 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len() || tokens[j].kind != TokenKind::Ident {
                continue;
            }

            // For IO, only popen passes a shell command string
            if is_io && tokens[j].text != "popen" {
                continue;
            }

            let method_name = format!("{}.{}", tokens[i].text, tokens[j].text);
            if first_arg_has_interpolation(tokens, j + 1) {
                diags.push(Diagnostic::new(
                    ctx.file,
                    tokens[i].line,
                    tokens[i].col,
                    "R053",
                    format!("`{method_name}` with interpolated string argument is a shell injection risk — use array form to avoid shell expansion"),
                    Severity::Warning,
                ));
            }
        }

        diags
    }
}

/// Returns true when the first string-literal argument starting at `start` contains `#{`.
/// Handles both paren form `method("#{x}")` and no-paren form `method "#{x}"`.
/// Correctly returns false for array forms like `method("safe", "#{x}")` because
/// the first argument `"safe"` does not contain `#{`.
fn first_arg_has_interpolation(tokens: &[crate::lexer::Token], start: usize) -> bool {
    let mut j = start;
    while j < tokens.len() && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline) {
        j += 1;
    }
    if j >= tokens.len() {
        return false;
    }
    if tokens[j].kind == TokenKind::LParen {
        j += 1;
        while j < tokens.len()
            && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
        {
            j += 1;
        }
    }
    j < tokens.len() && tokens[j].kind == TokenKind::StringLiteral && tokens[j].text.contains("#{")
}

// ── R054: Unsafe deserialization (Marshal.load / YAML.load) ──────────────────

pub struct UnsafeDeserializationRule;

impl Rule for UnsafeDeserializationRule {
    fn name(&self) -> &'static str {
        "R054"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let tokens = ctx.tokens;

        // Look for `Marshal` `.` `load` or `YAML` `.` `load` (not `safe_load`)
        let mut i = 0;
        while i + 2 < tokens.len() {
            let is_marshal = tokens[i].kind == TokenKind::Constant && tokens[i].text == "Marshal";
            let is_yaml = tokens[i].kind == TokenKind::Constant && tokens[i].text == "YAML";

            if !is_marshal && !is_yaml {
                i += 1;
                continue;
            }

            // Next non-whitespace/newline should be `.`
            let mut j = i + 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len() || tokens[j].kind != TokenKind::Dot {
                i += 1;
                continue;
            }

            // Next non-whitespace/newline should be `load` (but NOT `safe_load`)
            let mut k = j + 1;
            while k < tokens.len()
                && matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                k += 1;
            }
            if k >= tokens.len() || tokens[k].kind != TokenKind::Ident {
                i += 1;
                continue;
            }
            let method = tokens[k].text.as_str();
            if method == "load" {
                let receiver = &tokens[i].text;
                let suggestion = if is_yaml {
                    "use `YAML.safe_load` instead"
                } else {
                    "avoid deserializing untrusted data with `Marshal.load`"
                };
                diags.push(Diagnostic::new(
                    ctx.file,
                    tokens[i].line,
                    tokens[i].col,
                    "R054",
                    format!("`{receiver}.load` deserializes arbitrary objects — {suggestion}"),
                    Severity::Warning,
                ));
            }

            i += 1;
        }

        diags
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn check_rule<R: Rule>(rule: &R, source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        rule.check(&ctx)
    }

    fn has_rule(diags: &[Diagnostic], rule: &str) -> bool {
        diags.iter().any(|d| d.rule == rule)
    }

    // --- R050: eval ---

    #[test]
    fn violation_eval() {
        let src = "eval(user_input)\n";
        assert!(
            has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    #[test]
    fn violation_instance_eval() {
        let src = "obj.instance_eval(code)\n";
        assert!(has_rule(&check_rule(&EvalUsageRule, src), "R050"));
    }

    #[test]
    fn violation_eval_bare_call() {
        let src = "eval user_input\n";
        assert!(
            has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    #[test]
    fn violation_instance_eval_bare_call() {
        let src = "obj.instance_eval code\n";
        assert!(has_rule(&check_rule(&EvalUsageRule, src), "R050"));
    }

    #[test]
    fn no_violation_eval_identifier() {
        // `eval` used as a variable name should not trigger
        let src = "result = some_eval\n";
        assert!(!has_rule(&check_rule(&EvalUsageRule, src), "R050"));
    }

    // --- R051: hardcoded credentials ---

    #[test]
    fn violation_hardcoded_password() {
        let src = "password = \"secret123\"\n";
        assert!(
            has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn violation_hardcoded_api_key() {
        let src = "api_key = \"sk-1234567890abcdef\"\n";
        assert!(has_rule(
            &check_rule(&HardcodedCredentialsRule, src),
            "R051"
        ));
    }

    #[test]
    fn no_violation_empty_password() {
        let src = "password = \"\"\n";
        assert!(!has_rule(
            &check_rule(&HardcodedCredentialsRule, src),
            "R051"
        ));
    }

    #[test]
    fn violation_hardcoded_class_variable_password() {
        // Class variables (@@password) should also be detected
        let src = "@@password = \"secret123\"\n";
        assert!(
            has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn violation_hardcoded_password_multiline() {
        let src = "password =\n  \"secret123\"\n";
        assert!(
            has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn no_violation_unrelated_variable() {
        let src = "username = \"admin\"\n";
        assert!(!has_rule(
            &check_rule(&HardcodedCredentialsRule, src),
            "R051"
        ));
    }

    #[test]
    fn no_violation_tokenizer_not_token() {
        // "tokenizer" contains "token" as a substring but not as a whole word
        let src = "tokenizer = \"abc123\"\n";
        assert!(
            !has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn no_violation_secretary_not_secret() {
        // "secretary" contains "secret" as a substring but not as a whole word
        let src = "secretary = \"abc123\"\n";
        assert!(
            !has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn violation_access_token_word_boundary() {
        // "access_token" should still match because "token" is a full word segment
        let src = "access_token = \"sk-abc123\"\n";
        assert!(has_rule(
            &check_rule(&HardcodedCredentialsRule, src),
            "R051"
        ));
    }

    // --- R052: dynamic send ---

    #[test]
    fn violation_dynamic_send() {
        let src = "obj.send(method_name)\n";
        assert!(
            has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_symbol_send() {
        let src = "obj.send(:foo)\n";
        assert!(!has_rule(&check_rule(&DynamicSendRule, src), "R052"));
    }

    #[test]
    fn violation_send_without_parens() {
        let src = "obj.send method_name\n";
        assert!(
            has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_send_without_parens_symbol() {
        let src = "obj.send :foo\n";
        assert!(!has_rule(&check_rule(&DynamicSendRule, src), "R052"));
    }

    #[test]
    fn violation_receiverless_send() {
        let src = "send(method_name)\n";
        assert!(
            has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_receiverless_send_symbol() {
        let src = "send(:foo)\n";
        assert!(!has_rule(&check_rule(&DynamicSendRule, src), "R052"));
    }

    #[test]
    fn violation_receiverless_send_no_parens() {
        let src = "send method_name\n";
        assert!(
            has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_receiverless_send_no_parens_symbol() {
        let src = "send :foo\n";
        assert!(!has_rule(&check_rule(&DynamicSendRule, src), "R052"));
    }

    #[test]
    fn no_violation_send_multiline_symbol() {
        // obj.send(\n  :foo) should NOT be flagged — symbol arg, just wrapped
        let src = "obj.send(\n  :foo)\n";
        assert!(!has_rule(&check_rule(&DynamicSendRule, src), "R052"));
    }

    // --- R053: shell injection ---

    #[test]
    fn violation_backtick_with_interpolation() {
        let src = "result = `ls #{dir}`\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn violation_system_with_interpolation() {
        let src = "system(\"rm -rf #{path}\")\n";
        assert!(has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    #[test]
    fn no_violation_backtick_without_interpolation() {
        let src = "result = `ls -la`\n";
        assert!(!has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    #[test]
    fn violation_system_no_paren_with_interpolation() {
        let src = "system \"rm -rf #{path}\"\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_system_array_form() {
        // Array form: interpolation is in a non-first argument, so no shell expansion risk
        let src = "system(\"rm\", \"-rf\", \"#{path}\")\n";
        assert!(!has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    #[test]
    fn violation_open3_interpolated_string() {
        // Single-string form: shell expansion occurs
        let src = "Open3.capture3(\"git show #{ref}\")\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_open3_array_form() {
        // Array form: no shell expansion, safe call
        let src = "Open3.capture3(\"git\", \"show\", \"#{ref}\")\n";
        assert!(
            !has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn violation_io_popen_interpolated_string() {
        let src = "IO.popen(\"ls #{dir}\")\n";
        assert!(has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    #[test]
    fn no_violation_io_popen_array_form() {
        let src = "IO.popen([\"ls\", \"#{dir}\"])\n";
        assert!(!has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    // --- R054: unsafe deserialization ---

    #[test]
    fn violation_marshal_load() {
        let src = "obj = Marshal.load(data)\n";
        assert!(
            has_rule(&check_rule(&UnsafeDeserializationRule, src), "R054"),
            "{:?}",
            check_rule(&UnsafeDeserializationRule, src)
        );
    }

    #[test]
    fn violation_yaml_load() {
        let src = "data = YAML.load(input)\n";
        assert!(has_rule(
            &check_rule(&UnsafeDeserializationRule, src),
            "R054"
        ));
    }

    #[test]
    fn no_violation_yaml_safe_load() {
        let src = "data = YAML.safe_load(input)\n";
        assert!(!has_rule(
            &check_rule(&UnsafeDeserializationRule, src),
            "R054"
        ));
    }

    #[test]
    fn violation_yaml_load_multiline() {
        let src = "data = YAML.\n  load(input)\n";
        assert!(
            has_rule(&check_rule(&UnsafeDeserializationRule, src), "R054"),
            "{:?}",
            check_rule(&UnsafeDeserializationRule, src)
        );
    }

    #[test]
    fn violation_marshal_load_multiline() {
        let src = "obj = Marshal.\n  load(data)\n";
        assert!(
            has_rule(&check_rule(&UnsafeDeserializationRule, src), "R054"),
            "{:?}",
            check_rule(&UnsafeDeserializationRule, src)
        );
    }
}
