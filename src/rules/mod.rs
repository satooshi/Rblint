mod complexity;
mod frozen_string_literal;
mod line_length;
mod missing_frozen_literal;
mod naming;
mod style;
mod syntax;
mod trailing_whitespace;

use std::cell::OnceCell;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::lexer::Token;
use crate::tree::{Node, TreeBuilder};

pub use complexity::ComplexityRule;
pub use frozen_string_literal::FrozenStringLiteralRule;
pub use line_length::LineLengthRule;
pub use naming::NamingRule;
pub use style::{
    BlankLinesRule, DoubleNegationRule, EmptyMethodRule, FinalNewlineRule, MethodSpacingRule,
    NegatedIfRule, OperatorSpacingRule, PNilRule, RedundantSelfRule, SemicolonRule,
    TrailingCommaRule,
};
pub use syntax::SyntaxRule;
pub use trailing_whitespace::TrailingWhitespaceRule;

/// Context passed to each rule.
///
/// Marked `#[non_exhaustive]` to prevent external code from constructing this
/// struct via struct-literal syntax. This is a **breaking change** in this
/// release — callers must use [`LintContext::new`] instead. The attribute also
/// ensures that adding new fields in future releases is non-breaking.
#[non_exhaustive]
pub struct LintContext<'a> {
    pub file: &'a str,
    pub source: &'a str,
    pub lines: &'a [&'a str],
    pub tokens: &'a [Token],
    nodes: OnceCell<Vec<Node>>,
}

impl<'a> LintContext<'a> {
    pub fn new(file: &'a str, source: &'a str, lines: &'a [&'a str], tokens: &'a [Token]) -> Self {
        Self {
            file,
            source,
            lines,
            tokens,
            nodes: OnceCell::new(),
        }
    }

    /// Returns the AST nodes, building them lazily on first access.
    pub fn nodes(&self) -> &[Node] {
        self.nodes.get_or_init(|| TreeBuilder::build(self.tokens))
    }
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
        Box::new(SemicolonRule),       // R020
        Box::new(OperatorSpacingRule), // R021
        Box::new(TrailingCommaRule),   // R022
        Box::new(BlankLinesRule),      // R023
        Box::new(PNilRule),            // R024
        Box::new(FinalNewlineRule),    // R025
        Box::new(MethodSpacingRule),   // R026
        Box::new(EmptyMethodRule),     // R027
        Box::new(NegatedIfRule),       // R028
        Box::new(DoubleNegationRule),  // R029
        Box::new(RedundantSelfRule),   // R033
        Box::new(SyntaxRule),
        Box::new(ComplexityRule {
            max_method_lines: config.max_method_lines,
            max_class_lines: config.max_class_lines,
            max_complexity: config.max_complexity,
            max_parameters: config.max_parameters,
        }),
    ]
}
