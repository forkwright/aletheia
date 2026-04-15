//! Full-text search subsystem.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use compact_str::CompactString;

use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fts::tokenizer::TextAnalyzer;

pub(crate) mod ast;
mod config;
pub(crate) mod error;
pub(crate) mod indexing;
pub(crate) mod tokenizer;

/// Manifest describing an FTS index: which relation it belongs to, the
/// text extractor expression, and the tokenizer + filter pipeline.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FtsIndexManifest {
    pub(crate) base_relation: CompactString,
    pub(crate) index_name: CompactString,
    pub(crate) extractor: String,
    pub(crate) tokenizer: TokenizerConfig,
    pub(crate) filters: Vec<TokenizerConfig>,
}

/// Configuration for a tokenizer or token filter, including its name and arguments.
///
/// The `name` selects which tokenizer or filter to instantiate (e.g. `"Simple"`,
/// `"Stemmer"`, `"Stopwords"`). The `args` carry filter-specific parameters
/// (language codes, length limits, word lists, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TokenizerConfig {
    pub name: CompactString,
    pub args: Vec<DataValue>,
}

/// Two-level cache for built [`TextAnalyzer`] pipelines.
///
/// First checks by index name (fast path for repeated queries against the same
/// index), then falls back to a content-addressed hash of the tokenizer + filter
/// config (deduplicates structurally identical pipelines with different names).
#[derive(Default)]
pub(crate) struct TokenizerCache {
    pub(crate) named_cache: RwLock<HashMap<CompactString, Arc<TextAnalyzer>>>,
    pub(crate) hashed_cache: RwLock<HashMap<Vec<u8>, Arc<TextAnalyzer>>>,
}

impl TokenizerCache {
    #[expect(
        clippy::result_large_err,
        reason = "FTS error carries structured tokenization context"
    )]
    pub(crate) fn get(
        &self,
        tokenizer_name: &str,
        tokenizer: &TokenizerConfig,
        filters: &[TokenizerConfig],
    ) -> Result<Arc<TextAnalyzer>> {
        {
            let idx_cache = self
                .named_cache
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(analyzer) = idx_cache.get(tokenizer_name) {
                return Ok(analyzer.clone());
            }
        }
        let hash = tokenizer.config_hash(filters);
        {
            let hashed_cache = self
                .hashed_cache
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(analyzer) = hashed_cache.get(hash.as_ref()) {
                let mut idx_cache = self
                    .named_cache
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                idx_cache.insert(tokenizer_name.into(), analyzer.clone());
                return Ok(analyzer.clone());
            }
        }
        {
            let analyzer = Arc::new(tokenizer.build(filters)?);
            let mut hashed_cache = self
                .hashed_cache
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            hashed_cache.insert(hash.as_ref().to_vec(), analyzer.clone());
            let mut idx_cache = self
                .named_cache
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            idx_cache.insert(tokenizer_name.into(), analyzer.clone());
            Ok(analyzer)
        }
    }
}
