//! Stop word removal: drops tokens that carry little retrieval signal.
use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::error::{InternalError, InternalResult as Result};
use crate::fts::error::TokenizationFailedSnafu;
use crate::fts::tokenizer::{BoxTokenStream, Token, TokenFilter, TokenStream};

#[rustfmt::skip]
#[expect(
    clippy::needless_raw_string_hashes,
    reason = "generated data: every word literal is emitted as a uniformly hashed raw string so the generator never has to reason about quoting"
)]
#[expect(
    clippy::unicode_not_nfc,
    reason = "vendored data: some entries are not NFC upstream, and normalising them would change the words this crate claims to vendor unaltered"
)]
mod stopwords;

/// Recognised language codes paired with their word list.
///
/// Alphabetical by ISO 639-1 code, purely so a reader can find an entry; the
/// lookup does not depend on the ordering.
const LANGUAGES: &[(&str, &[&str])] = &[
    ("af", stopwords::AF),
    ("ar", stopwords::AR),
    ("bg", stopwords::BG),
    ("bn", stopwords::BN),
    ("br", stopwords::BR),
    ("ca", stopwords::CA),
    ("cs", stopwords::CS),
    ("da", stopwords::DA),
    ("de", stopwords::DE),
    ("el", stopwords::EL),
    ("en", stopwords::EN),
    ("eo", stopwords::EO),
    ("es", stopwords::ES),
    ("et", stopwords::ET),
    ("eu", stopwords::EU),
    ("fa", stopwords::FA),
    ("fi", stopwords::FI),
    ("fr", stopwords::FR),
    ("ga", stopwords::GA),
    ("gl", stopwords::GL),
    ("gu", stopwords::GU),
    ("ha", stopwords::HA),
    ("he", stopwords::HE),
    ("hi", stopwords::HI),
    ("hr", stopwords::HR),
    ("hu", stopwords::HU),
    ("hy", stopwords::HY),
    ("id", stopwords::ID),
    ("it", stopwords::IT),
    ("ja", stopwords::JA),
    ("ko", stopwords::KO),
    ("ku", stopwords::KU),
    ("la", stopwords::LA),
    ("lt", stopwords::LT),
    ("lv", stopwords::LV),
    ("mr", stopwords::MR),
    ("ms", stopwords::MS),
    ("nl", stopwords::NL),
    ("no", stopwords::NO),
    ("pl", stopwords::PL),
    ("pt", stopwords::PT),
    ("ro", stopwords::RO),
    ("ru", stopwords::RU),
    ("sk", stopwords::SK),
    ("sl", stopwords::SL),
    ("so", stopwords::SO),
    ("st", stopwords::ST),
    ("sv", stopwords::SV),
    ("sw", stopwords::SW),
    ("th", stopwords::TH),
    ("tl", stopwords::TL),
    ("tr", stopwords::TR),
    ("uk", stopwords::UK),
    ("ur", stopwords::UR),
    ("vi", stopwords::VI),
    ("yo", stopwords::YO),
    ("zh", stopwords::ZH),
    ("zu", stopwords::ZU),
];

/// Drops tokens whose text appears in a stop word set.
///
/// Construct it from a language code with [`StopWordFilter::for_lang`], or from
/// an explicit word list with [`StopWordFilter::new`].
///
/// WHY(Arc): [`TokenFilter::transform`] runs once per analysed document while
/// the word set is fixed at construction, so the set is shared by reference
/// count rather than copied into each stream.
#[derive(Clone)]
pub(crate) struct StopWordFilter {
    words: Arc<FxHashSet<String>>,
}

impl StopWordFilter {
    /// Builds a filter over an explicit word list.
    pub(crate) fn new<W, S>(words: W) -> Self
    where
        W: IntoIterator<Item = S>,
        S: Into<String>,
    {
        StopWordFilter {
            words: Arc::new(words.into_iter().map(Into::into).collect()),
        }
    }

    /// Builds a filter over the vendored word list for an ISO 639-1 `language`
    /// code, e.g. `"en"`.
    ///
    /// An unrecognised code is an error: silently filtering nothing would make a
    /// misconfigured index look like a correct one.
    #[expect(
        clippy::result_large_err,
        reason = "FTS error carries structured tokenization context"
    )]
    pub(crate) fn for_lang(language: &str) -> Result<Self> {
        let words = LANGUAGES
            .iter()
            .find(|&&(code, _)| code == language)
            .map(|&(_, words)| words)
            .ok_or_else(|| Self::unsupported(language))?;
        Ok(Self::new(words.iter().copied()))
    }

    fn unsupported(language: &str) -> InternalError {
        let supported = LANGUAGES
            .iter()
            .map(|&(code, _)| code)
            .collect::<Vec<_>>()
            .join(", ");
        InternalError::from(
            TokenizationFailedSnafu {
                message: format!(
                    "Filter Stopwords has no word list for language {language:?}; supported codes: {supported}"
                ),
            }
            .build(),
        )
    }
}

// WHY: a derived `Debug` would render every vendored word, so a single log line
// carrying this filter would run to tens of thousands of entries.
impl fmt::Debug for StopWordFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StopWordFilter")
            .field("words", &self.words.len())
            .finish()
    }
}

impl TokenFilter for StopWordFilter {
    fn transform<'a>(&self, token_stream: BoxTokenStream<'a>) -> BoxTokenStream<'a> {
        BoxTokenStream::from(StopWordFilterStream {
            words: Arc::clone(&self.words),
            tail: token_stream,
        })
    }
}

/// Stream yielding only those tokens of `tail` that are not stop words.
pub(crate) struct StopWordFilterStream<'a> {
    words: Arc<FxHashSet<String>>,
    tail: BoxTokenStream<'a>,
}

impl TokenStream for StopWordFilterStream<'_> {
    /// INVARIANT: only whole tokens are dropped. A token that survives reaches
    /// the caller with its text, offsets and position untouched.
    fn advance(&mut self) -> bool {
        while self.tail.advance() {
            if !self.words.contains(self.tail.token().text.as_str()) {
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
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions: index into known-length token vectors"
)]
mod tests {
    use super::{LANGUAGES, StopWordFilter};
    use crate::fts::tokenizer::tests::{assert_token, collect_tokens};
    use crate::fts::tokenizer::{SimpleTokenizer, TextAnalyzer, Token};

    fn filtered(text: &str, filter: StopWordFilter) -> Vec<Token> {
        let analyzer = TextAnalyzer::from(SimpleTokenizer).filter(filter);
        collect_tokens(analyzer.token_stream(text))
    }

    fn english() -> StopWordFilter {
        let Ok(filter) = StopWordFilter::for_lang("en") else {
            panic!("`en` must resolve to a vendored word list");
        };
        filter
    }

    #[test]
    fn explicit_word_list_removes_only_listed_words() {
        let filter = StopWordFilter::new(["the".to_string(), "fox".to_string()]);
        let tokens = filtered("the quick brown fox", filter);

        let texts: Vec<&str> = tokens.iter().map(|token| token.text.as_str()).collect();
        assert_eq!(texts, ["quick", "brown"]);
    }

    #[test]
    fn surviving_tokens_keep_their_offsets_and_positions() {
        let tokens = filtered("the quick brown fox", english());

        // NOTE: dropping `the` neither renumbers the survivors nor rewrites
        // their offsets — each still reports where it came from in the source.
        assert_eq!(tokens.len(), 3);
        assert_token(&tokens[0], 1, "quick", 4, 9);
        assert_token(&tokens[1], 2, "brown", 10, 15);
        assert_token(&tokens[2], 3, "fox", 16, 19);
    }

    #[test]
    fn a_stream_of_only_stop_words_yields_nothing() {
        assert!(filtered("the and of", english()).is_empty());
    }

    #[test]
    fn an_empty_word_list_removes_nothing() {
        let tokens = filtered(
            "the quick brown fox",
            StopWordFilter::new(Vec::<String>::new()),
        );
        assert_eq!(tokens.len(), 4);
    }

    #[test]
    fn a_filter_survives_cloning_and_reuse() {
        let filter = english();
        let reused = filter.clone();

        assert_eq!(filtered("the fox", filter).len(), 1);
        assert_eq!(filtered("the fox", reused).len(), 1);
    }

    #[test]
    fn every_declared_language_resolves_to_a_non_empty_list() {
        for &(code, words) in LANGUAGES {
            assert!(!words.is_empty(), "{code} has an empty word list");
            assert!(
                StopWordFilter::for_lang(code).is_ok(),
                "{code} is declared but does not resolve"
            );
        }
    }

    #[test]
    fn language_codes_are_unique() {
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|&(code, _)| code).collect();
        let declared = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), declared, "a language code is declared twice");
    }

    #[test]
    fn an_unknown_language_is_an_error_not_an_empty_filter() {
        assert!(StopWordFilter::for_lang("klingon").is_err());
        assert!(StopWordFilter::for_lang("").is_err());
        // NOTE: codes match exactly. An uppercased code is rejected rather than
        // accepted-and-ignored, so a caller finds out at configuration time.
        assert!(StopWordFilter::for_lang("EN").is_err());
    }

    #[test]
    fn the_unsupported_error_names_the_codes_that_would_have_worked() {
        let Err(error) = StopWordFilter::for_lang("klingon") else {
            panic!("`klingon` is not a vendored language");
        };
        let message = error.to_string();
        assert!(message.contains("klingon"), "{message}");
        assert!(message.contains(", en,"), "{message}");
    }

    #[test]
    fn debug_reports_the_word_count_not_the_words() {
        let rendered = format!(
            "{:?}",
            StopWordFilter::new(["alpha".to_string(), "beta".to_string()])
        );
        assert!(rendered.contains('2'), "{rendered}");
        assert!(!rendered.contains("alpha"), "{rendered}");
    }
}
