//! Backend subsystem status contract: `GET /api/v1/system/status` (#5313, #5315).
//!
//! Distinct from the flat `/api/v1/system/health` (`skene::api::types::HealthResponse`):
//! this is pylon's richer per-subsystem contract, with an explicit owner per
//! record and `"unknown"` reporting instead of a default-to-healthy check
//! array. No frontend consumed this endpoint before #5315.

use reqwest::StatusCode;

use crate::api::client::{AuthenticatedClientError, authenticated_client};
use crate::state::backend_health::BackendHealthState;
use crate::state::connection::ConnectionConfig;

/// One subsystem's status record. Mirrors pylon's `SubsystemStatus` DTO
/// (`crates/pylon/src/handlers/health_dto.rs`); only the fields this
/// frontend renders are modeled.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct SubsystemStatus {
    /// Stable machine identifier (e.g. `"session_store"`).
    #[serde(default)]
    pub(crate) id: String,
    /// Human-readable name for the control-plane UI.
    #[serde(default)]
    pub(crate) name: String,
    /// `"healthy"`, `"degraded"`, `"failed"`, or `"unknown"`.
    #[serde(default)]
    pub(crate) status: String,
}

/// Response body for `GET /api/v1/system/status`.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct SystemStatusResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"failed"`.
    #[serde(default)]
    pub(crate) status: String,
    /// One record per tracked subsystem.
    #[serde(default)]
    pub(crate) subsystems: Vec<SubsystemStatus>,
}

impl SystemStatusResponse {
    /// Names of subsystems not reporting `"healthy"`.
    #[must_use]
    pub(crate) fn failing_names(&self) -> Vec<String> {
        self.subsystems
            .iter()
            .filter(|subsystem| subsystem.status != "healthy")
            .map(|subsystem| {
                if subsystem.name.is_empty() {
                    subsystem.id.clone()
                } else {
                    subsystem.name.clone()
                }
            })
            .collect()
    }

    /// Reduce into the frontend's [`BackendHealthState`].
    #[must_use]
    pub(crate) fn to_backend_health(&self) -> BackendHealthState {
        let failing = self.failing_names();
        match self.status.as_str() {
            "healthy" => BackendHealthState::Healthy,
            "degraded" => BackendHealthState::Degraded { failing },
            // WHY: "failed" and any unrecognized aggregate value both mean
            // the endpoint is not confidently reporting "healthy" or
            // "degraded" -- treat unrecognized values as the worse case
            // rather than defaulting to healthy (the same anti-pattern
            // #5313's own subsystem records exist to avoid).
            _ => BackendHealthState::Failed { failing },
        }
    }
}

/// Failure classes for a system-status fetch (#5315).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemStatusFetchError {
    /// Local HTTP client could not be built (e.g. malformed token).
    Client {
        /// Human-readable cause.
        message: String,
        /// Whether the cause was specifically a malformed bearer token,
        /// distinct from any other client-construction failure.
        invalid_token: bool,
    },
    /// Transport-level failure: DNS, connect, TLS, timeout.
    Unreachable(String),
    /// The connected bearer token lacks the role required to read backend
    /// health (401/403 from `/api/v1/system/status`).
    ///
    /// INVARIANT: kept distinct from `Unreachable` at the type level.
    /// Server-side, `require_role(&claims, Role::Operator)` in
    /// `crates/pylon/src/handlers/health.rs::system_status` returns 403
    /// Forbidden for an authenticated-but-under-privileged token and 401
    /// Unauthorized for a missing/invalid one; both mean "this token cannot
    /// see health", never "the server is unreachable" (operator decision,
    /// #5315).
    Unauthorized,
    /// A 2xx/503 response body did not match the expected contract.
    Malformed(String),
    /// Any other non-success status.
    Server(u16),
}

impl std::fmt::Display for SystemStatusFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client { message, .. } => write!(f, "client build failed: {message}"),
            Self::Unreachable(message) => write!(f, "unreachable: {message}"),
            Self::Unauthorized => write!(f, "unauthorized: token lacks the Operator role"),
            Self::Malformed(message) => write!(f, "malformed response: {message}"),
            Self::Server(status) => write!(f, "server returned {status}"),
        }
    }
}

impl std::error::Error for SystemStatusFetchError {}

impl SystemStatusFetchError {
    /// Reduce into the frontend's [`BackendHealthState`].
    ///
    /// Every non-auth failure class collapses to `Unreachable` for display:
    /// the operator-visible vocabulary is the issue's five states, not this
    /// fetch layer's internal failure taxonomy.
    #[must_use]
    pub(crate) fn to_backend_health(&self) -> BackendHealthState {
        match self {
            Self::Unauthorized => BackendHealthState::Unauthorized,
            Self::Client { .. } | Self::Unreachable(_) | Self::Malformed(_) | Self::Server(_) => {
                BackendHealthState::Unreachable
            }
        }
    }

    /// Whether the local client could not be built specifically because the
    /// bearer token was malformed (distinct from any other local failure).
    #[must_use]
    pub(crate) fn is_invalid_token(&self) -> bool {
        matches!(self, Self::Client { invalid_token, .. } if *invalid_token)
    }
}

/// Fetch `GET /api/v1/system/status` for the configured server.
///
/// # Errors
///
/// See [`SystemStatusFetchError`] for the failure taxonomy; 401/403 map to
/// [`SystemStatusFetchError::Unauthorized`] specifically so callers can
/// render the operator-mandated distinct unauthorized state (#5315).
pub(crate) async fn fetch_system_status(
    config: &ConnectionConfig,
) -> Result<SystemStatusResponse, SystemStatusFetchError> {
    let client = authenticated_client(config).map_err(|err: AuthenticatedClientError| {
        SystemStatusFetchError::Client {
            invalid_token: err.is_invalid_token(),
            message: err.to_string(),
        }
    })?;
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/system/status");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|err| SystemStatusFetchError::Unreachable(err.to_string()))?;
    let status = resp.status();

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(SystemStatusFetchError::Unauthorized);
    }
    if !(status.is_success() || status == StatusCode::SERVICE_UNAVAILABLE) {
        return Err(SystemStatusFetchError::Server(status.as_u16()));
    }

    let body = resp
        .text()
        .await
        .map_err(|err| SystemStatusFetchError::Unreachable(err.to_string()))?;
    serde_json::from_str(&body).map_err(|err| SystemStatusFetchError::Malformed(err.to_string()))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use std::error::Error;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn status_body(status: &str) -> String {
        serde_json::json!({
            "status": status,
            "generated_at": "2026-01-01T00:00:00Z",
            "subsystems": [
                {"id": "session_store", "name": "Session Store", "status": "healthy"},
                {"id": "embeddings", "name": "Embedding Provider", "status": status},
            ],
        })
        .to_string()
    }

    async fn spawn_status_server(
        http_status: u16,
        body: String,
    ) -> std::io::Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf).await;
            let reason = match http_status {
                200 => "OK",
                401 => "Unauthorized",
                403 => "Forbidden",
                503 => "Service Unavailable",
                _ => "Error",
            };
            let response = format!(
                "HTTP/1.1 {http_status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            Ok(())
        });
        Ok((format!("http://{addr}"), handle))
    }

    #[test]
    fn failing_names_prefers_name_over_id() {
        let response = SystemStatusResponse {
            status: "degraded".to_string(),
            subsystems: vec![
                SubsystemStatus {
                    id: "session_store".to_string(),
                    name: "Session Store".to_string(),
                    status: "healthy".to_string(),
                },
                SubsystemStatus {
                    id: "embeddings".to_string(),
                    name: "Embedding Provider".to_string(),
                    status: "degraded".to_string(),
                },
                SubsystemStatus {
                    id: "unnamed_thing".to_string(),
                    name: String::new(),
                    status: "failed".to_string(),
                },
            ],
        };
        assert_eq!(
            response.failing_names(),
            vec![
                "Embedding Provider".to_string(),
                "unnamed_thing".to_string()
            ]
        );
    }

    #[test]
    fn to_backend_health_maps_aggregate_status() {
        let healthy = SystemStatusResponse {
            status: "healthy".to_string(),
            subsystems: vec![],
        };
        assert_eq!(healthy.to_backend_health(), BackendHealthState::Healthy);

        let degraded = SystemStatusResponse {
            status: "degraded".to_string(),
            subsystems: vec![],
        };
        assert!(matches!(
            degraded.to_backend_health(),
            BackendHealthState::Degraded { .. }
        ));

        let unrecognized = SystemStatusResponse {
            status: "wat".to_string(),
            subsystems: vec![],
        };
        assert!(
            matches!(
                unrecognized.to_backend_health(),
                BackendHealthState::Failed { .. }
            ),
            "unrecognized aggregate status must not default to healthy"
        );
    }

    #[test]
    fn fetch_error_to_backend_health_keeps_unauthorized_distinct_from_unreachable() {
        assert_eq!(
            SystemStatusFetchError::Unauthorized.to_backend_health(),
            BackendHealthState::Unauthorized
        );
        assert_eq!(
            SystemStatusFetchError::Server(500).to_backend_health(),
            BackendHealthState::Unreachable
        );
        assert_eq!(
            SystemStatusFetchError::Unreachable("boom".to_string()).to_backend_health(),
            BackendHealthState::Unreachable
        );
        assert_ne!(
            SystemStatusFetchError::Unauthorized.to_backend_health(),
            SystemStatusFetchError::Server(500).to_backend_health(),
        );
    }

    #[test]
    fn is_invalid_token_only_true_for_flagged_client_error() {
        assert!(
            SystemStatusFetchError::Client {
                message: "bad".to_string(),
                invalid_token: true,
            }
            .is_invalid_token()
        );
        assert!(
            !SystemStatusFetchError::Client {
                message: "bad".to_string(),
                invalid_token: false,
            }
            .is_invalid_token()
        );
        assert!(!SystemStatusFetchError::Unreachable("x".to_string()).is_invalid_token());
    }

    #[tokio::test]
    async fn fetch_system_status_parses_200_healthy() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_status_server(200, status_body("healthy")).await?;
        let config = ConnectionConfig {
            server_url,
            ..ConnectionConfig::default()
        };

        let response = fetch_system_status(&config).await?;
        assert_eq!(response.status, "healthy");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_system_status_accepts_503_failed() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_status_server(503, status_body("failed")).await?;
        let config = ConnectionConfig {
            server_url,
            ..ConnectionConfig::default()
        };

        let response = fetch_system_status(&config).await?;
        assert_eq!(response.status, "failed");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_system_status_maps_401_to_unauthorized() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_status_server(401, "{}".to_string()).await?;
        let config = ConnectionConfig {
            server_url,
            ..ConnectionConfig::default()
        };

        let result = fetch_system_status(&config).await;
        // WHY matches! rather than assert_eq!: SystemStatusResponse has no
        // PartialEq, and deriving one solely to compare an error arm would put
        // a trait on the wire type for a test's convenience.
        assert!(matches!(result, Err(SystemStatusFetchError::Unauthorized)));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_system_status_maps_403_to_unauthorized() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_status_server(403, "{}".to_string()).await?;
        let config = ConnectionConfig {
            server_url,
            ..ConnectionConfig::default()
        };

        let result = fetch_system_status(&config).await;
        // WHY matches! rather than assert_eq!: SystemStatusResponse has no
        // PartialEq, and deriving one solely to compare an error arm would put
        // a trait on the wire type for a test's convenience.
        assert!(matches!(result, Err(SystemStatusFetchError::Unauthorized)));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_system_status_reports_unreachable_on_closed_port() {
        install_crypto();
        let config = ConnectionConfig {
            server_url: "http://127.0.0.1:1".to_string(),
            ..ConnectionConfig::default()
        };

        let result = fetch_system_status(&config).await;
        assert!(matches!(
            result,
            Err(SystemStatusFetchError::Unreachable(_))
        ));
    }

    #[tokio::test]
    async fn fetch_system_status_reports_malformed_on_bad_json() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_status_server(200, "not-json".to_string()).await?;
        let config = ConnectionConfig {
            server_url,
            ..ConnectionConfig::default()
        };

        let result = fetch_system_status(&config).await;
        assert!(matches!(result, Err(SystemStatusFetchError::Malformed(_))));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_system_status_invalid_token_never_reaches_server() -> Result<(), Box<dyn Error>>
    {
        install_crypto();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let server_url = format!("http://{}", listener.local_addr()?);
        let config = ConnectionConfig {
            server_url,
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };

        let result = fetch_system_status(&config).await;
        assert!(matches!(
            result,
            Err(SystemStatusFetchError::Client {
                invalid_token: true,
                ..
            })
        ));
        let accepted =
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept()).await;
        assert!(accepted.is_err(), "invalid token must not reach the server");
        Ok(())
    }
}
