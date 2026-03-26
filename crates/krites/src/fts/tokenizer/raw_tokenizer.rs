//! Tokenizer that emits the entire input as one token.
#![cfg_attr(
    test,
    expect(
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]
use super::{Token, TokenStream, Tokenizer};
use crate::fts::tokenizer::BoxTokenStream;

/// For each value of the field, emit a single unprocessed token.
#[derive(Clone)]
pub(crate) struct RawTokenizer;

pub(crate) struct RawTokenStream {
    token: Token,
    has_token: bool,
}

impl Tokenizer for RawTokenizer {
    fn token_stream<'a>(&self, text: &'a str) -> BoxTokenStream<'a> {
        let token = Token {
            offset_from: 0,
            offset_to: text.len(),
            position: 0,
            text: text.to_string(),
            position_length: 1,
        };
        RawTokenStream {
            token,
            has_token: true,
        }
        .into()
    }
}

impl TokenStream for RawTokenStream {
    fn advance(&mut self) -> bool {
        let result = self.has_token;
        self.has_token = false;
        result
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

#[cfg(test)]
mod tests {
    use crate::fts::tokenizer::tests::assert_token;
    use crate::fts::tokenizer::{RawTokenizer, TextAnalyzer, Token};

    #[test]
    fn raw_tokenizer_emits_entire_text_as_single_token() {
        let tokens = token_stream_helper("Hello, happy tax payer!");
        assert_eq!(tokens.len(), 1);
        assert_token(&tokens[0], 0, "Hello, happy tax payer!", 0, 23);
    }

    fn token_stream_helper(text: &str) -> Vec<Token> {
        use crate::fts::tokenizer::tests::collect_tokens;
        let a = TextAnalyzer::from(RawTokenizer);
        collect_tokens(a.token_stream(text))
    }
}
