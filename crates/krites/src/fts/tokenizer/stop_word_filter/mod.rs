//! Stop word removal filter with multi-language support.
//!
//! The word lists are vendored from [stopwords-iso](https://github.com/stopwords-iso/stopwords-iso/)
//! under MIT — see `sovereign/NOTICE.md` for the attribution and license text.
//! They replaced a CozoDB-derived copy carrying the same 21,707 literals across
//! 58 languages while attributing them incorrectly; the replacement was verified
//! token-multiset identical per language before the derived copy was deleted, so
//! retiring it changed which project is credited and nothing else.
mod sovereign;
pub(crate) use sovereign::StopWordFilter;
