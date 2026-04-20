//! Thin HTTP client against a Matrix homeserver.
//!
//! Phase 2 scope: only `probe()` is live — it hits `/_matrix/client/versions`
//! to confirm the homeserver responds. `matrix-sdk-base` usage for login/sync
//! arrives in Phase 3; the struct here holds the base URL and a shared
//! `reqwest::Client` so Phase 3 can extend it without re-plumbing config.

use std::time::Duration;

use reqwest::Client as HttpClient;
use snafu::ResultExt;
use tracing::{debug, instrument};

use super::error::{HttpSnafu, InvalidUrlSnafu, Result};

/// Default probe timeout. Small enough that a wedged homeserver does not
/// stall health-check sweeps; large enough to tolerate a cold container.
pub(crate) const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Response shape of `GET /_matrix/client/versions`. We only need to
/// confirm a 200 — field contents are not consumed in Phase 2.
#[derive(Debug, Clone)]
pub struct VersionsReport {
    /// HTTP status code from the versions endpoint.
    pub status: u16,
    /// Round-trip latency in milliseconds, measured locally.
    pub latency_ms: u64,
}

/// Matrix homeserver HTTP client.
///
/// Cheap to clone (`reqwest::Client` is internally reference-counted).
#[derive(Debug, Clone)]
pub struct MatrixClient {
    /// Base URL, trailing slash trimmed. All endpoint paths are joined with `/`.
    homeserver: String,
    http: HttpClient,
}

impl MatrixClient {
    /// Build a client for a homeserver at `homeserver_url` with the default probe timeout.
    pub fn new(homeserver_url: &str) -> Result<Self> {
        Self::with_timeout(homeserver_url, DEFAULT_PROBE_TIMEOUT)
    }

    /// Build a client with a custom request timeout.
    pub fn with_timeout(homeserver_url: &str, timeout: Duration) -> Result<Self> {
        let trimmed = homeserver_url.trim_end_matches('/').to_owned();
        // WHY: scheme validation routed through `koina::http` so the
        // plaintext HTTP literal lives in exactly one audited place
        // (see `SECURITY/insecure-transport`).
        if !koina::http::has_http_or_https_scheme(&trimmed) {
            return Err(InvalidUrlSnafu {
                url: homeserver_url.to_owned(),
                message: "expected http:// or https:// prefix".to_owned(),
            }
            .build());
        }

        let http = HttpClient::builder()
            .timeout(timeout)
            .build()
            .context(HttpSnafu)?;

        Ok(Self {
            homeserver: trimmed,
            http,
        })
    }

    /// Base URL (trailing slash trimmed) this client talks to.
    #[must_use]
    pub fn homeserver(&self) -> &str {
        &self.homeserver
    }

    /// Issue `GET {homeserver}/_matrix/client/versions`.
    ///
    /// Returns the status code and measured latency even on non-2xx responses;
    /// only HTTP transport failures surface as `Err`.
    #[instrument(skip(self))]
    pub async fn versions(&self) -> Result<VersionsReport> {
        let start = std::time::Instant::now();
        let url = format!("{}/_matrix/client/versions", self.homeserver);
        debug!(url = %url, "matrix probe");
        let resp = self.http.get(&url).send().await.context(HttpSnafu)?;
        let status = resp.status().as_u16();
        let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(VersionsReport { status, latency_ms })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use organon::testing::install_crypto_provider;

    use super::*;

    #[test]
    fn new_rejects_missing_scheme() {
        install_crypto_provider();
        let err = MatrixClient::new("menos.lan:6167").expect_err("should reject");
        assert!(format!("{err}").contains("http://"));
    }

    #[test]
    fn new_trims_trailing_slash() {
        install_crypto_provider();
        let c = MatrixClient::new("http://127.0.0.1:6167/").expect("ok");
        assert_eq!(c.homeserver(), "http://127.0.0.1:6167");
    }

    #[tokio::test]
    async fn versions_200() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/versions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"versions": ["v1.11"]})),
            )
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri()).expect("client");
        let report = client.versions().await.expect("versions");
        assert_eq!(report.status, 200);
    }

    #[tokio::test]
    async fn versions_502_returns_status_not_error() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/versions"))
            .respond_with(wiremock::ResponseTemplate::new(502))
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri()).expect("client");
        let report = client.versions().await.expect("versions");
        assert_eq!(report.status, 502);
    }
}
