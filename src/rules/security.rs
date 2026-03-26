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
            // Skip method definitions (`def eval`) and alias statements (`alias eval …`).
            let prev_meaningful = (0..i)
                .rev()
                .find(|&k| !matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline))
                .map(|k| &tokens[k]);
            let is_definition = matches!(prev_meaningful.map(|t| &t.kind), Some(TokenKind::Def))
                || prev_meaningful
                    .map(|t| t.kind == TokenKind::Ident && t.text == "alias")
                    .unwrap_or(false);
            if is_definition {
                continue;
            }

            // Must be followed by `(`, a string literal, or an identifier (bare call)
            let j = i + 1;
            let next_meaningful = (j..tokens.len())
                .find(|&k| !matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline))
                .map(|k| &tokens[k]);
            if matches!(
                next_meaningful.map(|t| &t.kind),
                Some(TokenKind::LParen)
                    | Some(TokenKind::StringLiteral)
                    | Some(TokenKind::Ident)
                    | Some(TokenKind::LShift) // heredoc: eval <<~RUBY
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
    let orig = name.as_bytes();
    let lower_bytes = lower.as_bytes();
    CRED_PATTERNS.iter().any(|p| {
        // Require word-boundary match. A boundary is one of:
        //   • start / end of string
        //   • underscore separator (snake_case)
        //   • camelCase transition: preceding char is lowercase and the first char of the
        //     match is uppercase in the original string (e.g. auth**T**oken)
        //   • camelCase end: the character after the match is uppercase in the original
        //     string (e.g. **token**Secret)
        // This prevents false positives like `tokenizer` or `secretary` while still
        // catching camelCase names like `authToken` and `accessToken`.
        let mut start = 0;
        while let Some(pos) = lower[start..].find(p) {
            let abs = start + pos;
            let end = abs + p.len();
            let before_ok = abs == 0
                || lower_bytes[abs - 1] == b'_'
                || (orig[abs - 1].is_ascii_lowercase() && orig[abs].is_ascii_uppercase());
            let after_ok = end == lower_bytes.len()
                || lower_bytes[end] == b'_'
                || orig[end].is_ascii_uppercase();
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

            // Skip method definitions (`def send`) and alias statements.
            let prev_meaningful = (0..i)
                .rev()
                .find(|&k| !matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline))
                .map(|k| &tokens[k]);
            let is_definition = matches!(prev_meaningful.map(|t| &t.kind), Some(TokenKind::Def))
                || prev_meaningful
                    .map(|t| t.kind == TokenKind::Ident && t.text == "alias")
                    .unwrap_or(false);
            if is_definition {
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
                // Symbol (`:name`) and string literals (`"name"`) are static — not dynamic
                let is_static = j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Symbol | TokenKind::StringLiteral);
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
                let is_static =
                    matches!(tokens[j].kind, TokenKind::Symbol | TokenKind::StringLiteral);
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

        // Source-level detection for backtick and %x{} / %x() literals.
        // Handles multi-line strings where `#{` may appear on a different line
        // than the opening delimiter.
        for (start_line, kind) in scan_shell_literals(ctx.source) {
            let msg: &str = if kind == "backtick" {
                "Backtick command with string interpolation is a shell injection risk — use array form of `system()` instead"
            } else {
                "`%x{...}` command with string interpolation is a shell injection risk"
            };
            diags.push(Diagnostic::new(
                ctx.file,
                start_line,
                1,
                "R053",
                msg,
                Severity::Warning,
            ));
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
                if first_arg_has_interpolation(tokens, i + 1, false) {
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
            if first_arg_has_interpolation(tokens, j + 1, true) {
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

/// Returns true when the effective command string argument contains `#{`
/// AND the call is in single-string form (i.e. no comma follows that argument).
///
/// Handles both paren form `method("#{x}")` and no-paren form `method "#{x}"`.
/// Returns false for multi-argument array forms (`method("#{x}", "y")`) because
/// those are passed directly as argv with no shell expansion.
///
/// Also handles the Ruby env-hash prefix convention:
///   `system({"LANG" => "en"}, "cmd #{x}")` — hash literal first arg is env vars
///   `Open3.capture3(env, "cmd #{x}")`      — ident first arg may be env hash var
/// In both cases the second argument is the actual shell command.
fn first_arg_has_interpolation(
    tokens: &[crate::lexer::Token],
    start: usize,
    allow_ident_env_skip: bool,
) -> bool {
    let mut j = start;
    while j < tokens.len() && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline) {
        j += 1;
    }
    if j >= tokens.len() {
        return false;
    }
    // Consume optional opening parenthesis.
    if tokens[j].kind == TokenKind::LParen {
        j += 1;
        while j < tokens.len()
            && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
        {
            j += 1;
        }
    }
    if j >= tokens.len() {
        return false;
    }

    // If the first argument looks like an env hash or env variable, skip it and
    // advance to the actual command argument.
    match tokens[j].kind {
        // Hash literal `{...}` — definitely an env hash.
        TokenKind::LBrace => {
            let mut depth = 1usize;
            j += 1;
            while j < tokens.len() && depth > 0 {
                match tokens[j].kind {
                    TokenKind::LBrace => depth += 1,
                    TokenKind::RBrace => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            // Require a separating comma before the command argument.
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len() || tokens[j].kind != TokenKind::Comma {
                return false;
            }
            j += 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
        }
        // Identifier or constant — may be an env hash variable (common with Open3).
        // Only skip if a comma follows immediately AND the caller has indicated that an
        // identifier env-hash prefix is plausible (i.e. Open3 / IO.popen calls).
        // For standalone system/exec/spawn, any extra argument already makes the call
        // argv-form (no shell expansion), so skipping here would produce false positives.
        TokenKind::Ident | TokenKind::Constant if allow_ident_env_skip => {
            let mut peek = j + 1;
            while peek < tokens.len()
                && matches!(
                    tokens[peek].kind,
                    TokenKind::Whitespace | TokenKind::Newline
                )
            {
                peek += 1;
            }
            if peek < tokens.len() && tokens[peek].kind == TokenKind::Comma {
                j = peek + 1;
                while j < tokens.len()
                    && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
                {
                    j += 1;
                }
            }
            // If no comma follows, j still points at the original first arg — fall through.
        }
        _ => {}
    }

    // j now points at the argument to inspect.
    if j >= tokens.len()
        || tokens[j].kind != TokenKind::StringLiteral
        || !tokens[j].text.contains("#{")
    {
        return false;
    }
    // Check whether a comma follows — if so, this is a multi-argument (argv) call
    // and no shell expansion occurs.
    let mut k = j + 1;
    while k < tokens.len() && matches!(tokens[k].kind, TokenKind::Whitespace | TokenKind::Newline) {
        k += 1;
    }
    if k < tokens.len() && tokens[k].kind == TokenKind::Comma {
        return false; // multi-arg form — safe
    }
    true
}

/// Scan raw source text for backtick and `%x()`/`%x{}` shell literals that
/// contain string interpolation (`#{`).  Returns `(line_number, kind)` pairs
/// where `kind` is `"backtick"` or `"percent_x"`.
///
/// This source-level scan correctly handles multi-line literals where `#{`
/// appears on a different line than the opening delimiter.
fn scan_shell_literals(source: &str) -> Vec<(usize, &'static str)> {
    let mut results = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut line = 1usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            // Outside a shell literal a bare `#` starts a comment — skip to EOL.
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Skip single-quoted strings (no interpolation inside).
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            i += 2;
                        }
                        b'\'' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
            }
            // Skip double-quoted strings (not a shell command; handled by token scan).
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            i += 2;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
            }
            // Backtick shell literal.
            b'`' => {
                let start_line = line;
                i += 1;
                let mut has_interp = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            i += 2;
                            continue;
                        }
                        b'`' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                            continue;
                        }
                        b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                            has_interp = true;
                            i += 1;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if has_interp {
                    results.push((start_line, "backtick"));
                }
            }
            // `%x(...)`, `%x{...}`, `%x[...]`, or `%x<...>` shell literal.
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1] == b'x'
                && matches!(bytes[i + 2], b'(' | b'{' | b'[' | b'<') =>
            {
                let open = bytes[i + 2];
                let close = match open {
                    b'(' => b')',
                    b'{' => b'}',
                    b'[' => b']',
                    b'<' => b'>',
                    _ => unreachable!(),
                };
                let start_line = line;
                i += 3;
                let mut depth = 1usize;
                let mut has_interp = false;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'\\' => {
                            i += 2;
                            continue;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                            continue;
                        }
                        b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                            has_interp = true;
                            // Fall through so the `{` is processed by the depth check below.
                            i += 1;
                        }
                        _ => {}
                    }
                    if bytes[i] == open {
                        depth += 1;
                    } else if bytes[i] == close {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    i += 1;
                }
                if has_interp {
                    results.push((start_line, "percent_x"));
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    results
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

            // Next non-whitespace/newline should be `.` or `::`
            let mut j = i + 1;
            while j < tokens.len()
                && matches!(tokens[j].kind, TokenKind::Whitespace | TokenKind::Newline)
            {
                j += 1;
            }
            if j >= tokens.len()
                || !matches!(tokens[j].kind, TokenKind::Dot | TokenKind::ColonColon)
            {
                i += 1;
                continue;
            }
            let sep = if tokens[j].kind == TokenKind::ColonColon {
                "::"
            } else {
                "."
            };

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
                    format!("`{receiver}{sep}load` deserializes arbitrary objects — {suggestion}"),
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

    // Bug 3: method definitions named `eval` must not be flagged
    #[test]
    fn no_violation_eval_def() {
        let src = "def eval(code)\n  # implementation\nend\n";
        assert!(
            !has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    #[test]
    fn no_violation_instance_eval_def() {
        let src = "def instance_eval(arg)\n  # ...\nend\n";
        assert!(
            !has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    #[test]
    fn no_violation_eval_alias() {
        // alias statement: `eval` appears as a name, not a call
        let src = "alias eval original_eval\n";
        assert!(
            !has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    // Bug: eval with heredoc argument must be flagged
    #[test]
    fn violation_eval_heredoc() {
        let src = "eval <<~RUBY\n  puts 1\nRUBY\n";
        assert!(
            has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
    }

    #[test]
    fn violation_instance_eval_heredoc() {
        let src = "obj.instance_eval <<~RUBY\n  puts 1\nRUBY\n";
        assert!(
            has_rule(&check_rule(&EvalUsageRule, src), "R050"),
            "{:?}",
            check_rule(&EvalUsageRule, src)
        );
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

    #[test]
    fn violation_camel_case_auth_token() {
        let src = "authToken = \"tok-abc123\"\n";
        assert!(
            has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn violation_camel_case_access_token() {
        let src = "accessToken = \"tok-abc123\"\n";
        assert!(
            has_rule(&check_rule(&HardcodedCredentialsRule, src), "R051"),
            "{:?}",
            check_rule(&HardcodedCredentialsRule, src)
        );
    }

    #[test]
    fn no_violation_tokenizer_still_not_credential() {
        let src = "tokenizer = \"abc123\"\n";
        assert!(!has_rule(
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

    // Bug: static string literal is not dynamic — must not be flagged
    #[test]
    fn no_violation_send_static_string_literal() {
        let src = "obj.send(\"foo\")\n";
        assert!(
            !has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_public_send_static_string_literal() {
        let src = "obj.public_send(\"bar\")\n";
        assert!(
            !has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_receiverless_send_static_string_no_parens() {
        let src = "send \"foo\"\n";
        assert!(
            !has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    // Bug: method definitions named `send` / `public_send` must not be flagged
    #[test]
    fn no_violation_send_def() {
        let src = "def send(method_name)\n  # impl\nend\n";
        assert!(
            !has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
    }

    #[test]
    fn no_violation_public_send_def() {
        let src = "def public_send(method_name)\n  # impl\nend\n";
        assert!(
            !has_rule(&check_rule(&DynamicSendRule, src), "R052"),
            "{:?}",
            check_rule(&DynamicSendRule, src)
        );
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

    // Bug 1: Open3 multi-arg form with interpolated first arg should NOT be flagged
    #[test]
    fn no_violation_open3_interpolated_first_arg_multiarg_form() {
        // "#{git_bin}" is first arg but multiple args → argv form, no shell expansion
        let src = "Open3.capture3(\"#{git_bin}\", \"show\", ref)\n";
        assert!(
            !has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_system_interpolated_first_arg_multiarg_form() {
        let src = "system(\"#{git_bin}\", \"show\", ref)\n";
        assert!(
            !has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    // Bug 2: multi-line %x with interpolation on a different line must be detected
    #[test]
    fn violation_percent_x_multiline_interpolation() {
        let src = "cmd = %x(\n  git show #{ref}\n)\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    // Bug: system/exec/spawn with a variable as first arg and interpolated string as
    // second arg is argv form — no shell expansion occurs, must NOT be flagged.
    #[test]
    fn no_violation_system_argv_form_var_command() {
        let src = "system(cmd, \"show #{ref}\")\n";
        assert!(
            !has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_exec_argv_form_var_command() {
        let src = "exec(cmd, \"--flag=#{val}\")\n";
        assert!(
            !has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    // Bug: %x[...] and %x<...> shell literals with interpolation must be detected
    #[test]
    fn violation_percent_x_bracket_interpolation() {
        let src = "out = %x[git show #{ref}]\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn violation_percent_x_angle_interpolation() {
        let src = "out = %x<ls #{dir}>\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_percent_x_bracket_no_interpolation() {
        let src = "out = %x[ls -la]\n";
        assert!(!has_rule(&check_rule(&ShellInjectionRule, src), "R053"));
    }

    // Bug: env hash / env variable as first arg must not hide dangerous second arg
    #[test]
    fn violation_system_env_hash_interpolated_command() {
        let src = "system({\"LANG\" => \"en_US\"}, \"rm -rf #{path}\")\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn violation_open3_env_var_interpolated_command() {
        let src = "Open3.capture3(env, \"git show #{ref}\")\n";
        assert!(
            has_rule(&check_rule(&ShellInjectionRule, src), "R053"),
            "{:?}",
            check_rule(&ShellInjectionRule, src)
        );
    }

    #[test]
    fn no_violation_system_env_hash_safe_command() {
        // Env hash present but no interpolation in command → safe
        let src = "system({\"LANG\" => \"en_US\"}, \"ls /tmp\")\n";
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

    // Bug: YAML::load and Marshal::load (ColonColon) must be detected
    #[test]
    fn violation_yaml_coloncolon_load() {
        let src = "data = YAML::load(input)\n";
        assert!(
            has_rule(&check_rule(&UnsafeDeserializationRule, src), "R054"),
            "{:?}",
            check_rule(&UnsafeDeserializationRule, src)
        );
    }

    #[test]
    fn violation_marshal_coloncolon_load() {
        let src = "obj = Marshal::load(data)\n";
        assert!(
            has_rule(&check_rule(&UnsafeDeserializationRule, src), "R054"),
            "{:?}",
            check_rule(&UnsafeDeserializationRule, src)
        );
    }
}
