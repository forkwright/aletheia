//! Filter that removes non-alphanumeric tokens.
#![cfg_attr(
    test,
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]
use super::{BoxTokenStream, Token, TokenFilter, TokenStream};

/// `TokenFilter` that removes all tokens that contain non
/// ascii alphanumeric characters.
#[derive(Clone)]
pub(crate) struct AlphaNumOnlyFilter;

pub(crate) struct AlphaNumOnlyFilterStream<'a> {
    tail: BoxTokenStream<'a>,
}

impl<'a> AlphaNumOnlyFilterStream<'a> {
    fn predicate(&self, token: &Token) -> bool {
        token.text.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

impl TokenFilter for AlphaNumOnlyFilter {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(AlphaNumOnlyFilterStream { tail: token_stream })
    }
}

impl<'a> TokenStream for AlphaNumOnlyFilterStream<'a> {
    fn advance(&mut self) -> bool {
        while self.tail.advance() {
            if self.predicate(self.tail.token()) {
                return true;
            }
        }

        false
    }

    fn token(&self) -> &Token {
        self.tail.token()
    }

    fn token_mut(&mut self) -> &mut Token {
        self.tail.token_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::fts::tokenizer::tests::assert_token;
    use crate::engine::fts::tokenizer::{AlphaNumOnlyFilter, SimpleTokenizer, TextAnalyzer, Token};

    #[test]
    fn test_alphanum_only() {
        let tokens = token_stream_helper("I am a cat. 我輩は猫である。(1906)");
        assert_eq!(tokens.len(), 5);
        assert_token(&tokens[0], 0, "I", 0, 1);
        assert_token(&tokens[1], 1, "am", 2, 4);
        assert_token(&tokens[2], 2, "a", 5, 6);
        assert_token(&tokens[3], 3, "cat", 7, 10);
        assert_token(&tokens[4], 5, "1906", 37, 41);
    }

    fn token_stream_helper(text: &str) -> Vec<Token> {
        use crate::engine::fts::tokenizer::tests::collect_tokens;
        let a = TextAnalyzer::from(SimpleTokenizer).filter(AlphaNumOnlyFilter);
        collect_tokens(a.token_stream(text))
    }
}
