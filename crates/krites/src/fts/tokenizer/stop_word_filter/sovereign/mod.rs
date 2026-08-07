//! Stop word removal filter with multi-language support — sovereign
//! implementation.
//!
//! Same public behavior as [`super::derived`]: removes stop words (common
//! low-signal words) from the token stream, for any of 58 languages
//! identified by ISO 639-1 code, or a custom word set. The word lists
//! themselves are vendored, not re-authored — see `NOTICE.md` for their
//! actual (third-party, MIT-licensed) provenance.

#[rustfmt::skip]
#[expect(
    clippy::needless_raw_string_hashes,
    clippy::unicode_not_nfc,
    reason = "stopword data imported verbatim from stopwords-iso — preserving source encoding"
)]
mod stopwords;

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::super::{BoxTokenStream, Token, TokenFilter, TokenStream};
use crate::error::InternalResult as Result;
use crate::fts::error::TokenizationFailedSnafu;

/// Removes stop words from a token stream.
///
/// Supports 58 languages via built-in word lists (see [`stopwords`]), or a
/// caller-supplied word set via [`StopWordFilter::new`].
#[derive(Clone)]
pub(crate) struct StopWordFilter {
    stop_set: Arc<FxHashSet<String>>,
}

impl StopWordFilter {
    /// Builds a [`StopWordFilter`] for the given ISO 639-1 language code.
    #[expect(
        clippy::result_large_err,
        reason = "FTS error carries structured tokenization context"
    )]
    pub(crate) fn for_lang(language: &str) -> Result<Self> {
        let list: &[&str] = match language {
            "af" => stopwords::AF,
            "ar" => stopwords::AR,
            "bg" => stopwords::BG,
            "bn" => stopwords::BN,
            "br" => stopwords::BR,
            "ca" => stopwords::CA,
            "cs" => stopwords::CS,
            "da" => stopwords::DA,
            "de" => stopwords::DE,
            "el" => stopwords::EL,
            "en" => stopwords::EN,
            "eo" => stopwords::EO,
            "es" => stopwords::ES,
            "et" => stopwords::ET,
            "eu" => stopwords::EU,
            "fa" => stopwords::FA,
            "fi" => stopwords::FI,
            "fr" => stopwords::FR,
            "ga" => stopwords::GA,
            "gl" => stopwords::GL,
            "gu" => stopwords::GU,
            "ha" => stopwords::HA,
            "he" => stopwords::HE,
            "hi" => stopwords::HI,
            "hr" => stopwords::HR,
            "hu" => stopwords::HU,
            "hy" => stopwords::HY,
            "id" => stopwords::ID,
            "it" => stopwords::IT,
            "ja" => stopwords::JA,
            "ko" => stopwords::KO,
            "ku" => stopwords::KU,
            "la" => stopwords::LA,
            "lt" => stopwords::LT,
            "lv" => stopwords::LV,
            "mr" => stopwords::MR,
            "ms" => stopwords::MS,
            "nl" => stopwords::NL,
            "no" => stopwords::NO,
            "pl" => stopwords::PL,
            "pt" => stopwords::PT,
            "ro" => stopwords::RO,
            "ru" => stopwords::RU,
            "sk" => stopwords::SK,
            "sl" => stopwords::SL,
            "so" => stopwords::SO,
            "st" => stopwords::ST,
            "sv" => stopwords::SV,
            "sw" => stopwords::SW,
            "th" => stopwords::TH,
            "tl" => stopwords::TL,
            "tr" => stopwords::TR,
            "uk" => stopwords::UK,
            "ur" => stopwords::UR,
            "vi" => stopwords::VI,
            "yo" => stopwords::YO,
            "zh" => stopwords::ZH,
            "zu" => stopwords::ZU,
            unsupported => {
                return Err(TokenizationFailedSnafu {
                    message: format!("unsupported stop-word language code: {unsupported:?}"),
                }
                .build()
                .into());
            }
        };
        Ok(Self::new(list.iter().copied().map(str::to_owned)))
    }

    /// Builds a [`StopWordFilter`] from a caller-supplied word set.
    pub(crate) fn new<I>(words: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        Self {
            stop_set: Arc::new(words.into_iter().collect()),
        }
    }
}

impl TokenFilter for StopWordFilter {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(StopWordFilterStream {
            stop_set: Arc::clone(&self.stop_set),
            inner: token_stream,
        })
    }
}

pub(crate) struct StopWordFilterStream<'a> {
    stop_set: Arc<FxHashSet<String>>,
    inner: BoxTokenStream<'a>,
}

impl StopWordFilterStream<'_> {
    fn is_significant(&self, token: &Token) -> bool {
        !self.stop_set.contains(&token.text)
    }
}

impl TokenStream for StopWordFilterStream<'_> {
    fn advance(&mut self) -> bool {
        loop {
            if !self.inner.advance() {
                return false;
            }
            if self.is_significant(self.inner.token()) {
                return true;
            }
        }
    }

    fn token(&self) -> &Token {
        self.inner.token()
    }

    fn token_mut(&mut self) -> &mut Token {
        self.inner.token_mut()
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions: index into known-length token vectors"
)]
#[expect(
    clippy::expect_used,
    reason = "test assertions: fixture setup must not silently pass"
)]
mod tests {
    use crate::fts::tokenizer::tests::assert_token;
    use crate::fts::tokenizer::{SimpleTokenizer, StopWordFilter, TextAnalyzer, Token};

    #[test]
    fn filters_common_english_stop_words() {
        let tokens = run("she sat by the window and watched the rain fall quietly");
        // "she", "by", "the", "and", "the" are removed; five words survive.
        assert_eq!(tokens.len(), 6);
        assert_token(&tokens[0], 1, "sat", 4, 7);
        assert_token(&tokens[1], 4, "window", 15, 21);
        assert_token(&tokens[2], 6, "watched", 26, 33);
        assert_token(&tokens[3], 8, "rain", 38, 42);
        assert_token(&tokens[4], 9, "fall", 43, 47);
        assert_token(&tokens[5], 10, "quietly", 48, 55);
    }

    #[test]
    fn every_declared_language_code_resolves() {
        const CODES: [&str; 58] = [
            "af", "ar", "bg", "bn", "br", "ca", "cs", "da", "de", "el", "en", "eo", "es", "et",
            "eu", "fa", "fi", "fr", "ga", "gl", "gu", "ha", "he", "hi", "hr", "hu", "hy", "id",
            "it", "ja", "ko", "ku", "la", "lt", "lv", "mr", "ms", "nl", "no", "pl", "pt", "ro",
            "ru", "sk", "sl", "so", "st", "sv", "sw", "th", "tl", "tr", "uk", "ur", "vi", "yo",
            "zh", "zu",
        ];
        for code in CODES {
            let built = StopWordFilter::for_lang(code);
            assert!(
                built.is_ok(),
                "{code:?} should resolve, got {:?}",
                built.err()
            );
        }
    }

    #[test]
    fn unknown_language_code_is_an_error() {
        assert!(StopWordFilter::for_lang("not-a-real-code").is_err());
    }

    #[test]
    fn custom_word_set_overrides_built_in_lists() {
        let tokens = token_stream_helper(
            "the quick brown fox",
            ["the".to_string(), "quick".to_string()],
        );
        assert_eq!(tokens.len(), 2);
        assert_token(&tokens[0], 2, "brown", 10, 15);
        assert_token(&tokens[1], 3, "fox", 16, 19);
    }

    fn run(text: &str) -> Vec<Token> {
        StopWordFilter::for_lang("en")
            .map(|filter| {
                use crate::fts::tokenizer::tests::collect_tokens;
                let analyzer = TextAnalyzer::from(SimpleTokenizer).filter(filter);
                collect_tokens(analyzer.token_stream(text))
            })
            .expect("english stopword list must load")
    }

    fn token_stream_helper<I: IntoIterator<Item = String>>(text: &str, words: I) -> Vec<Token> {
        use crate::fts::tokenizer::tests::collect_tokens;
        let analyzer = TextAnalyzer::from(SimpleTokenizer).filter(StopWordFilter::new(words));
        collect_tokens(analyzer.token_stream(text))
    }
}
