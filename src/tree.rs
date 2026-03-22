/// Lightweight AST layer for Rblint.
///
/// Builds a simplified tree of block-structured Ruby constructs from the
/// token stream produced by the lexer.  The goal is not a full parse —
/// it is good enough for structural rules (method/class length, cyclomatic
/// complexity) without duplicating depth-tracking logic in every rule.
use crate::lexer::{Token, TokenKind};

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
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    /// 1-based line where the opening keyword appears.
    pub start_line: usize,
    /// 1-based line where the matching `end` appears.
    pub end_line: usize,
    pub children: Vec<Node>,
    pub name: Option<String>,
}

/// Builds a list of top-level [`Node`]s from a flat token slice.
pub struct TreeBuilder;

impl TreeBuilder {
    pub fn build(tokens: &[Token]) -> Vec<Node> {
        let stmt_starts = Self::precompute_statement_starts(tokens);
        let mut idx = 0;
        let mut roots = Vec::new();
        Self::parse_level(tokens, &mut idx, &mut roots, &stmt_starts, true);
        roots
    }

    /// Pre-compute which token indices are statement starts (first
    /// non-whitespace/comment token on their line) for O(1) lookup.
    fn precompute_statement_starts(tokens: &[Token]) -> Vec<bool> {
        let mut result = vec![false; tokens.len()];
        let mut at_line_start = true;
        for (i, tok) in tokens.iter().enumerate() {
            match tok.kind {
                TokenKind::Newline => {
                    at_line_start = true;
                }
                // A semicolon acts like a statement separator: the next
                // non-whitespace token is treated as a statement start.
                TokenKind::Semicolon => {
                    at_line_start = true;
                }
                TokenKind::Whitespace | TokenKind::Comment => {}
                _ => {
                    if at_line_start {
                        result[i] = true;
                    }
                    at_line_start = false;
                }
            }
        }
        result
    }

    fn parse_level(
        tokens: &[Token],
        idx: &mut usize,
        out: &mut Vec<Node>,
        stmt_starts: &[bool],
        is_root: bool,
    ) {
        while *idx < tokens.len() {
            let tok = &tokens[*idx];

            match &tok.kind {
                TokenKind::End => {
                    if is_root {
                        // Unexpected/extra `end` at root level — skip to recover
                        *idx += 1;
                        continue;
                    }
                    return;
                }

                TokenKind::Def => out.push(Self::parse_def(tokens, idx, stmt_starts)),
                TokenKind::Class => {
                    out.push(Self::parse_keyed(tokens, idx, NodeKind::Class, stmt_starts))
                }
                TokenKind::Module => out.push(Self::parse_keyed(
                    tokens,
                    idx,
                    NodeKind::Module,
                    stmt_starts,
                )),
                TokenKind::Begin => out.push(Self::parse_anonymous(
                    tokens,
                    idx,
                    NodeKind::Begin,
                    stmt_starts,
                )),
                TokenKind::Case => out.push(Self::parse_anonymous(
                    tokens,
                    idx,
                    NodeKind::Case,
                    stmt_starts,
                )),
                // Standalone `do...end` blocks (e.g., method call blocks like
                // `items.each do ... end`) are modeled as Block nodes so that
                // their `end` does not prematurely close the enclosing construct.
                // The optional `do` in `while/for ... do ... end` is consumed
                // inside `consume_until_end` before reaching this arm.
                TokenKind::Do => out.push(Self::parse_anonymous(
                    tokens,
                    idx,
                    NodeKind::Block,
                    stmt_starts,
                )),

                // `for` is always a block form in Ruby (needs `end`)
                TokenKind::For => out.push(Self::parse_anonymous(
                    tokens,
                    idx,
                    NodeKind::For,
                    stmt_starts,
                )),

                TokenKind::If | TokenKind::Unless | TokenKind::While | TokenKind::Until => {
                    let kind = match tok.kind {
                        TokenKind::If => NodeKind::If,
                        TokenKind::Unless => NodeKind::Unless,
                        TokenKind::While => NodeKind::While,
                        TokenKind::Until => NodeKind::Until,
                        _ => unreachable!(),
                    };
                    if let Some(node) = Self::parse_block_or_postfix(tokens, idx, kind, stmt_starts)
                    {
                        out.push(node);
                    }
                }

                TokenKind::Eof => return,

                _ => {
                    *idx += 1;
                }
            }

            if *idx >= tokens.len() {
                break;
            }
        }
    }

    fn parse_def(tokens: &[Token], idx: &mut usize, stmt_starts: &[bool]) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1;
        let name = Self::next_name(tokens, *idx);
        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children, stmt_starts);

        Node {
            kind: NodeKind::Method,
            start_line,
            end_line,
            children,
            name: Some(name),
        }
    }

    fn parse_keyed(
        tokens: &[Token],
        idx: &mut usize,
        kind: NodeKind,
        stmt_starts: &[bool],
    ) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1;
        let name = Self::next_name(tokens, *idx);
        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children, stmt_starts);

        Node {
            kind,
            start_line,
            end_line,
            children,
            name: Some(name),
        }
    }

    fn parse_anonymous(
        tokens: &[Token],
        idx: &mut usize,
        kind: NodeKind,
        stmt_starts: &[bool],
    ) -> Node {
        let start_line = tokens[*idx].line;
        *idx += 1;

        // For while/for/until, a `do` keyword on the same line is just a
        // separator — skip it so it doesn't open a spurious Block node.
        if matches!(kind, NodeKind::While | NodeKind::Until | NodeKind::For) {
            Self::skip_separator_do(tokens, idx, start_line);
        }

        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children, stmt_starts);

        Node {
            kind,
            start_line,
            end_line,
            children,
            name: None,
        }
    }

    /// If the next non-whitespace/comment token on `kw_line` is `do`, skip it.
    /// This handles the optional `do` separator in `while cond do`, `for x in
    /// xs do`, and `until cond do`.
    fn skip_separator_do(tokens: &[Token], idx: &mut usize, kw_line: usize) {
        let mut j = *idx;
        while j < tokens.len() {
            let t = &tokens[j];
            if t.line != kw_line {
                break;
            }
            match t.kind {
                TokenKind::Whitespace | TokenKind::Comment => {
                    j += 1;
                }
                TokenKind::Do => {
                    // Found the separator `do` — advance past it.
                    *idx = j + 1;
                    return;
                }
                _ => {
                    // Some other token on the same line — not a separator `do`.
                    j += 1;
                }
            }
        }
    }

    /// Parse `if`/`unless`/`while`/`until`. Returns `None` for postfix forms.
    ///
    /// Postfix forms (e.g. `x = 1 if cond`) appear mid-line and have no
    /// matching `end`, so we return `None`.  Non-statement-start block forms
    /// like `x = if cond ... end` *do* have a matching `end` — we must
    /// consume it (via `consume_until_end`) to keep nesting correct, but we
    /// still return `None` because these expression-position blocks are not
    /// meaningful for the structural rules this AST is designed for.
    ///
    /// Heuristic: if the keyword is followed (on the same line, ignoring
    /// whitespace/comments) by a newline/EOF/`;`, it is a block form and
    /// needs its `end` consumed.  Otherwise it is postfix.
    fn parse_block_or_postfix(
        tokens: &[Token],
        idx: &mut usize,
        kind: NodeKind,
        stmt_starts: &[bool],
    ) -> Option<Node> {
        let is_stmt_start = stmt_starts[*idx];

        if !is_stmt_start {
            // Determine whether this keyword is a block form (expression-
            // position, e.g. `x = if cond ... end`) or a true postfix
            // modifier (e.g. `x = 1 if cond`).
            //
            // Heuristic: look at the token immediately before the keyword
            // (skipping whitespace).  If it is an operator/assignment/open-
            // bracket, the keyword is in expression position (block form).
            // Otherwise it follows a value and is a postfix modifier.
            let looks_like_block = Self::preceded_by_operator(tokens, *idx);

            if looks_like_block {
                // Consume the matching `end` so it doesn't break the
                // enclosing nesting, but don't emit a Node.
                let kw_line_for_do = tokens[*idx].line;
                *idx += 1;
                if matches!(kind, NodeKind::While | NodeKind::Until) {
                    Self::skip_separator_do(tokens, idx, kw_line_for_do);
                }
                let mut children = Vec::new();
                Self::consume_until_end(tokens, idx, &mut children, stmt_starts);
                return None;
            }

            *idx += 1;
            return None;
        }

        let start_line = tokens[*idx].line;
        *idx += 1;
        // Skip separator `do` for while/until block forms.
        if matches!(kind, NodeKind::While | NodeKind::Until) {
            Self::skip_separator_do(tokens, idx, start_line);
        }
        let mut children = Vec::new();
        let end_line = Self::consume_until_end(tokens, idx, &mut children, stmt_starts);

        Some(Node {
            kind,
            start_line,
            end_line,
            children,
            name: None,
        })
    }

    fn consume_until_end(
        tokens: &[Token],
        idx: &mut usize,
        children: &mut Vec<Node>,
        stmt_starts: &[bool],
    ) -> usize {
        Self::parse_level(tokens, idx, children, stmt_starts, false);

        if *idx < tokens.len() {
            let end_line = tokens[*idx].line;
            if tokens[*idx].kind == TokenKind::End {
                *idx += 1;
            }
            end_line
        } else {
            tokens.last().map(|t| t.line).unwrap_or(1)
        }
    }

    /// Returns `true` if the token at `idx` is preceded (skipping whitespace)
    /// by an operator, assignment, open bracket, or keyword that puts the
    /// following expression in value position — indicating a block form
    /// rather than a postfix modifier.
    fn preceded_by_operator(tokens: &[Token], idx: usize) -> bool {
        let mut j = idx;
        while j > 0 {
            j -= 1;
            match tokens[j].kind {
                TokenKind::Whitespace | TokenKind::Comment => continue,
                // Operators / assignments that expect a value on the right
                TokenKind::Eq
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
                | TokenKind::AndEq
                | TokenKind::OrEq
                | TokenKind::FatArrow
                | TokenKind::Arrow
                | TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Colon
                | TokenKind::Return
                | TokenKind::Yield
                | TokenKind::And2
                | TokenKind::Or2
                | TokenKind::Bang
                | TokenKind::Not => return true,
                _ => return false,
            }
        }
        // Beginning of file — treat as statement start (block form)
        true
    }

    fn next_name(tokens: &[Token], from: usize) -> String {
        tokens
            .iter()
            .skip(from)
            .find(|t| {
                !matches!(
                    t.kind,
                    TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment
                )
            })
            .map(|t| t.text.clone())
            .unwrap_or_else(|| "<unknown>".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn build(src: &str) -> Vec<Node> {
        let tokens = Lexer::new(src).tokenize();
        TreeBuilder::build(&tokens)
    }

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
        let nodes = build("def foo; bar; end\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 1);
    }

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

    #[test]
    fn block_if() {
        let nodes = build("if cond\n  x\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::If);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn postfix_if_not_a_node() {
        let nodes = build("x = 1 if cond\n");
        assert!(
            nodes.iter().all(|n| n.kind != NodeKind::If),
            "postfix if should not produce a node"
        );
    }

    #[test]
    fn postfix_unless_not_a_node() {
        let nodes = build("x = 1 unless cond\n");
        assert!(nodes.iter().all(|n| n.kind != NodeKind::Unless));
    }

    #[test]
    fn postfix_while_not_a_node() {
        let nodes = build("x += 1 while x < 10\n");
        assert!(nodes.iter().all(|n| n.kind != NodeKind::While));
    }

    #[test]
    fn nested_if_inside_method() {
        let nodes = build("def foo\n  if x\n    y\n  end\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::If);
    }

    #[test]
    fn multiple_top_level_methods() {
        let nodes = build("def foo\nend\ndef bar\nend\n");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name.as_deref(), Some("foo"));
        assert_eq!(nodes[1].name.as_deref(), Some("bar"));
    }

    #[test]
    fn begin_block() {
        let nodes = build("begin\n  x\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Begin);
    }

    #[test]
    fn case_block() {
        let nodes = build("case x\nwhen 1\n  y\nend\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Case);
    }

    #[test]
    fn do_block_inside_method() {
        let src = "def foo\n  items.each do\n    x\n  end\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].name.as_deref(), Some("foo"));
        assert_eq!(nodes[0].end_line, 5);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::Block);
    }

    #[test]
    fn extra_end_at_root_recovers() {
        // An unexpected `end` at root level should be skipped, not stop parsing
        let src = "end\ndef foo\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].name.as_deref(), Some("foo"));
    }

    #[test]
    fn expression_if_inside_def_does_not_steal_end() {
        // `x = if cond ... end` is a block-form `if` in expression position.
        // Its `end` must not close the enclosing `def`.
        let src = "def foo\n  x = if cond\n    y\n  end\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1, "should produce exactly one top-level node");
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].end_line, 5, "def should close on line 5");
    }

    #[test]
    fn while_with_do_separator() {
        // `while cond do ... end` — the `do` is a separator, not a block opener.
        let src = "while cond do\n  x\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::While);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn for_with_do_separator() {
        // `for i in xs do ... end` — the `do` is a separator.
        let src = "for i in xs do\n  x\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::For);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn until_with_do_separator() {
        let src = "until done do\n  work\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Until);
        assert_eq!(nodes[0].start_line, 1);
        assert_eq!(nodes[0].end_line, 3);
    }

    #[test]
    fn while_do_inside_method() {
        // The `do` separator inside while must not consume the method's `end`.
        let src = "def foo\n  while cond do\n    x\n  end\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].end_line, 5);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::While);
    }

    #[test]
    fn for_do_inside_method() {
        let src = "def foo\n  for i in xs do\n    x\n  end\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Method);
        assert_eq!(nodes[0].end_line, 5);
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].kind, NodeKind::For);
    }

    #[test]
    fn semicolon_as_statement_boundary() {
        // `foo; if cond ... end` — the `if` after `;` is a statement start.
        let src = "foo; if cond\n  x\nend\n";
        let nodes = build(src);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::If);
    }
}
