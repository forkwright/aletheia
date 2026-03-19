//! Folds Unicode characters to ASCII equivalents.
use std::mem;

use super::{BoxTokenStream, Token, TokenFilter, TokenStream};

/// This class converts alphabetic, numeric, and symbolic Unicode characters
/// which are not in the first 127 ASCII characters (the "Basic Latin" Unicode
/// block) into their ASCII equivalents, if one exists.
#[derive(Clone)]
pub(crate) struct AsciiFoldingFilter;

impl TokenFilter for AsciiFoldingFilter {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        From::from(AsciiFoldingFilterTokenStream {
            tail: token_stream,
            buffer: String::with_capacity(100),
        })
    }
}

pub(crate) struct AsciiFoldingFilterTokenStream<'a> {
    buffer: String,
    tail: BoxTokenStream<'a>,
}

impl<'a> TokenStream for AsciiFoldingFilterTokenStream<'a> {
    fn advance(&mut self) -> bool {
        if !self.tail.advance() {
            return false;
        }
        if !self.token_mut().text.is_ascii() {
            // ignore its already ascii
            to_ascii(&self.tail.token().text, &mut self.buffer);
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

mod fold_table;
use fold_table::fold_non_ascii_char;

#[cfg(test)]
mod tests;

// https://github.com/apache/lucene-solr/blob/master/lucene/analysis/common/src/java/org/apache/lucene/analysis/miscellaneous/ASCIIFoldingFilter.java#L187
pub(crate) fn to_ascii(text: &str, output: &mut String) {
    output.clear();

    for c in text.chars() {
        if let Some(folded) = fold_non_ascii_char(c) {
            output.push_str(folded);
        } else {
            output.push(c);
        }
    }
}
