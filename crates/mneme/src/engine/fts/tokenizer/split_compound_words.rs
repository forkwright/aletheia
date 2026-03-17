//! Compound word splitting filter.
use super::{BoxTokenStream, Token, TokenFilter, TokenStream};
use crate::engine::error::InternalResult as Result;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

/// A [`TokenFilter`] which splits compound words into their parts
/// based on a given dictionary.
///
/// Words only will be split if they can be fully decomposed into
/// consecutive matches into the given dictionary.
///
/// This is mostly useful to split [compound nouns][compound] common to many
/// Germanic languages into their constituents.
///
/// # Example
///
/// The quality of the dictionary determines the quality of the splits,
/// e.g. the missing stem "back" of "backen" implies that "brotbackautomat"
/// is not split in the following example.
///
/// ```text
/// use tantivy::tokenizer::{SimpleTokenizer, SplitCompoundWords, TextAnalyzer};
///
/// let tokenizer =
///        TextAnalyzer::from(SimpleTokenizer).filter(SplitCompoundWords::from_dictionary([
///            "dampf", "schiff", "fahrt", "brot", "backen", "automat",
///        ]));
///
/// let mut stream = tokenizer.token_stream("dampfschifffahrt");
/// assert_eq!(stream.next()?.text, "dampf");
/// assert_eq!(stream.next()?.text, "schiff");
/// assert_eq!(stream.next()?.text, "fahrt");
/// assert_eq!(stream.next(), None);
///
/// let mut stream = tokenizer.token_stream("brotbackautomat");
/// assert_eq!(stream.next()?.text, "brotbackautomat");
/// assert_eq!(stream.next(), None);
/// ```
///
/// [compound]: https://en.wikipedia.org/wiki/Compound_(linguistics)
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
                crate::engine::error::InternalError::from(
                    crate::engine::fts::error::TokenizationFailedSnafu {
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
    // Will use `self.cuts` to fill `self.parts` if `self.tail.token()`
    // can fully be split into consecutive matches against `self.dict`.
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
            // Fill `self.parts` in reverse order,
            // so that `self.parts.pop()` yields
            // the tokens in their original order.
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

        // Will yield either `self.parts.last()` or
        // `self.tail.token()` if it could not be split.
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
mod tests {
    use super::*;
    use crate::engine::fts::tokenizer::{SimpleTokenizer, TextAnalyzer};

    #[test]
    fn splitting_compound_words_works() {
        let tokenizer = TextAnalyzer::from(SimpleTokenizer).filter(
            SplitCompoundWords::from_dictionary(["foo", "bar"])
                .expect("dictionary construction with valid patterns should succeed"),
        );

        {
            let mut stream = tokenizer.token_stream("");
            assert_eq!(stream.next(), None, "empty input should produce no tokens");
        }

        {
            let mut stream = tokenizer.token_stream("foo bar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foo",
                "first token of 'foo bar' should be 'foo'"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "bar",
                "second token of 'foo bar' should be 'bar'"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foo bar' should produce exactly two tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foo",
                "compound 'foobar' should split into 'foo' as the first token"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "bar",
                "compound 'foobar' should split into 'bar' as the second token"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobar' should produce exactly two tokens after splitting"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobarbaz");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foobarbaz",
                "'foobarbaz' cannot be fully split and should be returned as-is"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobarbaz' should produce exactly one token"
            );
        }

        {
            let mut stream = tokenizer.token_stream("baz foobar qux");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "baz",
                "first token of 'baz foobar qux' should be 'baz'"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "foo",
                "second token of 'baz foobar qux' should be 'foo' (from split 'foobar')"
            );
            assert_eq!(
                stream.next().expect("third token should be present").text,
                "bar",
                "third token of 'baz foobar qux' should be 'bar' (from split 'foobar')"
            );
            assert_eq!(
                stream.next().expect("fourth token should be present").text,
                "qux",
                "fourth token of 'baz foobar qux' should be 'qux'"
            );
            assert_eq!(
                stream.next(),
                None,
                "'baz foobar qux' should produce exactly four tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobar foobar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foo",
                "first token of 'foobar foobar' should be 'foo'"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "bar",
                "second token of 'foobar foobar' should be 'bar'"
            );
            assert_eq!(
                stream.next().expect("third token should be present").text,
                "foo",
                "third token of 'foobar foobar' should be 'foo' (second compound)"
            );
            assert_eq!(
                stream.next().expect("fourth token should be present").text,
                "bar",
                "fourth token of 'foobar foobar' should be 'bar' (second compound)"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobar foobar' should produce exactly four tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobar foo bar foobar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foo",
                "first token should be 'foo' (from first 'foobar')"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "bar",
                "second token should be 'bar' (from first 'foobar')"
            );
            assert_eq!(
                stream.next().expect("third token should be present").text,
                "foo",
                "third token should be standalone 'foo'"
            );
            assert_eq!(
                stream.next().expect("fourth token should be present").text,
                "bar",
                "fourth token should be standalone 'bar'"
            );
            assert_eq!(
                stream.next().expect("fifth token should be present").text,
                "foo",
                "fifth token should be 'foo' (from last 'foobar')"
            );
            assert_eq!(
                stream.next().expect("sixth token should be present").text,
                "bar",
                "sixth token should be 'bar' (from last 'foobar')"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobar foo bar foobar' should produce exactly six tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobazbar foo bar foobar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foobazbar",
                "'foobazbar' cannot be fully split and should be returned as-is"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "foo",
                "second token should be standalone 'foo'"
            );
            assert_eq!(
                stream.next().expect("third token should be present").text,
                "bar",
                "third token should be standalone 'bar'"
            );
            assert_eq!(
                stream.next().expect("fourth token should be present").text,
                "foo",
                "fourth token should be 'foo' (from split 'foobar')"
            );
            assert_eq!(
                stream.next().expect("fifth token should be present").text,
                "bar",
                "fifth token should be 'bar' (from split 'foobar')"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobazbar foo bar foobar' should produce exactly five tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("foobar qux foobar");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "foo",
                "first token should be 'foo' (from first 'foobar')"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "bar",
                "second token should be 'bar' (from first 'foobar')"
            );
            assert_eq!(
                stream.next().expect("third token should be present").text,
                "qux",
                "third token should be standalone 'qux'"
            );
            assert_eq!(
                stream.next().expect("fourth token should be present").text,
                "foo",
                "fourth token should be 'foo' (from last 'foobar')"
            );
            assert_eq!(
                stream.next().expect("fifth token should be present").text,
                "bar",
                "fifth token should be 'bar' (from last 'foobar')"
            );
            assert_eq!(
                stream.next(),
                None,
                "'foobar qux foobar' should produce exactly five tokens"
            );
        }

        {
            let mut stream = tokenizer.token_stream("barfoo");
            assert_eq!(
                stream.next().expect("first token should be present").text,
                "bar",
                "compound 'barfoo' should split into 'bar' as the first token"
            );
            assert_eq!(
                stream.next().expect("second token should be present").text,
                "foo",
                "compound 'barfoo' should split into 'foo' as the second token"
            );
            assert_eq!(
                stream.next(),
                None,
                "'barfoo' should produce exactly two tokens after splitting"
            );
        }
    }
}
