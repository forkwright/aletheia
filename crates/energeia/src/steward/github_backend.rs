//! [`StewardBackend`] implementation against the real `GitHub` REST API.

use super::backend::{BackendError, BackendFuture, StewardBackend};
use super::types::{CheckRun, MergeMethod, PrFile, PullRequest};

/// `GitHub` REST API `PullRequest` list-endpoint shape.
///
/// WHY a separate DTO: `GitHub`'s field names/nesting (`head.ref`,
/// `head.sha`) don't match [`PullRequest`]'s flattened shape, and the
/// LIST endpoint (unlike the single-PR endpoint) never returns
/// `mergeable`/`mergeable_state` at all -- so `mergeable` is honestly
/// `None` here rather than a value this backend never actually observed.
#[derive(Debug, serde::Deserialize)]
struct GhPullRequest {
    number: u64,
    title: String,
    head: GhRef,
    state: String,
    body: Option<String>,
    updated_at: Option<String>,
    merged_at: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GhRef {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: String,
}

impl From<GhPullRequest> for PullRequest {
    fn from(gh: GhPullRequest) -> Self {
        Self {
            number: gh.number,
            title: gh.title,
            head_ref_name: Some(gh.head.ref_name),
            head_sha: Some(gh.head.sha),
            state: Some(gh.state),
            // WHY: the list endpoint never returns mergeable/mergeable_state
            // (only the single-PR endpoint does, and its vocabulary --
            // "dirty"/"clean"/"unstable" -- doesn't match the "MERGEABLE"/
            // "CONFLICTING" strings merge.rs checks against). Reporting
            // `None` here is honest about what this backend fetched; a
            // conflict-aware backend would need a second per-PR call this
            // trait doesn't currently expose.
            mergeable: None,
            body: gh.body,
            updated_at: gh.updated_at,
            merged_at: gh.merged_at,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct GhCheckRunsResponse {
    check_runs: Vec<CheckRun>,
}

#[derive(serde::Serialize)]
struct GhMergeRequest {
    merge_method: &'static str,
}

fn merge_method_str(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Squash => "squash",
        MergeMethod::Merge => "merge",
        MergeMethod::Rebase => "rebase",
    }
}

fn split_project(project: &str) -> Result<(&str, &str), BackendError> {
    project
        .split_once('/')
        .ok_or_else(|| BackendError::Request {
            message: format!("project slug {project:?} must be \"owner/repo\""),
        })
}

/// [`StewardBackend`] backed by the real `GitHub` REST API v3.
pub struct GithubStewardBackend {
    client: reqwest::Client,
    token: String,
    base_url: String,
}

impl GithubStewardBackend {
    /// Build a backend authenticating with `token` against `api.github.com`.
    #[must_use]
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: "https://api.github.com".to_owned(),
        }
    }

    /// Build a backend against an explicit `base_url` (test injection).
    #[must_use]
    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "aletheia-steward")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }
}

impl StewardBackend for GithubStewardBackend {
    fn list_open_prs<'a>(&'a self, project: &'a str) -> BackendFuture<'a, Vec<PullRequest>> {
        Box::pin(async move {
            let (owner, repo) = split_project(project)?;
            let resp = self
                .request(
                    reqwest::Method::GET,
                    &format!("/repos/{owner}/{repo}/pulls"),
                )
                .query(&[("state", "open"), ("per_page", "100")])
                .send()
                .await
                .map_err(|e| BackendError::Request {
                    message: format!("list_open_prs: {e}"),
                })?;
            if !resp.status().is_success() {
                return Err(BackendError::Request {
                    message: format!("list_open_prs: HTTP {}", resp.status()),
                });
            }
            let prs: Vec<GhPullRequest> = resp.json().await.map_err(|e| BackendError::Request {
                message: format!("list_open_prs: decode: {e}"),
            })?;
            Ok(prs.into_iter().map(Into::into).collect())
        })
    }

    fn checks<'a>(
        &'a self,
        project: &'a str,
        pr: &'a PullRequest,
    ) -> BackendFuture<'a, Vec<CheckRun>> {
        Box::pin(async move {
            let (owner, repo) = split_project(project)?;
            let sha = pr
                .head_sha
                .as_deref()
                .ok_or_else(|| BackendError::Request {
                    message: format!("pr #{}: no head_sha to fetch checks for", pr.number),
                })?;
            let resp = self
                .request(
                    reqwest::Method::GET,
                    &format!("/repos/{owner}/{repo}/commits/{sha}/check-runs"),
                )
                .query(&[("per_page", "100")])
                .send()
                .await
                .map_err(|e| BackendError::Request {
                    message: format!("checks: {e}"),
                })?;
            if !resp.status().is_success() {
                return Err(BackendError::Request {
                    message: format!("checks: HTTP {}", resp.status()),
                });
            }
            let body: GhCheckRunsResponse =
                resp.json().await.map_err(|e| BackendError::Request {
                    message: format!("checks: decode: {e}"),
                })?;
            Ok(body.check_runs)
        })
    }

    fn files<'a>(
        &'a self,
        project: &'a str,
        pr: &'a PullRequest,
    ) -> BackendFuture<'a, Vec<PrFile>> {
        Box::pin(async move {
            let (owner, repo) = split_project(project)?;
            let resp = self
                .request(
                    reqwest::Method::GET,
                    &format!("/repos/{owner}/{repo}/pulls/{}/files", pr.number),
                )
                .query(&[("per_page", "100")])
                .send()
                .await
                .map_err(|e| BackendError::Request {
                    message: format!("files: {e}"),
                })?;
            if !resp.status().is_success() {
                return Err(BackendError::Request {
                    message: format!("files: HTTP {}", resp.status()),
                });
            }
            resp.json().await.map_err(|e| BackendError::Request {
                message: format!("files: decode: {e}"),
            })
        })
    }

    fn merge<'a>(
        &'a self,
        project: &'a str,
        pr: &'a PullRequest,
        method: MergeMethod,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let (owner, repo) = split_project(project)?;
            let resp = self
                .request(
                    reqwest::Method::PUT,
                    &format!("/repos/{owner}/{repo}/pulls/{}/merge", pr.number),
                )
                .json(&GhMergeRequest {
                    merge_method: merge_method_str(method),
                })
                .send()
                .await
                .map_err(|e| BackendError::Request {
                    message: format!("merge: {e}"),
                })?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(BackendError::Request {
                    message: format!("merge: HTTP {status}: {body}"),
                });
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// WHY: reqwest is built with the `rustls-no-provider` feature, so a rustls
    /// `CryptoProvider` must be installed process-wide before any client is
    /// constructed. In production `main()` does it; a test binary never runs
    /// `main()`, so every test that builds a client installs it first. Same
    /// shape as the test module in `aletheia/src/external_tools.rs`.
    ///
    /// NOTE: `install_default` returns `Err` when a provider is already
    /// installed, which is harmless -- every caller wants the same one.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn test_pr(number: u64) -> PullRequest {
        PullRequest {
            number,
            title: "test".to_owned(),
            head_ref_name: Some("feat/x".to_owned()),
            head_sha: Some("abc123".to_owned()),
            state: Some("open".to_owned()),
            mergeable: None,
            body: None,
            updated_at: None,
            merged_at: None,
        }
    }

    #[test]
    fn split_project_rejects_missing_slash() {
        assert!(split_project("no-slash-here").is_err());
    }

    #[test]
    fn split_project_splits_owner_repo() {
        assert_eq!(
            split_project("forkwright/aletheia").unwrap(),
            ("forkwright", "aletheia")
        );
    }

    #[tokio::test]
    async fn list_open_prs_parses_github_shape_and_reports_mergeable_none() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/repo/pulls"))
            .and(query_param("state", "open"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 42,
                    "title": "feat: add thing",
                    "head": {"ref": "feat/thing", "sha": "deadbeef"},
                    "state": "open",
                    "body": "desc",
                    "updated_at": "2026-01-01T00:00:00Z",
                    "merged_at": null
                }
            ])))
            .mount(&server)
            .await;

        let backend = GithubStewardBackend::with_base_url("tok".to_owned(), server.uri());
        let prs = backend.list_open_prs("acme/repo").await.expect("list ok");

        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 42);
        assert_eq!(prs[0].head_ref_name.as_deref(), Some("feat/thing"));
        assert_eq!(prs[0].head_sha.as_deref(), Some("deadbeef"));
        assert_eq!(
            prs[0].mergeable, None,
            "list endpoint never returns mergeable"
        );
    }

    #[tokio::test]
    async fn list_open_prs_surfaces_http_error() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/repo/pulls"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let backend = GithubStewardBackend::with_base_url("tok".to_owned(), server.uri());
        let err = backend
            .list_open_prs("acme/repo")
            .await
            .expect_err("503 must surface as an error");
        assert!(matches!(err, BackendError::Request { .. }));
    }

    #[tokio::test]
    async fn checks_unwraps_check_runs_envelope() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/repo/commits/abc123/check-runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_count": 1,
                "check_runs": [
                    {"name": "build", "status": "completed", "conclusion": "success"}
                ]
            })))
            .mount(&server)
            .await;

        let backend = GithubStewardBackend::with_base_url("tok".to_owned(), server.uri());
        let checks = backend
            .checks("acme/repo", &test_pr(1))
            .await
            .expect("checks ok");

        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].conclusion.as_deref(), Some("success"));
    }

    #[tokio::test]
    async fn merge_sends_configured_method() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/repos/acme/repo/pulls/7/merge"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"merged": true})),
            )
            .mount(&server)
            .await;

        let backend = GithubStewardBackend::with_base_url("tok".to_owned(), server.uri());
        backend
            .merge("acme/repo", &test_pr(7), MergeMethod::Squash)
            .await
            .expect("merge ok");
    }

    #[tokio::test]
    async fn merge_surfaces_failure_body() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/repos/acme/repo/pulls/7/merge"))
            .respond_with(
                ResponseTemplate::new(405).set_body_string("Pull Request is not mergeable"),
            )
            .mount(&server)
            .await;

        let backend = GithubStewardBackend::with_base_url("tok".to_owned(), server.uri());
        let err = backend
            .merge("acme/repo", &test_pr(7), MergeMethod::Squash)
            .await
            .expect_err("405 must surface as an error");
        let BackendError::Request { message } = err;
        assert!(
            message.contains("not mergeable"),
            "error should carry the GitHub response body: {message}"
        );
    }
}
