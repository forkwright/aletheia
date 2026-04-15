//! Core tokenizer trait implementations.
//!
//! Defines the foundational traits (`Tokenizer`, `TokenFilter`, `TokenStream`)
//! and wrapper types (`TextAnalyzer`, `BoxTokenStream`, `BoxTokenFilter`) that
//! form the FTS text analysis pipeline.
use std::borrow::{Borrow, BorrowMut};
use std::iter;
use std::ops::{Deref, DerefMut};

use compact_str::CompactString;
use rustc_hash::FxHashSet;

use crate::fts::tokenizer::empty_tokenizer::EmptyTokenizer;

/// A single token produced by the tokenization pipeline.
///
/// Carries the token text along with source offsets and position metadata.
/// Offsets are byte indices into the original input string.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq, Hash)]
pub(crate) struct Token {
    /// Byte offset of the first character of the token in the source text.
    /// Token filters must not modify offsets.
    pub(crate) offset_from: usize,
    /// Byte offset one past the last character: `&text[offset_from..offset_to]`.
    /// Token filters must not modify offsets.
    pub(crate) offset_to: usize,
    /// Zero-based position in the token sequence.
    pub(crate) position: usize,
    /// The token text content, possibly transformed by filters.
    pub(crate) text: String,
    /// Span length in original tokens (typically 1; n-gram tokenizers may differ).
    pub(crate) position_length: usize,
}

impl Default for Token {
    fn default() -> Token {
        Token {
            offset_from: 0,
            offset_to: 0,
            position: usize::MAX,
            text: String::with_capacity(200),
            position_length: 1,
        }
    }
}

/// Text analysis pipeline: a [`Tokenizer`] followed by zero or more [`TokenFilter`]s.
///
/// The tokenizer splits input text into a raw token stream. Each filter in
/// sequence then transforms the stream (lowercasing, stemming, stop-word
/// removal, etc.).
pub(crate) struct TextAnalyzer {
    pub(crate) tokenizer: Box<dyn Tokenizer>,
    pub(crate) token_filters: Vec<BoxTokenFilter>,
}

impl Default for TextAnalyzer {
    fn default() -> TextAnalyzer {
        TextAnalyzer::from(EmptyTokenizer)
    }
}

impl<T: Tokenizer> From<T> for TextAnalyzer {
    fn from(tokenizer: T) -> Self {
        TextAnalyzer::new(tokenizer, Vec::new())
    }
}

impl TextAnalyzer {
    /// Creates a new `TextAnalyzer` given a tokenizer and a vector of `BoxTokenFilter`.
    ///
    /// When creating a `TextAnalyzer` from a `Tokenizer` alone, prefer using
    /// `TextAnalyzer::from(tokenizer)`.
    pub(crate) fn new<T: Tokenizer>(
        tokenizer: T,
        token_filters: Vec<BoxTokenFilter>,
    ) -> TextAnalyzer {
        TextAnalyzer {
            tokenizer: Box::new(tokenizer),
            token_filters,
        }
    }

    /// Appends a token filter to the pipeline, returning the modified analyzer.
    #[cfg(test)]
    pub(crate) fn filter<F: Into<BoxTokenFilter>>(mut self, token_filter: F) -> Self {
        self.token_filters.push(token_filter.into());
        self
    }

    /// Creates a token stream for a given `str`.
    pub(crate) fn token_stream<'a>(&self, text: &'a str) -> BoxTokenStream<'a> {
        let mut token_stream = self.tokenizer.token_stream(text);
        for token_filter in &self.token_filters {
            token_stream = token_filter.transform(token_stream);
        }
        token_stream
    }
    /// Produces the set of unique token n-grams (sliding windows of size `n`).
    pub(crate) fn unique_ngrams(&self, text: &str, n: usize) -> FxHashSet<Vec<CompactString>> {
        let mut token_stream = self.token_stream(text);
        let mut coll: Vec<CompactString> = vec![];
        while let Some(token) = token_stream.next() {
            coll.push(CompactString::from(token.text.as_str()));
        }

        if n == 1 {
            coll.iter().map(|x| vec![x.clone()]).collect()
        } else if n >= coll.len() {
            iter::once(coll).collect()
        } else {
            let mut ret = FxHashSet::default();
            for chunk in coll.windows(n) {
                ret.insert(chunk.to_vec());
            }
            ret
        }
    }
}

impl Clone for TextAnalyzer {
    fn clone(&self) -> Self {
        TextAnalyzer {
            tokenizer: self.tokenizer.box_clone(),
            token_filters: self
                .token_filters
                .iter()
                .map(|token_filter| token_filter.box_clone())
                .collect(),
        }
    }
}

/// Splits input text into a stream of [`Token`]s.
pub(crate) trait Tokenizer: 'static + Send + Sync + TokenizerClone {
    /// Creates a token stream for a given `str`.
    fn token_stream<'a>(&self, text: &'a str) -> BoxTokenStream<'a>;
}

/// Object-safe cloning for [`Tokenizer`] trait objects.
pub(crate) trait TokenizerClone {
    fn box_clone(&self) -> Box<dyn Tokenizer>;
}

impl<T: Tokenizer + Clone> TokenizerClone for T {
    fn box_clone(&self) -> Box<dyn Tokenizer> {
        Box::new(self.clone())
    }
}

impl<'a> TokenStream for Box<dyn TokenStream + 'a> {
    fn advance(&mut self) -> bool {
        let token_stream: &mut dyn TokenStream = self.borrow_mut();
        token_stream.advance()
    }

    fn token<'b>(&'b self) -> &'b Token {
        let token_stream: &'b (dyn TokenStream + 'a) = self.borrow();
        token_stream.token()
    }

    fn token_mut<'b>(&'b mut self) -> &'b mut Token {
        let token_stream: &'b mut (dyn TokenStream + 'a) = self.borrow_mut();
        token_stream.token_mut()
    }
}

/// Type-erased [`TokenStream`], used as the uniform return type across the pipeline.
pub(crate) struct BoxTokenStream<'a>(Box<dyn TokenStream + 'a>);

impl<'a, T> From<T> for BoxTokenStream<'a>
where
    T: TokenStream + 'a,
{
    fn from(token_stream: T) -> BoxTokenStream<'a> {
        BoxTokenStream(Box::new(token_stream))
    }
}

impl<'a> Deref for BoxTokenStream<'a> {
    type Target = dyn TokenStream + 'a;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
impl DerefMut for BoxTokenStream<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.0
    }
}

/// Type-erased [`TokenFilter`], used to store heterogeneous filter chains.
pub(crate) struct BoxTokenFilter(Box<dyn TokenFilter>);

impl Deref for BoxTokenFilter {
    type Target = dyn TokenFilter;

    fn deref(&self) -> &dyn TokenFilter {
        &*self.0
    }
}

impl<T: TokenFilter> From<T> for BoxTokenFilter {
    fn from(tokenizer: T) -> BoxTokenFilter {
        BoxTokenFilter(Box::new(tokenizer))
    }
}

/// Consumable stream of [`Token`]s produced by a [`Tokenizer`] or [`TokenFilter`].
pub(crate) trait TokenStream {
    /// Advance to the next token
    ///
    /// Returns false if there are no other tokens.
    fn advance(&mut self) -> bool;

    /// Returns a reference to the current token.
    fn token(&self) -> &Token;

    /// Returns a mutable reference to the current token.
    fn token_mut(&mut self) -> &mut Token;

    /// Advances and returns the next token, or `None` when exhausted.
    fn next(&mut self) -> Option<&Token> {
        if self.advance() {
            Some(self.token())
        } else {
            None
        }
    }

    /// Iterates over all tokens and calls `callback` for each.
    ///
    /// Only used in tests.
    #[cfg(test)]
    fn process(&mut self, callback: &mut dyn FnMut(&Token)) {
        while self.advance() {
            callback(self.token());
        }
    }
}

/// Object-safe cloning for [`TokenFilter`] trait objects.
pub(crate) trait TokenFilterClone {
    fn box_clone(&self) -> BoxTokenFilter;
}

/// Transforms a [`TokenStream`] by modifying, filtering, or expanding tokens.
pub(crate) trait TokenFilter: 'static + Send + Sync + TokenFilterClone {
    /// Wraps a token stream and returns the modified one.
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a>;
}

impl<T: TokenFilter + Clone> TokenFilterClone for T {
    fn box_clone(&self) -> BoxTokenFilter {
        BoxTokenFilter::from(self.clone())
    }
}

#[cfg(test)]
mod test {
    use super::Token;

    #[test]
    fn clone() {
        let t1 = Token {
            position: 1,
            offset_from: 2,
            offset_to: 3,
            text: "abc".to_string(),
            position_length: 1,
        };
        let t2 = t1.clone();

        assert_eq!(t1.position, t2.position);
        assert_eq!(t1.offset_from, t2.offset_from);
        assert_eq!(t1.offset_to, t2.offset_to);
        assert_eq!(t1.text, t2.text);
    }
}
