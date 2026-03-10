//! Full-text search subsystem.
use crate::engine::data::memcmp::MemCmpEncoder;
use crate::engine::data::value::DataValue;
use crate::engine::error::DbResult as Result;
use crate::engine::fts::error::TokenizationFailedSnafu;
use crate::engine::fts::tokenizer::{
    AlphaNumOnlyFilter, AsciiFoldingFilter, BoxTokenFilter, Language, LowerCaser, NgramTokenizer,
    RawTokenizer, RemoveLongFilter, SimpleTokenizer, SplitCompoundWords, Stemmer, StopWordFilter,
    TextAnalyzer, Tokenizer, WhitespaceTokenizer,
};
use compact_str::CompactString;
use sha2::digest::FixedOutput;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub(crate) mod ast;
pub(crate) mod error;
pub(crate) mod indexing;
pub(crate) mod tokenizer;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FtsIndexManifest {
    pub(crate) base_relation: CompactString,
    pub(crate) index_name: CompactString,
    pub(crate) extractor: String,
    pub(crate) tokenizer: TokenizerConfig,
    pub(crate) filters: Vec<TokenizerConfig>,
}

/// Configuration for a tokenizer or token filter, including its name and arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TokenizerConfig {
    pub name: CompactString,
    pub args: Vec<DataValue>,
}

impl TokenizerConfig {
    pub(crate) fn config_hash(&self, filters: &[Self]) -> impl AsRef<[u8]> {
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        let mut args_vec = vec![];
        for arg in &self.args {
            args_vec.encode_datavalue(arg);
        }
        hasher.update(&args_vec);
        for filter in filters {
            hasher.update(filter.name.as_bytes());
            args_vec.clear();
            for arg in &filter.args {
                args_vec.encode_datavalue(arg);
            }
            hasher.update(&args_vec);
        }
        hasher.finalize_fixed()
    }
    pub(crate) fn build(&self, filters: &[Self]) -> Result<TextAnalyzer> {
        let tokenizer = self.construct_tokenizer()?;
        let token_filters = filters
            .iter()
            .map(|filter| filter.construct_token_filter())
            .collect::<Result<Vec<_>>>()?;
        Ok(TextAnalyzer {
            tokenizer,
            token_filters,
        })
    }
    pub(crate) fn construct_tokenizer(&self) -> Result<Box<dyn Tokenizer>> {
        Ok(match &self.name as &str {
            "Raw" => Box::new(RawTokenizer),
            "Simple" => Box::new(SimpleTokenizer),
            "Whitespace" => Box::new(WhitespaceTokenizer),
            "NGram" => {
                let min_gram = self
                    .args
                    .first()
                    .unwrap_or(&DataValue::from(1))
                    .get_int()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "First argument `min_gram` must be an integer".to_string(),
                        )
                    })?;
                let max_gram = self
                    .args
                    .get(1)
                    .unwrap_or(&DataValue::from(min_gram))
                    .get_int()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "Second argument `max_gram` must be an integer".to_string(),
                        )
                    })?;
                let prefix_only = self
                    .args
                    .get(2)
                    .unwrap_or(&DataValue::Bool(false))
                    .get_bool()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "Third argument `prefix_only` must be a boolean".to_string(),
                        )
                    })?;
                if min_gram < 1 {
                    return Err(Box::new(
                        TokenizationFailedSnafu {
                            message: "min_gram must be >= 1".to_string(),
                        }
                        .build(),
                    ));
                }
                if max_gram < min_gram {
                    return Err(Box::new(
                        TokenizationFailedSnafu {
                            message: "max_gram must be >= min_gram".to_string(),
                        }
                        .build(),
                    ));
                }
                Box::new(NgramTokenizer::new(
                    min_gram as usize,
                    max_gram as usize,
                    prefix_only,
                ))
            }
            _ => {
                return Err(Box::new(
                    TokenizationFailedSnafu {
                        message: format!("Unknown tokenizer: {}", self.name),
                    }
                    .build(),
                ))
            }
        })
    }
    pub(crate) fn construct_token_filter(&self) -> Result<BoxTokenFilter> {
        Ok(match &self.name as &str {
            "AlphaNumOnly" => AlphaNumOnlyFilter.into(),
            "AsciiFolding" => AsciiFoldingFilter.into(),
            "LowerCase" | "Lowercase" => LowerCaser.into(),
            "RemoveLong" => RemoveLongFilter::limit(
                self.args
                    .first()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "Missing first argument `min_length`".to_string(),
                        )
                    })?
                    .get_int()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "First argument `min_length` must be an integer".to_string(),
                        )
                    })? as usize,
            )
            .into(),
            "SplitCompoundWords" => {
                let mut list_values = Vec::new();
                match self.args.first().ok_or_else(|| {
                    crate::engine::error::AdhocError(
                        "Missing first argument `compound_words_list`".to_string(),
                    )
                })? {
                    DataValue::List(l) => {
                        for v in l {
                            list_values.push(
                                v.get_str()
                                    .ok_or_else(|| {
                                        crate::engine::error::AdhocError("First argument `compound_words_list` must be a list of strings".to_string())
                                    })?,
                            );
                        }
                    }
                    _ => {
                        return Err(Box::new(
                            TokenizationFailedSnafu {
                                message: "First argument `compound_words_list` must be a list of strings".to_string(),
                            }
                            .build(),
                        ))
                    }
                }
                SplitCompoundWords::from_dictionary(list_values)
                    .map_err(|e| {
                        crate::engine::error::AdhocError(format!(
                            "Failed to load dictionary: {}",
                            e
                        ))
                    })?
                    .into()
            }
            "Stemmer" => {
                let language = match self
                    .args
                    .first()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "Missing first argument `language` to Stemmer".to_string(),
                        )
                    })?
                    .get_str()
                    .ok_or_else(|| {
                        crate::engine::error::AdhocError(
                            "First argument `language` to Stemmer must be a string".to_string(),
                        )
                    })?
                    .to_lowercase()
                    .as_str()
                {
                    "arabic" => Language::Arabic,
                    "danish" => Language::Danish,
                    "dutch" => Language::Dutch,
                    "english" => Language::English,
                    "finnish" => Language::Finnish,
                    "french" => Language::French,
                    "german" => Language::German,
                    "greek" => Language::Greek,
                    "hungarian" => Language::Hungarian,
                    "italian" => Language::Italian,
                    "norwegian" => Language::Norwegian,
                    "portuguese" => Language::Portuguese,
                    "romanian" => Language::Romanian,
                    "russian" => Language::Russian,
                    "spanish" => Language::Spanish,
                    "swedish" => Language::Swedish,
                    "tamil" => Language::Tamil,
                    "turkish" => Language::Turkish,
                    lang => {
                        return Err(Box::new(
                            TokenizationFailedSnafu {
                                message: format!("Unsupported language: {}", lang),
                            }
                            .build(),
                        ))
                    }
                };
                Stemmer::new(language).into()
            }
            "Stopwords" => {
                match self.args.first().ok_or_else(|| {
                    crate::engine::error::AdhocError(
                        "Filter Stopwords requires language name or a list of stopwords"
                            .to_string(),
                    )
                })? {
                    DataValue::Str(name) => StopWordFilter::for_lang(name)?.into(),
                    DataValue::List(l) => {
                        let mut stopwords = Vec::new();
                        for v in l {
                            stopwords.push(
                                v.get_str()
                                    .ok_or_else(|| {
                                        crate::engine::error::AdhocError(
                                            "First argument `stopwords` must be a list of strings"
                                                .to_string(),
                                        )
                                    })?
                                    .to_string(),
                            );
                        }
                        StopWordFilter::new(stopwords).into()
                    }
                    _ => {
                        return Err(Box::new(
                            TokenizationFailedSnafu {
                                message: "Filter Stopwords requires language name or a list of stopwords".to_string(),
                            }
                            .build(),
                        ))
                    }
                }
            }
            _ => {
                return Err(Box::new(
                    TokenizationFailedSnafu {
                        message: format!("Unknown token filter: {:?}", self.name),
                    }
                    .build(),
                ))
            }
        })
    }
}


#[derive(Default)]
pub(crate) struct TokenizerCache {
    pub(crate) named_cache: RwLock<HashMap<CompactString, Arc<TextAnalyzer>>>,
    pub(crate) hashed_cache: RwLock<HashMap<Vec<u8>, Arc<TextAnalyzer>>>,
}

impl TokenizerCache {
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
                .expect("tokenizer named_cache RwLock poisoned");
            if let Some(analyzer) = idx_cache.get(tokenizer_name) {
                return Ok(analyzer.clone());
            }
        }
        let hash = tokenizer.config_hash(filters);
        {
            let hashed_cache = self
                .hashed_cache
                .read()
                .expect("tokenizer hashed_cache RwLock poisoned");
            if let Some(analyzer) = hashed_cache.get(hash.as_ref()) {
                let mut idx_cache = self
                    .named_cache
                    .write()
                    .expect("tokenizer named_cache RwLock poisoned");
                idx_cache.insert(tokenizer_name.into(), analyzer.clone());
                return Ok(analyzer.clone());
            }
        }
        {
            let analyzer = Arc::new(tokenizer.build(filters)?);
            let mut hashed_cache = self
                .hashed_cache
                .write()
                .expect("tokenizer hashed_cache RwLock poisoned");
            hashed_cache.insert(hash.as_ref().to_vec(), analyzer.clone());
            let mut idx_cache = self
                .named_cache
                .write()
                .expect("tokenizer named_cache RwLock poisoned");
            idx_cache.insert(tokenizer_name.into(), analyzer.clone());
            Ok(analyzer)
        }
    }
}
