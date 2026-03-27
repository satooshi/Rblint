/// Ruby parser wrapper around lib-ruby-parser.
///
/// Provides a simple `parse()` function that converts Ruby source code
/// into an AST node, used by AST-based linting rules.
pub use lib_ruby_parser::Node;

use lib_ruby_parser::{ErrorLevel, Parser, ParserOptions};

/// Parse Ruby source code and return the root AST node.
///
/// Returns `None` when the parser produces no AST (e.g. empty source)
/// or when error-level diagnostics are present (syntax errors), to avoid
/// running AST rules on partial/invalid trees.
pub fn parse(source: &str) -> Option<Node> {
    let options = ParserOptions {
        buffer_name: "(input)".to_string(),
        record_tokens: false,
        ..Default::default()
    };
    let parser = Parser::new(source.as_bytes().to_vec(), options);
    let result = parser.do_parse();
    if result
        .diagnostics
        .iter()
        .any(|d| d.level == ErrorLevel::Error)
    {
        return None;
    }
    result.ast.map(|boxed| *boxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_assignment() {
        let node = parse("x = 1").expect("should parse assignment");
        match &node {
            Node::Lvasgn(_) => {} // expected
            other => panic!("expected Lvasgn, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_method_def() {
        let source = "def foo\n  42\nend";
        let node = parse(source).expect("should parse method def");
        match &node {
            Node::Def(_) => {} // expected
            other => panic!("expected Def, got {:?}", other),
        }
    }

    #[test]
    fn test_invalid_syntax_returns_none() {
        // Invalid syntax with error-level diagnostics must return None
        // to prevent AST rules from running on partial/invalid trees.
        let result = parse("def end end end @@@ !!!");
        assert!(result.is_none(), "parse with errors should return None");
    }

    #[test]
    fn test_parse_invalid_def_returns_none() {
        let result = parse("def def def");
        assert!(result.is_none(), "invalid def should return None");
    }

    #[test]
    fn test_empty_source() {
        let result = parse("");
        assert!(result.is_none(), "empty source should produce no AST");
    }
}
