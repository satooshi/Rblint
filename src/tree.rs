/// Lightweight AST layer for Rblint
///
/// Builds a simplified tree of block-structured Ruby constructs from the
/// token stream produced by the lexer.  The goal is not a full parse —
/// it is good enough for structural rules (method/class length, cyclomatic
/// complexity) without duplicating depth-tracking logic in every rule.
use crate::lexer::{Token, TokenKind};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Kinds of AST nodes that the tree builder recognises.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Method,
    Class,
    Module,
    Block,
    If,
    Unless,
    While,
    Until,
    For,
    Case,
    Begin,
    Do,
}

/// A single node in the lightweight AST.
#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    /// 1-based line where the opening keyword appears.
    pub start_line: usize,
    /// 1-based line where the matching `end` (or the line itself for postfix
    /// forms) appears.
    pub end_line: usize,
    pub children: Vec<Node>,
    /// Method / class / module name, if available.
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// Tree builder
// ---------------------------------------------------------------------------

/// Builds a list of top-level [`Node`]s from a flat token slice.
pub struct TreeBuilder;

impl TreeBuilder {
    pub fn build(tokens: &[Token]) -> Vec<Node> {
        let mut idx = 0;
        let mut roots = Vec::new();
        Self::parse_level(tokens, &mut idx, &mut roots, 0);
        roots
    }

    /// Parse tokens at the current nesting level, appending nodes to `out`.
    /// `depth` tracks how many block-openers we are inside; when we see an
    /// `end` at depth 0 we return (the caller pops it).
    fn parse_level(tokens: &[Token], idx: &mut usize, out: &mut Vec<Node>, depth: usize) {
        while *idx < tokens.len() {
            let tok = &tokens[*idx];

            match &tok.kind {
                // ---- block terminators ----
                TokenKind::End => {
                    // Let the parent call consume the `end`.
                    return;
                }

                // ---- openers that need a matching `end` ----
                TokenKind::Def => {
                    let node = Self::parse_def(tokens, idx);
                    out.push(node);
                }
                TokenKind::Class => {
                    let node = Self::parse_keyed(tokens, idx, NodeKind::Class);
                    out.push(node);
                }
                TokenKind::Module => {
                    let node = Self::parse_keyed(tokens, idx, NodeKind::Module);
                    out.push(node);
                }
                TokenKind::Begin => {
                    let node = Self::parse_anonymous(tokens, idx, NodeKind::Begin);
                    out.push(node);
                }
                TokenKind::Case => {
                    let node = Self::parse_anonymous(tokens, idx, NodeKind::Case);
                    out.push(node);
                }
                TokenKind::Do => {
                    let node = Self::parse_anonymous(tokens, idx, NodeKind::Do);
                    out.push(node);
                }

                // ---- if / unless / while / until / for ----
                // These can be postfix (no matching `end`) or block forms.
                TokenKind::If => {
                    if let Some(node) = Self::parse_conditional(tokens, idx, NodeKind::If) {
                        out.push(node);
                    }
                }
                TokenKind::Unless => {
                    if let Some(node) = Self::parse_conditional(tokens, idx, NodeKind::Unless) {
                        out.push(node);
                    }
                }
                TokenKind::While => {
                    if let Some(node) = Self::parse_loop(tokens, idx, NodeKind::While) {
                        out.push(node);
                    }
                }
                TokenKind::Until => {
                    if let Some(node) = Self::parse_loop(tokens, idx, NodeKind::Until) {
                        out.push(node);
                    }
                }
                TokenKind::For => {
                    // `for` is always a block form in Ruby (needs `end`)
                    let node = Self::parse_anonymous(tokens, idx, NodeKind::For);
                    out.push(node);
                }

                TokenKind::Eof => return,

                _ => {
                    *idx += 1;
                }
            }

            // Safety: avoid infinite loops if something goes wrong.
            if *idx >= tokens.len() {
                break;
            }

            let _ = depth; // unused but kept for future guard
        }
    }

    // -----------------------------------------------------------------------
    // Helpers: determine whether a keyword is at the start of a statement
    // (= block form) or in the middle (= postfix / modifier form).
    // -----------------------------------------------------------------------

    /// Returns `true` if the token at `idx` is the first non-whitespace,
    /// non-comment token on its line — i.e. it starts a new statement.
    fn is_statement_start(tokens: &[Token], idx: usize) -> bool {
        // Walk backwards; if we hit the start of the slice or a Newline before
        // any "real" token, the keyword is at the start of the line.
        if idx == 0 {
            return true;
        }
        let mut j = idx;
        loop {
            if j == 0 {
                return true;
            }
            j -= 1;
            match &tokens[j].kind {
                TokenKind::Newline => return true,
                TokenKind::Whitespace | TokenKind::Comment => continue,
                // There is a real token before us on the same line.
                _ => return false,
            }
        }
    }

    // -----------------------------------------------------------------------
    // Parsers for specific constructs
    // -----------------------------------------------------------------------

    /// Parse a `def` block: handles one-liners (`def foo; bar; end` on one
    /// line) and normal multi-line methods.
    fn parse_def(tokens: &[Token], idx: &mut usize) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1; // consume `def`

        // Collect name (first non-whitespace token after `def`)
        let name = Self::next_name(tokens, *idx);

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children);

        Node {
            kind: NodeKind::Method,
            start_line,
            end_line,
            children,
            name: Some(name),
        }
    }

    /// Parse a named block opener (`class`, `module`).
    fn parse_keyed(tokens: &[Token], idx: &mut usize, kind: NodeKind) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1; // consume keyword

        let name = Self::next_name(tokens, *idx);

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children);

        Node {
            kind,
            start_line,
            end_line,
            children,
            name: Some(name),
        }
    }

    /// Parse an anonymous block (`begin`, `do`, `case`, `for`).
    fn parse_anonymous(tokens: &[Token], idx: &mut usize, kind: NodeKind) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1; // consume keyword

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children);

        Node {
            kind,
            start_line,
            end_line,
            children,
            name: None,
        }
    }

    /// Parse `if` / `unless`.  Returns `None` for postfix forms.
    fn parse_conditional(tokens: &[Token], idx: &mut usize, kind: NodeKind) -> Option<Node> {
        if !Self::is_statement_start(tokens, *idx) {
            // Postfix form — not a block, skip
            *idx += 1;
            return None;
        }

        let start_line = tokens[*idx].line;
        *idx += 1; // consume keyword

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children);

        Some(Node {
            kind,
            start_line,
            end_line,
            children,
            name: None,
        })
    }

    /// Parse `while` / `until`.  Returns `None` for postfix forms.
    fn parse_loop(tokens: &[Token], idx: &mut usize, kind: NodeKind) -> Option<Node> {
        if !Self::is_statement_start(tokens, *idx) {
            // Postfix modifier — not a block, skip
            *idx += 1;
            return None;
        }

        let start_line = tokens[*idx].line;
        *idx += 1; // consume keyword

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children);

        Some(Node {
            kind,
            start_line,
            end_line,
            children,
            name: None,
        })
    }

    // -----------------------------------------------------------------------
    // Core recursive descent helper
    // -----------------------------------------------------------------------

    /// Consume tokens up to (and including) the matching `end` at the current
    /// nesting level, recursively building child nodes.  Returns the line of
    /// the consumed `end` token.
    fn consume_until_end(tokens: &[Token], idx: &mut usize, children: &mut Vec<Node>) -> usize {
        // We call parse_level which stops when it sees an `end` at depth 0.
        Self::parse_level(tokens, idx, children, 1);

        // Consume the `end` token (or EOF).
        if *idx < tokens.len() {
            let end_line = tokens[*idx].line;
            if tokens[*idx].kind == TokenKind::End {
                *idx += 1;
            }
            end_line
        } else {
            // No matching end found — use last token's line.
            tokens.last().map(|t| t.line).unwrap_or(1)
        }
    }

    /// Extract the name from the token stream: the first non-whitespace token
    /// after position `from`.
    fn next_name(tokens: &[Token], from: usize) -> String {
        tokens
            .iter()
            .skip(from)
            .find(|t| !matches!(t.kind, TokenKind::Whitespace | TokenKind::Newline))
            .map(|t| t.text.clone())
            .unwrap_or_else(|| "<unknown>".into())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn build(src: &str) -> Vec<Node> {
        let tokens = Lexer::new(src).tokenize();
        TreeBuilder::build(&tokens)
    }

    // ---- basic def ----

    #[test]
    fn simple_method() {
        let nodes = build("def foo\n  x = 1\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].name.as_deref(), Some("foo"));
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn one_liner_method() {
        // `def foo; bar; end` on a single line
        let nodes = build("def foo; bar; end\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 1);
    }

    // ---- class / module ----

    #[test]
    fn simple_class() {
        let nodes = build("class Foo\n  def bar\n  end\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Class);
        assert_eq!(nodes[0].name.as_deref(), Some("Foo"));
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 4);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::Method);
    }

    #[test]
    fn simple_module() {
        let nodes = build("module M\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Module);
        assert_eq!(nodes[0].name.as_deref(), Some("M"));
    }

    // ---- if / unless ----

    #[test]
    fn block_if() {
        let src = "if cond\n  x\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::If);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn postfix_if_not_a_node() {
        // postfix: `x = 1 if cond` — the `if` is not at statement start
        let src = "x = 1 if cond\n";
        let nodes = build(src);
        // No If node — postfix forms are ignored
        assert!(
            nodes.iter().all(|n| n.kind != NodeKind::If),
            "postfix if should not produce a node"
        );
    }

    #[test]
    fn postfix_unless_not_a_node() {
        let src = "x = 1 unless cond\n";
        let nodes = build(src);
        assert!(nodes.iter().all(|n| n.kind != NodeKind::Unless));
    }

    #[test]
    fn postfix_while_not_a_node() {
        let src = "x += 1 while x < 10\n";
        let nodes = build(src);
        assert!(nodes.iter().all(|n| n.kind != NodeKind::While));
    }

    // ---- nesting ----

    #[test]
    fn nested_if_inside_method() {
        let src = "def foo\n  if x\n    y\n  end\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::If);
    }

    #[test]
    fn multiple_top_level_methods() {
        let src = "def foo\nend\ndef bar\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name.as_deref(), Some("foo"));
        assert_eq!(nodes[1].name.as_deref(), Some("bar"));
    }

    // ---- begin / case / do ----

    #[test]
    fn begin_block() {
        let src = "begin\n  x\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Begin);
    }

    #[test]
    fn case_block() {
        let src = "case x\nwhen 1\n  y\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Case);
    }
}
