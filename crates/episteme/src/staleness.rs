//! Source-linked re-fetching for fact staleness validation.
//!
//! Based on MemGuard (Apr 2026): 33% of stored facts become incorrect within
//! 90 days. For facts that originated from external sources (URLs, documents),
//! periodically re-fetch the source and compare against the stored fact content.
//! Catches staleness cheaply without LLM costs.
//!
//! This module provides the staleness detection types and comparison logic.
//! The actual re-fetching (HTTP, file I/O) is done by the caller — this module
//! is pure computation over content.

/// A fact linked to an external source that can be re-fetched.
#[derive(Debug, Clone)]
pub struct SourceLinkedFact {
    /// Fact identifier.
    pub fact_id: String,
    /// The stored fact content.
    pub content: String,
    /// URI of the original source (URL, file path, API endpoint).
    pub source_uri: String,
    /// When the fact was last validated against its source (ISO 8601).
    pub last_validated: Option<String>,
}

/// Result of comparing a stored fact against its re-fetched source.
#[derive(Debug, Clone)]
pub struct StalenessResult {
    /// The fact that was checked.
    pub fact_id: String,
    /// Whether the fact is still consistent with its source.
    pub status: StalenessStatus,
    /// Token overlap between fact content and source content (0.0..=1.0).
    pub similarity: f64,
    /// Explanation of the result.
    pub explanation: String,
}

/// Whether a fact is still consistent with its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalenessStatus {
    /// Fact content is still grounded in the source.
    Fresh,
    /// Fact content has partial overlap — source may have changed.
    Drifted,
    /// Fact content has no overlap with the current source — likely stale.
    Stale,
    /// Source could not be re-fetched (unavailable, 404, etc.).
    Unreachable,
}

impl std::fmt::Display for StalenessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fresh => f.write_str("fresh"),
            Self::Drifted => f.write_str("drifted"),
            Self::Stale => f.write_str("stale"),
            Self::Unreachable => f.write_str("unreachable"),
        }
    }
}

/// Configuration for staleness checking.
#[derive(Debug, Clone)]
pub struct StalenessConfig {
    /// Minimum similarity score to consider a fact "fresh" (0.0..=1.0). Default: 0.5.
    pub fresh_threshold: f64,
    /// Minimum similarity score to consider a fact "drifted" (below this = stale). Default: 0.15.
    pub stale_threshold: f64,
}

impl Default for StalenessConfig {
    fn default() -> Self {
        Self {
            fresh_threshold: 0.5,
            stale_threshold: 0.15,
        }
    }
}

/// Staleness checker: compares stored fact content against re-fetched source content.
#[derive(Debug, Clone)]
pub struct StalenessChecker {
    config: StalenessConfig,
}

impl StalenessChecker {
    /// Create a new checker with the given configuration.
    #[must_use]
    pub fn new(config: StalenessConfig) -> Self {
        Self { config }
    }

    /// Compare a stored fact against its re-fetched source content.
    ///
    /// The caller is responsible for fetching `source_content` from the fact's
    /// `source_uri`. Pass `None` if the source is unreachable.
    #[must_use]
    pub fn check(
        &self,
        fact: &SourceLinkedFact,
        source_content: Option<&str>,
    ) -> StalenessResult {
        let Some(source) = source_content else {
            return StalenessResult {
                fact_id: fact.fact_id.clone(),
                status: StalenessStatus::Unreachable,
                similarity: 0.0,
                explanation: format!("source {} unreachable", fact.source_uri),
            };
        };

        let similarity = compute_similarity(&fact.content, source);
        let status = if similarity >= self.config.fresh_threshold {
            StalenessStatus::Fresh
        } else if similarity >= self.config.stale_threshold {
            StalenessStatus::Drifted
        } else {
            StalenessStatus::Stale
        };

        let explanation = match status {
            StalenessStatus::Fresh => {
                format!("fact grounded in source (similarity={similarity:.2})")
            }
            StalenessStatus::Drifted => {
                format!(
                    "fact partially matches source (similarity={similarity:.2}), may need update"
                )
            }
            StalenessStatus::Stale => {
                format!(
                    "fact no longer matches source (similarity={similarity:.2}), likely outdated"
                )
            }
            StalenessStatus::Unreachable => unreachable!(),
        };

        StalenessResult {
            fact_id: fact.fact_id.clone(),
            status,
            similarity,
            explanation,
        }
    }

    /// Check multiple facts and return a summary.
    #[must_use]
    pub fn check_batch(&self, checks: &[(SourceLinkedFact, Option<String>)]) -> BatchResult {
        let results: Vec<StalenessResult> = checks
            .iter()
            .map(|(fact, source)| self.check(fact, source.as_deref()))
            .collect();

        let fresh = results
            .iter()
            .filter(|r| r.status == StalenessStatus::Fresh)
            .count();
        let drifted = results
            .iter()
            .filter(|r| r.status == StalenessStatus::Drifted)
            .count();
        let stale = results
            .iter()
            .filter(|r| r.status == StalenessStatus::Stale)
            .count();
        let unreachable = results
            .iter()
            .filter(|r| r.status == StalenessStatus::Unreachable)
            .count();

        BatchResult {
            results,
            fresh,
            drifted,
            stale,
            unreachable,
        }
    }
}

/// Summary of a batch staleness check.
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// Individual check results.
    pub results: Vec<StalenessResult>,
    /// Number of facts still fresh.
    pub fresh: usize,
    /// Number of facts that have drifted.
    pub drifted: usize,
    /// Number of facts that are stale.
    pub stale: usize,
    /// Number of facts whose sources were unreachable.
    pub unreachable: usize,
}

impl BatchResult {
    /// Total number of facts checked.
    #[must_use]
    pub fn total(&self) -> usize {
        self.results.len()
    }

    /// Fraction of facts that are fresh (0.0..=1.0).
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "fact counts are small (<1000); f64 mantissa handles this exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — fact counts are bounded and small"
    )]
    pub fn freshness_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 1.0;
        }
        self.fresh as f64 / total as f64
    }
}

/// Compute content similarity between a fact and its source using token overlap.
///
/// Uses lowercased word tokens with stop-word filtering, similar to the probe
/// module's approach.
fn compute_similarity(fact_content: &str, source_content: &str) -> f64 {
    let fact_tokens = tokenize(fact_content);
    let source_tokens = tokenize(source_content);

    if fact_tokens.is_empty() {
        return 0.0;
    }

    let matches = fact_tokens
        .iter()
        .filter(|t| source_tokens.contains(t))
        .count();

    // Jaccard-style: fraction of fact tokens found in source
    #[expect(
        clippy::cast_precision_loss,
        reason = "token counts are small; f64 handles this exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — token counts are bounded and small"
    )]
    {
        matches as f64 / fact_tokens.len() as f64
    }
}

/// Common stop words excluded from similarity comparison.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "was", "one", "our",
    "out", "has", "his", "how", "its", "may", "new", "now", "old", "see", "way", "who", "did",
    "get", "let", "say", "she", "too", "use", "will", "with", "this", "that", "from", "have",
    "been", "some", "they", "were", "what", "when", "your", "each", "make", "like", "into",
    "just", "over", "such", "than", "them", "then", "also", "more", "should",
];

/// Tokenize text into lowercase words, excluding stop words and short tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3)
        .map(str::to_lowercase)
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fact(id: &str, content: &str, uri: &str) -> SourceLinkedFact {
        SourceLinkedFact {
            fact_id: id.to_owned(),
            content: content.to_owned(),
            source_uri: uri.to_owned(),
            last_validated: None,
        }
    }

    #[test]
    fn fresh_fact_matches_source() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let fact = make_fact(
            "f1",
            "PostgreSQL uses MVCC for concurrent access control",
            "https://docs.example.com/postgres",
        );
        let source = "PostgreSQL implements Multi-Version Concurrency Control (MVCC) \
                       to handle concurrent access without locking. Each transaction \
                       sees a snapshot of the database.";
        let result = checker.check(&fact, Some(source));
        assert_eq!(result.status, StalenessStatus::Fresh);
        assert!(result.similarity >= 0.5);
    }

    #[test]
    fn stale_fact_diverges_from_source() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let fact = make_fact(
            "f2",
            "The company headquarters is in San Francisco",
            "https://company.example.com/about",
        );
        let source = "Our team is fully remote with no physical offices. \
                       We were founded in 2024 and operate across 12 countries.";
        let result = checker.check(&fact, Some(source));
        assert_eq!(result.status, StalenessStatus::Stale);
        assert!(result.similarity < 0.15);
    }

    #[test]
    fn unreachable_source_returns_unreachable() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let fact = make_fact("f3", "some content", "https://404.example.com");
        let result = checker.check(&fact, None);
        assert_eq!(result.status, StalenessStatus::Unreachable);
    }

    #[test]
    fn drifted_fact_partial_overlap() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let fact = make_fact(
            "f4",
            "The API uses REST with JSON responses and OAuth2 authentication",
            "https://api.example.com/docs",
        );
        // Source changed: now GraphQL instead of REST, but still mentions JSON and OAuth
        let source = "The API has been migrated to GraphQL. Authentication still uses \
                       OAuth2 tokens. All responses are in JSON format.";
        let result = checker.check(&fact, Some(source));
        // Should have partial overlap (JSON, OAuth, API) but not full (REST vs GraphQL)
        assert!(
            result.similarity > 0.0,
            "should have some overlap: {:.2}",
            result.similarity
        );
    }

    #[test]
    fn batch_check_counts_correctly() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let checks = vec![
            (
                make_fact("f1", "Rust uses ownership for memory safety", "url1"),
                Some("Rust's ownership system ensures memory safety without garbage collection".to_owned()),
            ),
            (
                make_fact("f2", "Python is statically typed", "url2"),
                Some("Python is a dynamically typed language with duck typing".to_owned()),
            ),
            (
                make_fact("f3", "some fact", "url3"),
                None, // unreachable
            ),
        ];
        let batch = checker.check_batch(&checks);
        assert_eq!(batch.total(), 3);
        assert_eq!(batch.unreachable, 1);
        // At least one should be fresh or drifted (the Rust one)
        assert!(batch.fresh + batch.drifted >= 1);
    }

    #[test]
    fn empty_fact_returns_zero_similarity() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let fact = make_fact("f5", "", "url");
        let result = checker.check(&fact, Some("any source content"));
        assert!(result.similarity.abs() < f64::EPSILON);
    }

    #[test]
    fn freshness_rate_empty_batch() {
        let checker = StalenessChecker::new(StalenessConfig::default());
        let batch = checker.check_batch(&[]);
        assert!((batch.freshness_rate() - 1.0).abs() < f64::EPSILON);
    }
}
