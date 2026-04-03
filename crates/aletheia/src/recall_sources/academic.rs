//! Semantic Scholar recall source.
//!
//! WHY: Wraps the Semantic Scholar Academic Graph API so agents can query
//! academic literature as part of the recall pipeline, not just via ad-hoc
//! MCP tools in CC sessions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use snafu::ResultExt;
use tracing::debug;

use super::error::{HttpRequestSnafu, ParseResponseSnafu, RecallSourceError};
use super::{RecallSource, SourceResult};

const API_BASE: &str = "https://api.semanticscholar.org/graph/v1";

/// Fields requested FROM the Semantic Scholar paper search endpoint.
const PAPER_FIELDS: &str = "paperId,title,abstract,year,citationCount,url";

/// Recall source backed by the Semantic Scholar Academic Graph API.
///
/// Queries the paper search endpoint and returns results formatted as
/// recall-compatible content strings. An optional API key raises the
/// per-second rate limit.
pub(crate) struct AcademicSource {
    client: Arc<reqwest::Client>,
    api_key: Option<String>,
}

impl AcademicSource {
    pub(crate) fn new(client: Arc<reqwest::Client>, api_key: Option<String>) -> Self {
        Self { client, api_key }
    }
}

impl RecallSource for AcademicSource {
    fn query<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceResult>, RecallSourceError>> + Send + 'a>>
    {
        Box::pin(async move {
            let endpoint = format!("{API_BASE}/paper/search");
            let clamped_limit = limit.min(100);

            let mut req = self
                .client
                .get(&endpoint)
                .query(&[("query", query), ("fields", PAPER_FIELDS)])
                .query(&[("limit", clamped_limit)]);

            if let Some(ref key) = self.api_key {
                req = req.header("x-api-key", key);
            }

            let response = req.send().await.context(HttpRequestSnafu {
                endpoint: &endpoint,
            })?;

            let body = response.text().await.context(HttpRequestSnafu {
                endpoint: &endpoint,
            })?;

            let parsed: SearchResponse =
                serde_json::from_str(&body).context(ParseResponseSnafu {
                    endpoint: &endpoint,
                })?;

            debug!(
                total = parsed.total,
                returned = parsed.data.len(),
                "semantic scholar search complete"
            );

            let results = parsed
                .data
                .into_iter()
                .enumerate()
                .map(|(rank, paper)| {
                    let content = format_paper(&paper);
                    // NOTE: Position-based relevance: rank 0 = 1.0, declining linearly.
                    // Semantic Scholar returns results in relevance order.
                    let denominator = (clamped_limit.max(1)) as f64;
                    #[expect(clippy::cast_precision_loss, reason = "rank index is small enough that f64 precision is sufficient")]
                    let relevance = 1.0 - (rank as f64 / denominator);
                    SourceResult {
                        content,
                        relevance,
                        source_id: paper.paper_id,
                    }
                })
                .collect();

            Ok(results)
        })
    }

    fn source_type(&self) -> &str {
        "academic"
    }

    fn available(&self) -> bool {
        true
    }
}

fn format_paper(paper: &Paper) -> String {
    let mut parts = Vec::with_capacity(4);

    if let Some(year) = paper.year {
        parts.push(format!("{} ({})", paper.title, year));
    } else {
        parts.push(paper.title.clone());
    }

    if let Some(ref abs) = paper.r#abstract {
        if !abs.is_empty() {
            parts.push(abs.clone());
        }
    }

    if let Some(citations) = paper.citation_count {
        parts.push(format!("Citations: {citations}"));
    }

    if let Some(ref url) = paper.url {
        parts.push(format!("URL: {url}"));
    }

    parts.join("\n")
}

// -- Semantic Scholar API response types ------------------------------------

#[derive(Debug, serde::Deserialize)]
struct SearchResponse {
    #[serde(default)]
    total: u64,
    data: Vec<Paper>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paper {
    paper_id: String,
    title: String,
    r#abstract: Option<String>,
    year: Option<u32>,
    citation_count: Option<u64>,
    url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_paper_full() {
        let paper = Paper {
            paper_id: "abc123".to_owned(),
            title: "Attention Is All You Need".to_owned(),
            r#abstract: Some("We propose a new architecture.".to_owned()),
            year: Some(2017),
            citation_count: Some(100_000),
            url: Some("https://arxiv.org/abs/1706.03762".to_owned()),
        };
        let formatted = format_paper(&paper);
        assert!(formatted.contains("Attention Is All You Need (2017)"));
        assert!(formatted.contains("We propose a new architecture."));
        assert!(formatted.contains("Citations: 100000"));
        assert!(formatted.contains("https://arxiv.org/abs/1706.03762"));
    }

    #[test]
    fn format_paper_minimal() {
        let paper = Paper {
            paper_id: "xyz".to_owned(),
            title: "Some Paper".to_owned(),
            r#abstract: None,
            year: None,
            citation_count: None,
            url: None,
        };
        let formatted = format_paper(&paper);
        assert_eq!(formatted, "Some Paper");
    }

    #[test]
    fn parse_search_response() {
        let json = r#"{
            "total": 1,
            "OFFSET": 0,
            "data": [{
                "paperId": "p1",
                "title": "Test Paper",
                "abstract": "An abstract.",
                "year": 2024,
                "citationCount": 5,
                "url": "https://example.com/p1"
            }]
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total, 1);
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data.get(0).copied().unwrap_or_default().title, "Test Paper");
        assert_eq!(resp.data.get(0).copied().unwrap_or_default().year, Some(2024));
    }
}
