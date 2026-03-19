//! Lowercasing token filter.
#![cfg_attr(
    test,
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]
use std::mem;

use super::{Token, TokenFilter, TokenStream};
use crate::engine::fts::tokenizer::BoxTokenStream;

impl TokenFilter for LowerCaser {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(LowerCaserTokenStream {
            tail: token_stream,
            buffer: String::with_capacity(100),
        })
    }
}

/// Token filter that lowercase terms.
#[derive(Clone)]
pub(crate) struct LowerCaser;

pub(crate) struct LowerCaserTokenStream<'a> {
    buffer: String,
    tail: BoxTokenStream<'a>,
}

fn to_lowercase_unicode(text: &str, output: &mut String) {
    output.clear();
    for c in text.chars() {
        // NOTE: Contrary to the std, we do not take care of sigma special case.
        // This will have an normalizationo effect, which is ok for search.
        output.extend(c.to_lowercase());
    }
}

impl<'a> TokenStream for LowerCaserTokenStream<'a> {
    fn advance(&mut self) -> bool {
        if !self.tail.advance() {
            return false;
        }
        if self.token_mut().text.is_ascii() {
            self.token_mut().text.make_ascii_lowercase();
        } else {
            to_lowercase_unicode(&self.tail.token().text, &mut self.buffer);
            mem::swap(&mut self.tail.token_mut().text, &mut self.buffer);
        }
        true
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
    use crate::engine::fts::tokenizer::{LowerCaser, SimpleTokenizer, TextAnalyzer, Token};

    #[test]
    fn lower_caser_lowercases_ascii_and_unicode_tokens() {
        let tokens = token_stream_helper("Tree");
        assert_eq!(tokens.len(), 1);
        assert_token(&tokens[0], 0, "tree", 0, 4);

        let tokens = token_stream_helper("Русский текст");
        assert_eq!(tokens.len(), 2);
        assert_token(&tokens[0], 0, "русский", 0, 14);
        assert_token(&tokens[1], 1, "текст", 15, 25);
    }

    fn token_stream_helper(text: &str) -> Vec<Token> {
        use crate::engine::fts::tokenizer::tests::collect_tokens;
        let a = TextAnalyzer::from(SimpleTokenizer).filter(LowerCaser);
        collect_tokens(a.token_stream(text))
    }
}
