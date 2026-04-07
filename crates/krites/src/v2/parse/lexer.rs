//! Lexer for the krites v2 Datalog dialect.
//!
//! Converts source text into a flat token stream.  Hand-written for clarity
//! and precise error messages.

use crate::v2::error::{self};
use crate::v2::parse::ParseResult;

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A single lexical token.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Token {
    // Literals
    /// Identifier or keyword.
    Ident(String),
    /// Integer literal.
    Int(i64),
    /// Floating-point literal.
    Float(f64),
    /// String literal (content without quotes, escapes processed).
    String(String),

    // Commands / modifiers (lexed as single tokens for cleaner parsing)
    /// `?` — starts a query output clause.
    Question,
    /// `:=` — rule assignment.
    Arrow,
    /// `=>` — key/value separator in `:create`.
    FatArrow,
    /// `:put`
    ColonPut,
    /// `:create`
    ColonCreate,
    /// `:replace`
    ColonReplace,
    /// `:remove`
    ColonRemove,
    /// `:order`
    ColonOrder,
    /// `:limit`
    ColonLimit,
    /// `::fts`
    DoubleColonFts,
    /// `::hnsw`
    DoubleColonHnsw,

    // Operators
    /// `=`
    Eq,
    /// `!=`
    Neq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Lte,
    /// `>=`
    Gte,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `~`
    Tilde,
    /// `<~`
    LtTilde,
    /// `$`
    Dollar,
    /// `:`
    Colon,
    /// `::`
    DoubleColon,
    /// `.`
    Dot,

    // Delimiters
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `|`
    Pipe,

    /// End of input.
    Eof,
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Tokenize a Datalog source string.
pub fn tokenize(source: &str) -> ParseResult<Vec<Token>> {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn run(mut self) -> ParseResult<Vec<Token>> {
        while !self.at_end() {
            self.next_token()?;
        }
        self.tokens.push(Token::Eof);
        Ok(self.tokens)
    }

    // -----------------------------------------------------------------------
    // Position helpers
    // -----------------------------------------------------------------------

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> u8 {
        self.bytes.get(self.pos).copied().unwrap_or(b'\0')
    }

    fn peek_ahead(&self, n: usize) -> u8 {
        self.bytes.get(self.pos + n).copied().unwrap_or(b'\0')
    }

    fn advance(&mut self) -> u8 {
        let ch = self.peek();
        if !self.at_end() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while !self.at_end() {
            match self.peek() {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.advance();
                }
                b'/' if self.peek_ahead(1) == b'/' => {
                    // Line comment: skip to end of line.
                    while !self.at_end() && self.peek() != b'\n' {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn span(&self, start: usize) -> String {
        format!("{}..{}", start, self.pos)
    }

    // -----------------------------------------------------------------------
    // Token dispatch
    // -----------------------------------------------------------------------

    fn next_token(&mut self) -> ParseResult<()> {
        self.skip_whitespace();
        if self.at_end() {
            return Ok(());
        }

        let start = self.pos;
        let ch = self.peek();

        match ch {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                self.ident();
                return Ok(());
            }
            b'0'..=b'9' => {
                self.number()?;
                return Ok(());
            }
            b'\'' | b'"' => {
                self.string()?;
                return Ok(());
            }
            _ => {}
        }

        // Multi-character operators and commands.
        match (ch, self.peek_ahead(1)) {
            (b':', b':') => match self.peek_ahead(2) {
                b'f' if self.peek_ahead(3) == b't' && self.peek_ahead(4) == b's' => {
                    self.pos += 5;
                    self.tokens.push(Token::DoubleColonFts);
                }
                b'h'
                    if self.peek_ahead(3) == b'n'
                        && self.peek_ahead(4) == b's'
                        && self.peek_ahead(5) == b'w' =>
                {
                    self.pos += 6;
                    self.tokens.push(Token::DoubleColonHnsw);
                }
                _ => {
                    self.pos += 2;
                    self.tokens.push(Token::DoubleColon);
                }
            },
            (b':', b'p')
                if self.peek_ahead(2) == b'u' && self.peek_ahead(3) == b't' =>
            {
                self.pos += 4;
                self.tokens.push(Token::ColonPut);
            }
            (b':', b'c')
                if self.peek_ahead(2) == b'r'
                    && self.peek_ahead(3) == b'e'
                    && self.peek_ahead(4) == b'a'
                    && self.peek_ahead(5) == b't'
                    && self.peek_ahead(6) == b'e' =>
            {
                self.pos += 7;
                self.tokens.push(Token::ColonCreate);
            }
            (b':', b'r')
                if self.peek_ahead(2) == b'e'
                    && self.peek_ahead(3) == b'p'
                    && self.peek_ahead(4) == b'l'
                    && self.peek_ahead(5) == b'a'
                    && self.peek_ahead(6) == b'c'
                    && self.peek_ahead(7) == b'e' =>
            {
                self.pos += 8;
                self.tokens.push(Token::ColonReplace);
            }
            (b':', b'r')
                if self.peek_ahead(2) == b'e'
                    && self.peek_ahead(3) == b'm'
                    && self.peek_ahead(4) == b'o'
                    && self.peek_ahead(5) == b'v'
                    && self.peek_ahead(6) == b'e' =>
            {
                self.pos += 7;
                self.tokens.push(Token::ColonRemove);
            }
            (b':', b'o')
                if self.peek_ahead(2) == b'r'
                    && self.peek_ahead(3) == b'd'
                    && self.peek_ahead(4) == b'e'
                    && self.peek_ahead(5) == b'r' =>
            {
                self.pos += 6;
                self.tokens.push(Token::ColonOrder);
            }
            (b':', b'l')
                if self.peek_ahead(2) == b'i'
                    && self.peek_ahead(3) == b'm'
                    && self.peek_ahead(4) == b'i'
                    && self.peek_ahead(5) == b't' =>
            {
                self.pos += 6;
                self.tokens.push(Token::ColonLimit);
            }
            (b'=', b'>') => {
                self.pos += 2;
                self.tokens.push(Token::FatArrow);
            }
            (b':', b'=') => {
                self.pos += 2;
                self.tokens.push(Token::Arrow);
            }
            (b'=', b'=') => {
                self.pos += 2;
                self.tokens.push(Token::Eq);
            }
            (b'=', _) => {
                self.pos += 1;
                self.tokens.push(Token::Eq);
            }
            (b'!', b'=') => {
                self.pos += 2;
                self.tokens.push(Token::Neq);
            }
            (b'<', b'=') => {
                self.pos += 2;
                self.tokens.push(Token::Lte);
            }
            (b'<', b'~') => {
                self.pos += 2;
                self.tokens.push(Token::LtTilde);
            }
            (b'<', _) => {
                self.pos += 1;
                self.tokens.push(Token::Lt);
            }
            (b'>', b'=') => {
                self.pos += 2;
                self.tokens.push(Token::Gte);
            }
            (b'>', _) => {
                self.pos += 1;
                self.tokens.push(Token::Gt);
            }
            (b':', _) => {
                self.pos += 1;
                self.tokens.push(Token::Colon);
            }
            _ => {
                self.advance();
                let tok = match ch {
                    b'+' => Token::Plus,
                    b'-' => Token::Minus,
                    b'*' => Token::Star,
                    b'/' => Token::Slash,
                    b'~' => Token::Tilde,
                    b'$' => Token::Dollar,
                    b'(' => Token::LParen,
                    b')' => Token::RParen,
                    b'{' => Token::LBrace,
                    b'}' => Token::RBrace,
                    b'[' => Token::LBracket,
                    b']' => Token::RBracket,
                    b',' => Token::Comma,
                    b'|' => Token::Pipe,
                    b'?' => Token::Question,
                    b'.' => Token::Dot,
                    _ => {
                        return Err(error::ParseSnafu {
                            message: format!("unexpected character: '{}' (0x{:02x})", ch as char, ch),
                            span: self.span(start),
                        }
                        .build());
                    }
                };
                self.tokens.push(tok);
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Identifier
    // -----------------------------------------------------------------------

    fn ident(&mut self) {
        let start = self.pos;
        while !self.at_end() {
            match self.peek() {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' => {
                    self.advance();
                }
                _ => break,
            }
        }
        let text = &self.source[start..self.pos];
        self.tokens.push(Token::Ident(text.to_owned()));
    }

    // -----------------------------------------------------------------------
    // Number
    // -----------------------------------------------------------------------

    fn number(&mut self) -> ParseResult<()> {
        let start = self.pos;
        let mut is_float = false;

        while !self.at_end() && self.peek().is_ascii_digit() {
            self.advance();
        }

        if !self.at_end() && self.peek() == b'.' {
            is_float = true;
            self.advance();
            while !self.at_end() && self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        if !self.at_end() {
            let ch = self.peek();
            if ch == b'e' || ch == b'E' {
                is_float = true;
                self.advance();
                if !self.at_end() && (self.peek() == b'+' || self.peek() == b'-') {
                    self.advance();
                }
                if !self.at_end() && !self.peek().is_ascii_digit() {
                    return Err(error::ParseSnafu {
                        message: "expected digits after exponent in number".to_owned(),
                        span: self.span(start),
                    }
                    .build());
                }
                while !self.at_end() && self.peek().is_ascii_digit() {
                    self.advance();
                }
            }
        }

        let text = &self.source[start..self.pos];
        if is_float {
            let value: f64 = text.parse().map_err(|_| {
                error::ParseSnafu {
                    message: format!("invalid float literal: {text}"),
                    span: self.span(start),
                }
                .build()
            })?;
            self.tokens.push(Token::Float(value));
        } else {
            let value: i64 = text.parse().map_err(|_| {
                error::ParseSnafu {
                    message: format!("invalid integer literal: {text}"),
                    span: self.span(start),
                }
                .build()
            })?;
            self.tokens.push(Token::Int(value));
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // String
    // -----------------------------------------------------------------------

    fn string(&mut self) -> ParseResult<()> {
        let start = self.pos;
        let quote = self.advance(); // ' or "
        let mut content = String::new();

        while !self.at_end() {
            let ch = self.advance();
            if ch == quote {
                self.tokens.push(Token::String(content));
                return Ok(());
            }
            if ch == b'\\' {
                if self.at_end() {
                    break;
                }
                let esc = self.advance();
                match esc {
                    b'n' => content.push('\n'),
                    b't' => content.push('\t'),
                    b'r' => content.push('\r'),
                    b'\\' => content.push('\\'),
                    b'\'' => content.push('\''),
                    b'"' => content.push('"'),
                    b'0' => content.push('\0'),
                    _ => {
                        return Err(error::ParseSnafu {
                            message: format!("invalid escape sequence: \\{}", esc as char),
                            span: self.span(start),
                        }
                        .build());
                    }
                }
            } else {
                content.push(ch as char);
            }
        }

        Err(error::ParseSnafu {
            message: "unterminated string literal".to_owned(),
            span: self.span(start),
        }
        .build())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn tokenize_simple_query() {
        let tokens = tokenize("?[x] := *facts{id: x}").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Question,
                Token::LBracket,
                Token::Ident("x".to_owned()),
                Token::RBracket,
                Token::Arrow,
                Token::Star,
                Token::Ident("facts".to_owned()),
                Token::LBrace,
                Token::Ident("id".to_owned()),
                Token::Colon,
                Token::Ident("x".to_owned()),
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_commands() {
        let tokens = tokenize(":put :create :replace :remove ::fts ::hnsw").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ColonPut,
                Token::ColonCreate,
                Token::ColonReplace,
                Token::ColonRemove,
                Token::DoubleColonFts,
                Token::DoubleColonHnsw,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_operators() {
        let tokens = tokenize("= != < > <= >= + - * / := => ~ <~ $").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Eq,
                Token::Neq,
                Token::Lt,
                Token::Gt,
                Token::Lte,
                Token::Gte,
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Arrow,
                Token::FatArrow,
                Token::Tilde,
                Token::LtTilde,
                Token::Dollar,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_numbers() {
        let tokens = tokenize("42 3.14 1e10 2.5e-3").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Int(42),
                Token::Float(3.14),
                Token::Float(1e10),
                Token::Float(2.5e-3),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_strings() {
        let tokens = tokenize("'hello' \"world\" 'it\\'s'").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::String("hello".to_owned()),
                Token::String("world".to_owned()),
                Token::String("it's".to_owned()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_comments() {
        let tokens = tokenize("?[x] // this is a comment\n:= *facts{x}").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Question,
                Token::LBracket,
                Token::Ident("x".to_owned()),
                Token::RBracket,
                Token::Arrow,
                Token::Star,
                Token::Ident("facts".to_owned()),
                Token::LBrace,
                Token::Ident("x".to_owned()),
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_modifiers() {
        let tokens = tokenize(":order :limit :order -x :limit 10").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::ColonOrder,
                Token::ColonLimit,
                Token::ColonOrder,
                Token::Minus,
                Token::Ident("x".to_owned()),
                Token::ColonLimit,
                Token::Int(10),
                Token::Eof,
            ]
        );
    }
}
