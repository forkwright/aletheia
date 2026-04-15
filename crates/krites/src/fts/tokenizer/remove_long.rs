//! Filter that removes tokens exceeding a byte-length limit.
use super::{Token, TokenFilter, TokenStream};
use crate::fts::tokenizer::BoxTokenStream;

/// Drops tokens whose UTF-8 byte length meets or exceeds the configured limit.
///
/// Useful when indexing unconstrained content (e.g. base-64 blobs in mail).
#[derive(Debug, Clone)]
pub(crate) struct RemoveLongFilter {
    length_limit: usize,
}

impl RemoveLongFilter {
    /// Creates a `RemoveLongFilter` given a limit in bytes of the UTF-8 representation.
    pub(crate) fn limit(length_limit: usize) -> RemoveLongFilter {
        RemoveLongFilter { length_limit }
    }
}

impl RemoveLongFilterStream<'_> {
    fn predicate(&self, token: &Token) -> bool {
        token.text.len() < self.token_length_limit
    }
}

impl TokenFilter for RemoveLongFilter {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(RemoveLongFilterStream {
            token_length_limit: self.length_limit,
            tail: token_stream,
        })
    }
}

pub(crate) struct RemoveLongFilterStream<'a> {
    token_length_limit: usize,
    tail: BoxTokenStream<'a>,
}

impl TokenStream for RemoveLongFilterStream<'_> {
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
    use crate::fts::tokenizer::tests::assert_token;
    use crate::fts::tokenizer::{RemoveLongFilter, SimpleTokenizer, TextAnalyzer, Token};

    #[test]
    fn remove_long_filter_drops_tokens_exceeding_length_limit() {
        let tokens = token_stream_helper("hello tantivy, happy searching!");
        assert_eq!(tokens.len(), 2);
        assert_token(&tokens[0], 0, "hello", 0, 5);
        assert_token(&tokens[1], 2, "happy", 15, 20);
    }

    fn token_stream_helper(text: &str) -> Vec<Token> {
        use crate::fts::tokenizer::tests::collect_tokens;
        let a = TextAnalyzer::from(SimpleTokenizer).filter(RemoveLongFilter::limit(6));
        collect_tokens(a.token_stream(text))
    }
}
