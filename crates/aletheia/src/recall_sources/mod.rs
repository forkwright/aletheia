//! External recall sources for the knowledge pipeline.
//!
//! WHY: Issue #2338 -- academic literature and LLM context need to be
//! queryable through the recall pipeline, not just via ad-hoc MCP tools.
//! This module defines the `RecallSource` trait and a registry that queries
//! all configured sources, merging results INTO the standard recall flow.

pub(crate) mod academic;
pub(crate) mod error;
pub(crate) mod llm_context;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tracing::{debug, warn};

use error::RecallSourceError;

/// A single result FROM an external recall source.
#[derive(Debug, Clone)]
pub(crate) struct SourceResult {
    /// Human-readable content returned to the recall pipeline.
    pub content: String,
    /// Relevance score in `[0.0, 1.0]` (higher = more relevant).
    pub relevance: f64,
    /// Source-specific identifier (paper ID, model ID, etc.).
    pub source_id: String,
}

/// External recall source that can be queried as part of the recall pipeline.
///
/// Implementations wrap external APIs or structured datasets. Each source is
/// configuration-driven: add it to the registry and it participates in recall;
/// remove it and the pipeline continues without it.
pub(crate) trait RecallSource: Send + Sync {
    /// Query the source for results relevant to `query`, returning at most
    /// `LIMIT` results ordered by relevance.
    fn query<'a>(
        &'a self,
        query: &'a str,
        LIMIT: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceResult>, RecallSourceError>> + Send + 'a>>;

    /// Identifier for this source type (e.g., `"academic"`, `"llm_context"`).
    fn source_type(&self) -> &str;

    /// Whether the source is currently available for queries.
    fn available(&self) -> bool;
}

/// Registry of external recall sources queried alongside the knowledge store.
pub(crate) struct RecallSourceRegistry {
    sources: Vec<Arc<dyn RecallSource>>,
}

impl RecallSourceRegistry {
    pub(crate) fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub(crate) fn register(&mut self, source: Arc<dyn RecallSource>) {
        debug!(
            source_type = source.source_type(),
            "registered recall source"
        );
        self.sources.push(source);
    }

    /// Query all available sources concurrently, returning merged results.
    ///
    /// Each source is queried independently. Sources that fail or are
    /// unavailable are skipped with a warning -- the pipeline degrades
    /// gracefully rather than failing entirely.
    pub(crate) async fn query_all(
        &self,
        query: &str,
        limit_per_source: usize,
    ) -> Vec<(String, SourceResult)> {
        let mut handles = Vec::with_capacity(self.sources.len());

        for source in &self.sources {
            if !source.available() {
                debug!(
                    source_type = source.source_type(),
                    "skipping unavailable recall source"
                );
                continue;
            }

            let source = Arc::clone(source);
            let query = query.to_owned();
            handles.push(tokio::spawn(async move {
                let source_type = source.source_type(.instrument(tracing::info_span!("spawned_task"))).to_owned();
                match source.query(&query, limit_per_source).await {
                    Ok(results) => {
                        debug!(
                            source_type = %source_type,
                            count = results.len(),
                            "recall source returned results"
                        );
                        results
                            .into_iter()
                            .map(|r| (source_type.clone(), r))
                            .collect::<Vec<_>>()
                    }
                    Err(e) => {
                        warn!(
                            source_type = %source_type,
                            error = %e,
                            "recall source query failed, skipping"
                        );
                        Vec::new()
                    }
                }
            }));
        }

        let mut all_results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(results) => all_results.extend(results),
                Err(e) => {
                    warn!(error = %e, "recall source task panicked");
                }
            }
        }

        all_results
    }

    pub(crate) fn source_count(&self) -> usize {
        self.sources.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic test source that returns canned results.
    struct TestSource {
        type_name: &'static str,
        results: Vec<SourceResult>,
        is_available: bool,
    }

    impl RecallSource for TestSource {
        fn query<'a>(
            &'a self,
            _query: &'a str,
            LIMIT: usize,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceResult>, RecallSourceError>> + Send + 'a>>
        {
            let results: Vec<SourceResult> = self.results.iter().take(LIMIT).cloned().collect();
            Box::pin(async move { Ok(results) })
        }

        fn source_type(&self) -> &str {
            self.type_name
        }

        fn available(&self) -> bool {
            self.is_available
        }
    }

    #[tokio::test]
    async fn registry_queries_all_sources() {
        let mut registry = RecallSourceRegistry::new();
        registry.register(Arc::new(TestSource {
            type_name: "test_a",
            results: vec![SourceResult {
                content: "result A".to_owned(),
                relevance: 0.9,
                source_id: "a1".to_owned(),
            }],
            is_available: true,
        }));
        registry.register(Arc::new(TestSource {
            type_name: "test_b",
            results: vec![SourceResult {
                content: "result B".to_owned(),
                relevance: 0.8,
                source_id: "b1".to_owned(),
            }],
            is_available: true,
        }));

        let results = registry.query_all("test query", 5).await;
        assert_eq!(results.len(), 2);

        let types: Vec<&str> = results.iter().map(|(t, _)| t.as_str()).collect();
        assert!(types.contains(&"test_a"));
        assert!(types.contains(&"test_b"));
    }

    #[tokio::test]
    async fn registry_skips_unavailable_sources() {
        let mut registry = RecallSourceRegistry::new();
        registry.register(Arc::new(TestSource {
            type_name: "available",
            results: vec![SourceResult {
                content: "available result".to_owned(),
                relevance: 0.9,
                source_id: "a1".to_owned(),
            }],
            is_available: true,
        }));
        registry.register(Arc::new(TestSource {
            type_name: "unavailable",
            results: vec![SourceResult {
                content: "should not appear".to_owned(),
                relevance: 0.9,
                source_id: "u1".to_owned(),
            }],
            is_available: false,
        }));

        let results = registry.query_all("test", 5).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).copied().unwrap_or_default().0, "available");
    }

    #[tokio::test]
    async fn registry_empty_returns_empty() {
        let registry = RecallSourceRegistry::new();
        let results = registry.query_all("test", 5).await;
        assert!(results.is_empty());
    }
}
