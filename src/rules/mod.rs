mod complexity;
mod frozen_string_literal;
mod line_length;
mod missing_frozen_literal;
mod naming;
mod style;
mod syntax;
mod trailing_whitespace;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::lexer::Token;

pub use complexity::ComplexityRule;
pub use frozen_string_literal::FrozenStringLiteralRule;
pub use line_length::LineLengthRule;
pub use naming::NamingRule;
pub use style::StyleRule;
pub use syntax::SyntaxRule;
pub use trailing_whitespace::TrailingWhitespaceRule;

/// Context passed to each rule
pub struct LintContext<'a> {
    pub file: &'a str,
    pub source: &'a str,
    pub lines: &'a [&'a str],
    pub tokens: &'a [Token],
}

/// Trait implemented by every lint rule
pub trait Rule {
    fn name(&self) -> &'static str;
    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic>;
}

/// Returns all built-in rules configured with the given config
pub fn all_rules(config: &Config) -> Vec<Box<dyn Rule + Send + Sync>> {
    vec![
        Box::new(LineLengthRule {
            max_length: config.line_length,
        }),
        Box::new(TrailingWhitespaceRule),
        Box::new(FrozenStringLiteralRule),
        Box::new(NamingRule),
        Box::new(StyleRule),
        Box::new(SyntaxRule),
        Box::new(ComplexityRule {
            max_method_lines: config.max_method_lines,
            max_class_lines: config.max_class_lines,
            max_complexity: config.max_complexity,
        }),
    ]
}
