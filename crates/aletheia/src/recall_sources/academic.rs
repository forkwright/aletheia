//! Semantic Scholar recall source.
//!
//! WHY: Wraps the Semantic Scholar Academic Graph API so agents can query
//! academic literature as part of the recall pipeline, not just via ad-hoc
//! MCP tools in CC sessions.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use koina::http::TokioHostResolver;
use organon::builtins::http_client::{SafeRequest, send_with_safe_redirects};
use organon::sandbox::EgressGate;
use snafu::ResultExt;
use tracing::debug;

use super::error::{
    EgressRefusedSnafu, HttpRequestSnafu, ParseResponseSnafu, RecallSourceError,
    SourceUnavailableSnafu,
};
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
    /// SECURITY(#6921): the egress policy applied to every hop of the query.
    egress: EgressGate,
}

impl AcademicSource {
    /// Build the source.
    ///
    /// SECURITY(#6921): `client` must have redirects DISABLED.
    /// `send_with_safe_redirects` drives the chain itself so each hop passes the
    /// egress checkpoint; a client that follows redirects on its own would return an
    /// already-redirected response and skip that revalidation entirely -- which is how
    /// this source forwarded `x-api-key` to whatever host a response named.
    pub(crate) fn new(
        client: Arc<reqwest::Client>,
        api_key: Option<String>,
        egress: EgressGate,
    ) -> Self {
        Self {
            client,
            api_key,
            egress,
        }
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

            // WHY not unwrap: `API_BASE` is a const and this cannot fail in practice,
            // but the workspace denies unwrap/expect and a truthful error costs one
            // line. A malformed endpoint is a build-time defect, not an egress refusal.
            let mut url: reqwest::Url = endpoint.parse().map_err(|_e| {
                SourceUnavailableSnafu {
                    message: format!("search endpoint is not a valid URL: {endpoint}"),
                }
                .build()
            })?;
            url.query_pairs_mut()
                .append_pair("query", query)
                .append_pair("fields", PAPER_FIELDS)
                .append_pair("limit", &clamped_limit.to_string());

            let mut headers = HashMap::new();
            if let Some(ref key) = self.api_key {
                headers.insert("x-api-key".to_owned(), key.clone());
            }

            // SECURITY(#6921, #6910): every hop -- the first request and any redirect
            // the endpoint names -- goes through the same egress checkpoint and
            // internal-address revalidation `http_request` uses. Sending on a bare
            // client meant a redirect carried `x-api-key` to an attacker-chosen host:
            // reqwest strips only Authorization, Cookie, Proxy-Authorization and
            // WWW-Authenticate across a cross-host hop, never a custom header.
            let response = send_with_safe_redirects(
                &self.client,
                SafeRequest {
                    method: reqwest::Method::GET,
                    url: url.as_str(),
                    headers: &headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(30),
                },
                &TokioHostResolver,
                &self.egress,
            )
            .await
            .map_err(|message| {
                EgressRefusedSnafu {
                    endpoint: &endpoint,
                    message,
                }
                .build()
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
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_precision_loss,
                        reason = "clamped_limit ≤ 100: well within f64 mantissa"
                    )]
                    let denominator = (clamped_limit.max(1)) as f64;
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_precision_loss,
                        reason = "rank index ≤ 100: well within f64 mantissa"
                    )]
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

    fn source_type(&self) -> &'static str {
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

    if let Some(ref abs) = paper.r#abstract
        && !abs.is_empty()
    {
        parts.push(abs.clone());
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
#[expect(
    clippy::struct_field_names,
    reason = "field name matches Semantic Scholar API JSON key 'paperId'"
)]
struct Paper {
    paper_id: String,
    title: String,
    r#abstract: Option<String>,
    year: Option<u32>,
    citation_count: Option<u64>,
    url: Option<String>,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test data has known structure")]
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

    /// SECURITY(#6921): the query must pass the egress checkpoint, not a bare client.
    ///
    /// WHY this shape: the checkpoint refuses a `Deny` policy BEFORE resolving DNS, so
    /// a routed call fails as `EgressRefused` without touching the network. A call that
    /// went back to `client.send()` would ignore the gate entirely and surface as
    /// `HttpRequest` -- reaching Semantic Scholar, or failing to. Either way a
    /// different variant, so this assertion cannot pass on the unrouted code.
    #[tokio::test]
    async fn query_is_refused_by_a_deny_egress_policy() {
        let source = AcademicSource::new(
            Arc::new(
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
            ),
            Some("secret-key".to_owned()),
            EgressGate::new(organon::sandbox::EgressPolicy::Deny, &[]),
        );

        let error = source.query("transformers", 5).await.unwrap_err();

        assert!(
            matches!(error, RecallSourceError::EgressRefused { .. }),
            "a denied egress policy must refuse the query at the checkpoint, got: {error:?}"
        );
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
        assert_eq!(resp.data[0].title, "Test Paper");
        assert_eq!(resp.data[0].year, Some(2024));
    }
}
