//! Text tokenization pipeline for full-text search indexing.
mod alphanum_only;
mod ascii_folding_filter;
mod empty_tokenizer;
mod lower_caser;
mod ngram_tokenizer;
mod raw_tokenizer;
mod remove_long;
mod simple_tokenizer;
mod split_compound_words;
mod stemmer;
mod stop_word_filter;
mod tokenized_string;
mod tokenizer_impl;
mod whitespace_tokenizer;

pub(crate) use self::alphanum_only::AlphaNumOnlyFilter;
pub(crate) use self::ascii_folding_filter::AsciiFoldingFilter;
pub(crate) use self::lower_caser::LowerCaser;
pub(crate) use self::ngram_tokenizer::NgramTokenizer;
pub(crate) use self::raw_tokenizer::RawTokenizer;
pub(crate) use self::remove_long::RemoveLongFilter;
pub(crate) use self::simple_tokenizer::SimpleTokenizer;
pub(crate) use self::split_compound_words::SplitCompoundWords;
pub(crate) use self::stemmer::{Language, Stemmer};
pub(crate) use self::stop_word_filter::StopWordFilter;
pub(crate) use self::tokenizer_impl::{
    BoxTokenFilter, BoxTokenStream, TextAnalyzer, Token, TokenFilter, TokenStream, Tokenizer,
};
pub(crate) use self::whitespace_tokenizer::WhitespaceTokenizer;

#[cfg(test)]
pub(crate) mod tests {
    use crate::engine::fts::tokenizer::{BoxTokenStream, Token};

    /// Collect all tokens from a stream into a `Vec`.
    ///
    /// Shared by every tokenizer test module so the collection boilerplate
    /// lives in exactly one place.
    pub(crate) fn collect_tokens(mut stream: BoxTokenStream<'_>) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut add_token = |token: &Token| {
            tokens.push(token.clone());
        };
        stream.process(&mut add_token);
        tokens
    }

    pub(crate) fn assert_token(token: &Token, position: usize, text: &str, from: usize, to: usize) {
        assert_eq!(
            token.position, position,
            "expected position {} but {:?}",
            position, token
        );
        assert_eq!(token.text, text, "expected text {} but {:?}", text, token);
        assert_eq!(
            token.offset_from, from,
            "expected offset_from {} but {:?}",
            from, token
        );
        assert_eq!(
            token.offset_to, to,
            "expected offset_to {} but {:?}",
            to, token
        );
    }
}
