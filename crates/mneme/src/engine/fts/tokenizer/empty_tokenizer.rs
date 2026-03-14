//! No-op tokenizer producing zero tokens.
use crate::engine::fts::tokenizer::{BoxTokenStream, Token, TokenStream, Tokenizer};

#[derive(Clone)]
pub(crate) struct EmptyTokenizer;

impl Tokenizer for EmptyTokenizer {
    fn token_stream<'a>(&self, _text: &'a str) -> BoxTokenStream<'a> {
        EmptyTokenStream::default().into()
    }
}

#[derive(Default)]
struct EmptyTokenStream {
    token: Token,
}

impl TokenStream for EmptyTokenStream {
    fn advance(&mut self) -> bool {
        false
    }

    fn token(&self) -> &super::Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut super::Token {
        &mut self.token
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::fts::tokenizer::Tokenizer;

    #[test]
    fn empty_tokenizer_produces_no_tokens() {
        let tokenizer = super::EmptyTokenizer;
        let mut empty = tokenizer.token_stream("whatever string");
        assert!(!empty.advance());
    }
}
