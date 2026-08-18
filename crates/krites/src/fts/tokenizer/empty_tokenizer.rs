// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! No-op tokenizer producing zero tokens.
use crate::fts::tokenizer::{BoxTokenStream, Token, TokenStream, Tokenizer};

/// Produces zero tokens for any input. Used as the default `TextAnalyzer` tokenizer.
#[derive(Debug, Clone)]
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
    use crate::fts::tokenizer::Tokenizer;

    #[test]
    fn empty_tokenizer_produces_no_tokens() {
        let tokenizer = super::EmptyTokenizer;
        let mut empty = tokenizer.token_stream("whatever string");
        assert!(!empty.advance());
    }
}
