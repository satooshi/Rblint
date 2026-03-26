/// Ruby lexer - tokenizes Ruby source code
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Integer,
    Float,
    StringLiteral,
    Symbol,
    Heredoc,

    // Identifiers
    Ident,
    Constant,    // starts with uppercase
    GlobalVar,   // $foo
    InstanceVar, // @foo
    ClassVar,    // @@foo

    // Keywords
    Def,
    End,
    Class,
    Module,
    Do,
    If,
    Unless,
    While,
    Until,
    For,
    In,
    Return,
    Yield,
    And,
    Or,
    Not,
    Nil,
    True,
    False,
    Self_,
    Super,
    Begin,
    Rescue,
    Ensure,
    Raise,
    Then,
    Else,
    Elsif,
    Case,
    When,
    Require,
    Attr,
    AttrReader,
    AttrWriter,
    AttrAccessor,
    Private,
    Protected,
    Public,
    Freeze,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    EqEqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Spaceship, // <=>
    And2,      // &&
    Or2,       // ||
    Bang,      // !
    Amp,       // &
    Pipe,      // |
    Caret,     // ^
    Tilde,     // ~
    LShift,    // <<
    RShift,    // >>
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AndEq,
    OrEq,
    Arrow,    // ->
    FatArrow, // =>
    Dot,
    Dot2,       // ..
    Dot3,       // ...
    ColonColon, // ::
    Colon,
    Semicolon,
    Comma,
    Question,
    At,

    // Brackets
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // Special
    Newline,
    Comment,
    Whitespace,
    Eof,
    Unknown(char),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    source: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    line: usize,
    col: usize,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Lexer {
            source,
            chars: source.char_indices().peekable(),
            line: 1,
            col: 1,
            pos: 0,
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, c)| *c)
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((i, c)) = self.chars.next() {
            self.pos = i + c.len_utf8();
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        let mut iter = self.source[self.pos..].chars();
        for _ in 0..n {
            iter.next();
        }
        iter.next()
    }

    fn lex_string(&mut self, quote: char) -> Token {
        let start_line = self.line;
        let start_col = self.col - 1;
        let mut s = String::from(quote);
        let mut escaped = false;
        loop {
            match self.peek() {
                None => break,
                Some(c) => {
                    self.advance();
                    s.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == quote {
                        break;
                    }
                }
            }
        }
        Token {
            kind: TokenKind::StringLiteral,
            text: s,
            line: start_line,
            col: start_col,
        }
    }

    fn lex_number(&mut self, first: char) -> Token {
        let start_line = self.line;
        let start_col = self.col - 1;
        let mut s = String::from(first);
        let mut is_float = false;

        // Handle 0x, 0b, 0o prefixes
        if first == '0' {
            if let Some('x' | 'X') = self.peek() {
                s.push(self.advance().unwrap());
                while matches!(self.peek(), Some('0'..='9' | 'a'..='f' | 'A'..='F' | '_')) {
                    s.push(self.advance().unwrap());
                }
                return Token {
                    kind: TokenKind::Integer,
                    text: s,
                    line: start_line,
                    col: start_col,
                };
            }
        }

        while matches!(self.peek(), Some('0'..='9' | '_')) {
            s.push(self.advance().unwrap());
        }
        if self.peek() == Some('.') && matches!(self.peek_nth(1), Some('0'..='9')) {
            is_float = true;
            s.push(self.advance().unwrap()); // '.'
            while matches!(self.peek(), Some('0'..='9' | '_')) {
                s.push(self.advance().unwrap());
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            s.push(self.advance().unwrap());
            if matches!(self.peek(), Some('+' | '-')) {
                s.push(self.advance().unwrap());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                s.push(self.advance().unwrap());
            }
        }
        let kind = if is_float {
            TokenKind::Float
        } else {
            TokenKind::Integer
        };
        Token {
            kind,
            text: s,
            line: start_line,
            col: start_col,
        }
    }

    fn lex_ident(&mut self, first: char) -> Token {
        let start_line = self.line;
        let start_col = self.col - 1;
        let mut s = String::from(first);
        while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
            s.push(self.advance().unwrap());
        }
        // Allow trailing ? or ! for method names
        if matches!(self.peek(), Some('?' | '!')) && self.peek_nth(1) != Some('=') {
            s.push(self.advance().unwrap());
        }

        let kind = match s.as_str() {
            "def" => TokenKind::Def,
            "end" => TokenKind::End,
            "class" => TokenKind::Class,
            "module" => TokenKind::Module,
            "do" => TokenKind::Do,
            "if" => TokenKind::If,
            "unless" => TokenKind::Unless,
            "while" => TokenKind::While,
            "until" => TokenKind::Until,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            "yield" => TokenKind::Yield,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "nil" => TokenKind::Nil,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "self" => TokenKind::Self_,
            "super" => TokenKind::Super,
            "begin" => TokenKind::Begin,
            "rescue" => TokenKind::Rescue,
            "ensure" => TokenKind::Ensure,
            "raise" => TokenKind::Raise,
            "then" => TokenKind::Then,
            "else" => TokenKind::Else,
            "elsif" => TokenKind::Elsif,
            "case" => TokenKind::Case,
            "when" => TokenKind::When,
            "require" => TokenKind::Require,
            "attr_reader" => TokenKind::AttrReader,
            "attr_writer" => TokenKind::AttrWriter,
            "attr_accessor" => TokenKind::AttrAccessor,
            "attr" => TokenKind::Attr,
            "private" => TokenKind::Private,
            "protected" => TokenKind::Protected,
            "public" => TokenKind::Public,
            "freeze" => TokenKind::Freeze,
            _ if first.is_uppercase() => TokenKind::Constant,
            _ => TokenKind::Ident,
        };
        Token {
            kind,
            text: s,
            line: start_line,
            col: start_col,
        }
    }

    fn lex_comment(&mut self) -> Token {
        let start_line = self.line;
        let start_col = self.col - 1;
        let mut s = String::from('#');
        while self.peek() != Some('\n') && self.peek().is_some() {
            s.push(self.advance().unwrap());
        }
        Token {
            kind: TokenKind::Comment,
            text: s,
            line: start_line,
            col: start_col,
        }
    }

    /// Consume a `%x(...)` / `%x{...}` / `%x[...]` / `%x<...>` shell literal as a
    /// single `StringLiteral` token.  `%` has already been consumed; this method
    /// consumes `x`, the opening delimiter, the body, and the matching closing
    /// delimiter.  Nested same-delimiter pairs (e.g. `%x[arr[0]]`) are handled by
    /// tracking depth.  `advance()` keeps `self.line` / `self.col` up to date.
    fn lex_percent_x_literal(&mut self, start_line: usize, start_col: usize) -> Token {
        self.advance(); // consume 'x'
        let open_char = self.advance().unwrap(); // '(' | '{' | '[' | '<'
        let close_char = match open_char {
            '(' => ')',
            '{' => '}',
            '[' => ']',
            '<' => '>',
            _ => unreachable!(),
        };
        let mut text = format!("%x{}", open_char);
        let mut depth = 1usize;
        while self.peek().is_some() {
            let ch = self.advance().unwrap(); // updates self.line / self.col
            text.push(ch);
            if ch == open_char {
                depth += 1;
            } else if ch == close_char {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
        Token {
            kind: TokenKind::StringLiteral,
            text,
            line: start_line,
            col: start_col,
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn next_token(&mut self) -> Token {
        let start_line = self.line;
        let start_col = self.col;

        let c = match self.advance() {
            None => {
                return Token {
                    kind: TokenKind::Eof,
                    text: String::new(),
                    line: start_line,
                    col: start_col,
                }
            }
            Some(c) => c,
        };

        match c {
            '\n' => Token {
                kind: TokenKind::Newline,
                text: "\n".into(),
                line: start_line,
                col: start_col - 1,
            },
            ' ' | '\t' | '\r' => {
                let mut s = String::from(c);
                while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
                    s.push(self.advance().unwrap());
                }
                Token {
                    kind: TokenKind::Whitespace,
                    text: s,
                    line: start_line,
                    col: start_col,
                }
            }
            '#' => self.lex_comment(),
            '"' | '\'' => self.lex_string(c),
            '0'..='9' => self.lex_number(c),
            'a'..='z' | 'A'..='Z' | '_' => self.lex_ident(c),
            '$' => {
                let mut s = String::from(c);
                while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
                    s.push(self.advance().unwrap());
                }
                Token {
                    kind: TokenKind::GlobalVar,
                    text: s,
                    line: start_line,
                    col: start_col,
                }
            }
            '@' => {
                if self.peek() == Some('@') {
                    self.advance();
                    let mut s = String::from("@@");
                    while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
                        s.push(self.advance().unwrap());
                    }
                    Token {
                        kind: TokenKind::ClassVar,
                        text: s,
                        line: start_line,
                        col: start_col,
                    }
                } else {
                    let mut s = String::from('@');
                    while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
                        s.push(self.advance().unwrap());
                    }
                    Token {
                        kind: TokenKind::InstanceVar,
                        text: s,
                        line: start_line,
                        col: start_col,
                    }
                }
            }
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    Token {
                        kind: TokenKind::ColonColon,
                        text: "::".into(),
                        line: start_line,
                        col: start_col,
                    }
                } else if matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '_')) {
                    let mut s = String::from(':');
                    while matches!(
                        self.peek(),
                        Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '?' | '!')
                    ) {
                        s.push(self.advance().unwrap());
                    }
                    Token {
                        kind: TokenKind::Symbol,
                        text: s,
                        line: start_line,
                        col: start_col,
                    }
                } else {
                    Token {
                        kind: TokenKind::Colon,
                        text: ":".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
            }
            '=' => match self.peek() {
                Some('=') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token {
                            kind: TokenKind::EqEqEq,
                            text: "===".into(),
                            line: start_line,
                            col: start_col,
                        }
                    } else {
                        Token {
                            kind: TokenKind::EqEq,
                            text: "==".into(),
                            line: start_line,
                            col: start_col,
                        }
                    }
                }
                Some('>') => {
                    self.advance();
                    Token {
                        kind: TokenKind::FatArrow,
                        text: "=>".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Eq,
                    text: "=".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '!' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::NotEq,
                        text: "!=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Bang,
                    text: "!".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '<' => match self.peek() {
                Some('<') => {
                    self.advance();
                    Token {
                        kind: TokenKind::LShift,
                        text: "<<".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('=') => {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        Token {
                            kind: TokenKind::Spaceship,
                            text: "<=>".into(),
                            line: start_line,
                            col: start_col,
                        }
                    } else {
                        Token {
                            kind: TokenKind::LtEq,
                            text: "<=".into(),
                            line: start_line,
                            col: start_col,
                        }
                    }
                }
                _ => Token {
                    kind: TokenKind::Lt,
                    text: "<".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '>' => match self.peek() {
                Some('>') => {
                    self.advance();
                    Token {
                        kind: TokenKind::RShift,
                        text: ">>".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::GtEq,
                        text: ">=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Gt,
                    text: ">".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '+' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::PlusEq,
                        text: "+=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Plus,
                    text: "+".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '-' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::MinusEq,
                        text: "-=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('>') => {
                    self.advance();
                    Token {
                        kind: TokenKind::Arrow,
                        text: "->".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Minus,
                    text: "-".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '*' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::StarEq,
                        text: "*=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Star,
                    text: "*".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '/' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::SlashEq,
                        text: "/=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Slash,
                    text: "/".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '%' => match self.peek() {
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::PercentEq,
                        text: "%=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('x') if matches!(self.peek_nth(1), Some('(' | '{' | '[' | '<')) => {
                    self.lex_percent_x_literal(start_line, start_col)
                }
                _ => Token {
                    kind: TokenKind::Percent,
                    text: "%".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '&' => match self.peek() {
                Some('&') => {
                    self.advance();
                    Token {
                        kind: TokenKind::And2,
                        text: "&&".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::AndEq,
                        text: "&=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Amp,
                    text: "&".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '|' => match self.peek() {
                Some('|') => {
                    self.advance();
                    Token {
                        kind: TokenKind::Or2,
                        text: "||".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                Some('=') => {
                    self.advance();
                    Token {
                        kind: TokenKind::OrEq,
                        text: "|=".into(),
                        line: start_line,
                        col: start_col,
                    }
                }
                _ => Token {
                    kind: TokenKind::Pipe,
                    text: "|".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '.' => match self.peek() {
                Some('.') => {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        Token {
                            kind: TokenKind::Dot3,
                            text: "...".into(),
                            line: start_line,
                            col: start_col,
                        }
                    } else {
                        Token {
                            kind: TokenKind::Dot2,
                            text: "..".into(),
                            line: start_line,
                            col: start_col,
                        }
                    }
                }
                _ => Token {
                    kind: TokenKind::Dot,
                    text: ".".into(),
                    line: start_line,
                    col: start_col,
                },
            },
            '^' => Token {
                kind: TokenKind::Caret,
                text: "^".into(),
                line: start_line,
                col: start_col,
            },
            '~' => Token {
                kind: TokenKind::Tilde,
                text: "~".into(),
                line: start_line,
                col: start_col,
            },
            '?' => Token {
                kind: TokenKind::Question,
                text: "?".into(),
                line: start_line,
                col: start_col,
            },
            ';' => Token {
                kind: TokenKind::Semicolon,
                text: ";".into(),
                line: start_line,
                col: start_col,
            },
            ',' => Token {
                kind: TokenKind::Comma,
                text: ",".into(),
                line: start_line,
                col: start_col,
            },
            '(' => Token {
                kind: TokenKind::LParen,
                text: "(".into(),
                line: start_line,
                col: start_col,
            },
            ')' => Token {
                kind: TokenKind::RParen,
                text: ")".into(),
                line: start_line,
                col: start_col,
            },
            '[' => Token {
                kind: TokenKind::LBracket,
                text: "[".into(),
                line: start_line,
                col: start_col,
            },
            ']' => Token {
                kind: TokenKind::RBracket,
                text: "]".into(),
                line: start_line,
                col: start_col,
            },
            '{' => Token {
                kind: TokenKind::LBrace,
                text: "{".into(),
                line: start_line,
                col: start_col,
            },
            '}' => Token {
                kind: TokenKind::RBrace,
                text: "}".into(),
                line: start_line,
                col: start_col,
            },
            other => Token {
                kind: TokenKind::Unknown(other),
                text: other.to_string(),
                line: start_line,
                col: start_col,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(src: &str) -> Vec<Token> {
        Lexer::new(src).tokenize()
    }

    fn non_ws_kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src)
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.kind,
                    TokenKind::Whitespace | TokenKind::Newline | TokenKind::Eof
                )
            })
            .map(|t| t.kind)
            .collect()
    }

    // --- Keywords ---

    #[test]
    fn keywords_are_recognized() {
        assert_eq!(non_ws_kinds("def"), vec![TokenKind::Def]);
        assert_eq!(non_ws_kinds("end"), vec![TokenKind::End]);
        assert_eq!(non_ws_kinds("class"), vec![TokenKind::Class]);
        assert_eq!(non_ws_kinds("module"), vec![TokenKind::Module]);
        assert_eq!(non_ws_kinds("if"), vec![TokenKind::If]);
        assert_eq!(non_ws_kinds("unless"), vec![TokenKind::Unless]);
        assert_eq!(non_ws_kinds("while"), vec![TokenKind::While]);
        assert_eq!(non_ws_kinds("return"), vec![TokenKind::Return]);
        assert_eq!(non_ws_kinds("nil"), vec![TokenKind::Nil]);
        assert_eq!(non_ws_kinds("true"), vec![TokenKind::True]);
        assert_eq!(non_ws_kinds("false"), vec![TokenKind::False]);
    }

    #[test]
    fn ident_vs_constant() {
        assert_eq!(non_ws_kinds("foo"), vec![TokenKind::Ident]);
        assert_eq!(non_ws_kinds("Foo"), vec![TokenKind::Constant]);
        assert_eq!(non_ws_kinds("FOO"), vec![TokenKind::Constant]);
    }

    // --- Variables ---

    #[test]
    fn instance_variable() {
        let toks = tokenize("@foo");
        assert_eq!(toks[0].kind, TokenKind::InstanceVar);
        assert_eq!(toks[0].text, "@foo");
    }

    #[test]
    fn class_variable() {
        let toks = tokenize("@@bar");
        assert_eq!(toks[0].kind, TokenKind::ClassVar);
        assert_eq!(toks[0].text, "@@bar");
    }

    #[test]
    fn global_variable() {
        let toks = tokenize("$baz");
        assert_eq!(toks[0].kind, TokenKind::GlobalVar);
        assert_eq!(toks[0].text, "$baz");
    }

    // --- Literals ---

    #[test]
    fn integer_literal() {
        assert_eq!(non_ws_kinds("42"), vec![TokenKind::Integer]);
        assert_eq!(non_ws_kinds("0xFF"), vec![TokenKind::Integer]);
    }

    #[test]
    fn float_literal() {
        assert_eq!(non_ws_kinds("3.14"), vec![TokenKind::Float]);
    }

    #[test]
    fn string_double_quote() {
        let toks = tokenize("\"hello\"");
        assert_eq!(toks[0].kind, TokenKind::StringLiteral);
        assert_eq!(toks[0].text, "\"hello\"");
    }

    #[test]
    fn string_single_quote() {
        let toks = tokenize("'world'");
        assert_eq!(toks[0].kind, TokenKind::StringLiteral);
    }

    #[test]
    fn symbol_literal() {
        let toks = tokenize(":my_sym");
        assert_eq!(toks[0].kind, TokenKind::Symbol);
        assert_eq!(toks[0].text, ":my_sym");
    }

    // --- Compound operators ---

    #[test]
    fn compound_operators() {
        assert_eq!(non_ws_kinds("=="), vec![TokenKind::EqEq]);
        assert_eq!(non_ws_kinds("!="), vec![TokenKind::NotEq]);
        assert_eq!(non_ws_kinds("<="), vec![TokenKind::LtEq]);
        assert_eq!(non_ws_kinds(">="), vec![TokenKind::GtEq]);
        assert_eq!(non_ws_kinds("<=>"), vec![TokenKind::Spaceship]);
        assert_eq!(non_ws_kinds("&&"), vec![TokenKind::And2]);
        assert_eq!(non_ws_kinds("||"), vec![TokenKind::Or2]);
        assert_eq!(non_ws_kinds("->"), vec![TokenKind::Arrow]);
        assert_eq!(non_ws_kinds("=>"), vec![TokenKind::FatArrow]);
        assert_eq!(non_ws_kinds("::"), vec![TokenKind::ColonColon]);
    }

    // --- Line & column tracking ---

    #[test]
    fn line_numbers_are_tracked() {
        let toks = tokenize("a\nb\nc");
        let idents: Vec<_> = toks.iter().filter(|t| t.kind == TokenKind::Ident).collect();
        assert_eq!(idents[0].line, 1);
        assert_eq!(idents[1].line, 2);
        assert_eq!(idents[2].line, 3);
    }

    #[test]
    fn last_token_is_eof() {
        let toks = tokenize("x = 1");
        assert_eq!(toks.last().unwrap().kind, TokenKind::Eof);
    }

    #[test]
    fn empty_source_has_only_eof() {
        let toks = tokenize("");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Eof);
    }

    #[test]
    fn comment_is_tokenized() {
        let toks = tokenize("# hello world");
        assert_eq!(toks[0].kind, TokenKind::Comment);
        assert_eq!(toks[0].text, "# hello world");
    }

    #[test]
    fn method_with_bang_or_question() {
        assert_eq!(non_ws_kinds("valid?"), vec![TokenKind::Ident]);
        assert_eq!(non_ws_kinds("save!"), vec![TokenKind::Ident]);
    }
}
