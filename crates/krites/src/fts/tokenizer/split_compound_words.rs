//! Compound word splitting filter.
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

use super::{BoxTokenStream, Token, TokenFilter, TokenStream};
use crate::error::InternalResult as Result;
#[derive(Clone)]
pub(crate) struct SplitCompoundWords {
    dict: AhoCorasick,
}

impl SplitCompoundWords {
    /// Create a filter from a given dictionary.
    ///
    /// The dictionary will be used to construct an [`AhoCorasick`] automaton
    /// with reasonable defaults. See [`from_automaton`][Self::from_automaton] if
    /// more control over its construction is required.
    pub(crate) fn from_dictionary<I, P>(dict: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<[u8]>,
    {
        let dict = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(dict)
            .map_err(|e| {
                crate::error::InternalError::from(
                    crate::fts::error::TokenizationFailedSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

        Ok(Self::from_automaton(dict))
    }
}

impl SplitCompoundWords {
    /// Create a filter from a given automaton.
    ///
    /// The automaton should use one of the leftmost-first match kinds
    /// and it should not be anchored.
    pub(crate) fn from_automaton(dict: AhoCorasick) -> Self {
        Self { dict }
    }
}

impl TokenFilter for SplitCompoundWords {
    fn transform<'a>(&self, stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(SplitCompoundWordsTokenStream {
            dict: self.dict.clone(),
            tail: stream,
            cuts: Vec::new(),
            parts: Vec::new(),
        })
    }
}

struct SplitCompoundWordsTokenStream<'a> {
    dict: AhoCorasick,
    tail: BoxTokenStream<'a>,
    cuts: Vec<usize>,
    parts: Vec<Token>,
}

impl<'a> SplitCompoundWordsTokenStream<'a> {
    fn split(&mut self) {
        let token = self.tail.token();
        let mut text = token.text.as_str();

        self.cuts.clear();
        let mut pos = 0;

        for match_ in self.dict.find_iter(text) {
            if pos != match_.start() {
                break;
            }

            self.cuts.push(pos);
            pos = match_.end();
        }

        if pos == token.text.len() {
            for pos in self.cuts.iter().rev() {
                let (head, tail) = text.split_at(*pos);

                text = head;
                self.parts.push(Token {
                    text: tail.to_owned(),
                    ..*token
                });
            }
        }
    }
}

impl<'a> TokenStream for SplitCompoundWordsTokenStream<'a> {
    fn advance(&mut self) -> bool {
        self.parts.pop();

        if !self.parts.is_empty() {
            return true;
        }

        if !self.tail.advance() {
            return false;
        }

        self.split();
        true
    }

    fn token(&self) -> &Token {
        self.parts.last().unwrap_or_else(|| self.tail.token())
    }

    fn token_mut(&mut self) -> &mut Token {
        self.parts
            .last_mut()
            .unwrap_or_else(|| self.tail.token_mut())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::fts::tokenizer::{SimpleTokenizer, TextAnalyzer};

    #[test]
    fn splitting_compound_words_works() {
        let tokenizer = TextAnalyzer::from(SimpleTokenizer)
            .filter(SplitCompoundWords::from_dictionary(["foo", "bar"]).unwrap());

        {
            let mut stream = tokenizer.token_stream("");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foo bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobarbaz");
            assert_eq!(stream.next().unwrap().text, "foobarbaz");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("baz foobar qux");
            assert_eq!(stream.next().unwrap().text, "baz");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "qux");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobar foobar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobar foo bar foobar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobazbar foo bar foobar");
            assert_eq!(stream.next().unwrap().text, "foobazbar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("foobar qux foobar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "qux");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next(), None);
        }

        {
            let mut stream = tokenizer.token_stream("barfoo");
            assert_eq!(stream.next().unwrap().text, "bar");
            assert_eq!(stream.next().unwrap().text, "foo");
            assert_eq!(stream.next(), None);
        }
    }
}
