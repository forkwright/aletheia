//! Full-text search index with BM25 scoring.
//!
//! Invoked via `~facts:content_fts{id | query: $q, k: $k, score_kind: 'bm25'}`
//! in episteme's recall pipeline.
//!
//! Implementation: inverted index with term frequency tracking and BM25
//! scoring. Supports simple tokenization, English stemming (via
//! rust-stemmers), and stopword filtering.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::RwLock;

use crate::v2::error::{self, Result};
use crate::v2::value::Value;

use super::Index;

// ---------------------------------------------------------------------------
// FTS configuration
// ---------------------------------------------------------------------------

/// Configuration for the full-text search index.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FtsConfig {
    /// Name of the tokenizer ("simple" or "unicode").
    pub tokenizer: String,
    /// Whether to apply stemming.
    pub stem: bool,
    /// Language for stemming.
    pub language: String,
    /// Whether to filter stopwords.
    pub filter_stopwords: bool,
}

impl Default for FtsConfig {
    fn default() -> Self {
        Self {
            tokenizer: "simple".to_owned(),
            stem: true,
            language: "english".to_owned(),
            filter_stopwords: true,
        }
    }
}

// ---------------------------------------------------------------------------
// BM25 parameters
// ---------------------------------------------------------------------------

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

// ---------------------------------------------------------------------------
// FTS index
// ---------------------------------------------------------------------------

/// Full-text search index with BM25 scoring.
pub struct FtsIndex {
    name: String,
    config: FtsConfig,
    /// Inverted index: term → set of (doc_id, term_frequency).
    index: RwLock<InvertedIndex>,
}

struct InvertedIndex {
    /// term → vec of (doc_id, term_frequency)
    postings: HashMap<String, Vec<(Vec<u8>, u32)>>,
    /// doc_id → document length (in tokens)
    doc_lengths: HashMap<Vec<u8>, u32>,
    /// Total documents indexed.
    doc_count: usize,
    /// Sum of all document lengths (for avgdl).
    total_length: u64,
}

impl InvertedIndex {
    fn new() -> Self {
        Self {
            postings: HashMap::new(),
            doc_lengths: HashMap::new(),
            doc_count: 0,
            total_length: 0,
        }
    }
}

impl FtsIndex {
    /// Create a new FTS index.
    #[must_use]
    pub fn new(name: impl Into<String>, config: FtsConfig) -> Self {
        Self {
            name: name.into(),
            config,
            index: RwLock::new(InvertedIndex::new()),
        }
    }

    /// Tokenize text into terms.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let tokens: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();

        let tokens = if self.config.filter_stopwords {
            tokens
                .into_iter()
                .filter(|t| !is_stopword(t))
                .collect()
        } else {
            tokens
        };

        if self.config.stem {
            tokens.into_iter().map(|t| stem_english(&t)).collect()
        } else {
            tokens
        }
    }
}

impl Index for FtsIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn upsert(&self, id: &[u8], value: &Value) -> Result<()> {
        let text = match value {
            Value::Str(s) => s.to_string(),
            _ => {
                return Err(error::IndexSnafu {
                    index_name: self.name.clone(),
                    message: format!("expected string for FTS, got {}", value.type_name()),
                }
                .build())
            }
        };

        let terms = self.tokenize(&text);
        let doc_len = terms.len() as u32;

        let mut idx = self.index.write().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;

        // Remove old entry if exists (upsert).
        if let Some(old_len) = idx.doc_lengths.remove(id) {
            idx.total_length -= u64::from(old_len);
            idx.doc_count -= 1;
            for postings in idx.postings.values_mut() {
                postings.retain(|(doc_id, _)| doc_id != id);
            }
        }

        // Count term frequencies.
        let mut tf_map: HashMap<String, u32> = HashMap::new();
        for term in &terms {
            *tf_map.entry(term.clone()).or_insert(0) += 1;
        }

        // Add to postings.
        for (term, tf) in tf_map {
            idx.postings
                .entry(term)
                .or_default()
                .push((id.to_vec(), tf));
        }

        idx.doc_lengths.insert(id.to_vec(), doc_len);
        idx.doc_count += 1;
        idx.total_length += u64::from(doc_len);

        Ok(())
    }

    fn remove(&self, id: &[u8]) -> Result<()> {
        let mut idx = self.index.write().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;

        if let Some(old_len) = idx.doc_lengths.remove(id) {
            idx.total_length -= u64::from(old_len);
            idx.doc_count -= 1;
            for postings in idx.postings.values_mut() {
                postings.retain(|(doc_id, _)| doc_id != id);
            }
        }
        Ok(())
    }

    fn search(
        &self,
        query: &Value,
        k: usize,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Vec<(Vec<u8>, f64)>> {
        let query_text = match query {
            Value::Str(s) => s.to_string(),
            _ => {
                return Err(error::IndexSnafu {
                    index_name: self.name.clone(),
                    message: format!("expected string query, got {}", query.type_name()),
                }
                .build())
            }
        };

        let query_terms = self.tokenize(&query_text);
        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let idx = self.index.read().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;

        let n = idx.doc_count as f64;
        let avgdl = if idx.doc_count > 0 {
            idx.total_length as f64 / n
        } else {
            1.0
        };

        // Accumulate BM25 scores per document.
        let mut scores: HashMap<Vec<u8>, f64> = HashMap::new();

        for term in &query_terms {
            let Some(postings) = idx.postings.get(term) else {
                continue;
            };

            let df = postings.len() as f64;
            // IDF: log((N - df + 0.5) / (df + 0.5) + 1)
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

            for (doc_id, tf) in postings {
                let tf_f64 = f64::from(*tf);
                let dl = f64::from(idx.doc_lengths.get(doc_id).copied().unwrap_or(1));

                // BM25 term score
                let numerator = tf_f64 * (BM25_K1 + 1.0);
                let denominator = tf_f64 + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl);
                let score = idf * numerator / denominator;

                *scores.entry(doc_id.clone()).or_insert(0.0) += score;
            }
        }

        // Sort by score descending (higher = more relevant).
        // WHY: BM25 scores are relevance scores, not distances.
        // Convert to distance = 1/score for consistency with Index trait.
        let mut results: Vec<(Vec<u8>, f64)> = scores
            .into_iter()
            .map(|(id, score)| {
                let distance = if score > 0.0 { 1.0 / score } else { f64::MAX };
                (id, distance)
            })
            .collect();

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);
        Ok(results)
    }

    fn len(&self) -> usize {
        self.index
            .read()
            .map(|idx| idx.doc_count)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Text processing
// ---------------------------------------------------------------------------

/// Simple English stemmer (Porter-like suffix stripping).
fn stem_english(word: &str) -> String {
    // WHY: rust-stemmers is already a krites dependency. Use it.
    use rust_stemmers::{Algorithm, Stemmer};
    let stemmer = Stemmer::create(Algorithm::English);
    stemmer.stem(word).to_string()
}

/// English stopword check.
fn is_stopword(word: &str) -> bool {
    static STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "as", "at", "be", "but", "by", "for",
        "from", "had", "has", "have", "he", "her", "his", "how", "i",
        "if", "in", "into", "is", "it", "its", "just", "me", "my",
        "no", "not", "of", "on", "or", "our", "out", "own", "say",
        "she", "so", "some", "than", "that", "the", "their", "them",
        "then", "there", "these", "they", "this", "those", "through",
        "to", "too", "up", "us", "very", "was", "we", "were", "what",
        "when", "where", "which", "who", "will", "with", "would", "you",
        "your",
    ];
    STOPWORDS.contains(&word)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn text_val(s: &str) -> Value {
        Value::Str(Arc::from(s))
    }

    #[test]
    fn insert_and_search() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"d1", &text_val("rust programming language")).unwrap();
        idx.upsert(b"d2", &text_val("python scripting language")).unwrap();
        idx.upsert(b"d3", &text_val("rust compiler optimization")).unwrap();

        let results = idx.search(&text_val("rust"), 3, &BTreeMap::new()).unwrap();
        assert_eq!(results.len(), 2); // d1 and d3 contain "rust"
    }

    #[test]
    fn bm25_prefers_shorter_docs() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"short", &text_val("rust")).unwrap();
        idx.upsert(b"long", &text_val("rust is a systems programming language with memory safety guarantees")).unwrap();

        let results = idx.search(&text_val("rust"), 2, &BTreeMap::new()).unwrap();
        // Shorter doc should score higher (lower distance) for "rust"
        assert_eq!(results[0].0, b"short");
    }

    #[test]
    fn upsert_replaces() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"d1", &text_val("old content")).unwrap();
        idx.upsert(b"d1", &text_val("new content")).unwrap();

        assert_eq!(idx.len(), 1);
        let results = idx.search(&text_val("old"), 1, &BTreeMap::new()).unwrap();
        assert!(results.is_empty()); // "old" should not match after replace
    }

    #[test]
    fn remove() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"d1", &text_val("hello world")).unwrap();
        idx.remove(b"d1").unwrap();

        assert_eq!(idx.len(), 0);
        let results = idx.search(&text_val("hello"), 1, &BTreeMap::new()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn stemming_matches() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"d1", &text_val("programming languages")).unwrap();
        // "programs" should match via stemming (program -> program, programming -> program)
        let results = idx.search(&text_val("programs"), 1, &BTreeMap::new()).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn stopwords_filtered() {
        let idx = FtsIndex::new("test", FtsConfig::default());

        idx.upsert(b"d1", &text_val("the quick brown fox")).unwrap();
        // "the" is a stopword — should not match
        let results = idx.search(&text_val("the"), 1, &BTreeMap::new()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn empty_query() {
        let idx = FtsIndex::new("test", FtsConfig::default());
        idx.upsert(b"d1", &text_val("hello")).unwrap();
        let results = idx.search(&text_val(""), 1, &BTreeMap::new()).unwrap();
        assert!(results.is_empty());
    }
}
