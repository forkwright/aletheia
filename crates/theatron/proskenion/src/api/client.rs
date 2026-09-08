//! Shared authenticated HTTP client for view-layer API requests.

use std::time::Duration;

use reqwest::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use snafu::{ResultExt, Snafu};

use koina::http::{CSRF_HEADER_NAME, DEFAULT_CSRF_HEADER_VALUE};

use crate::state::commands::ServerCommandDescriptor;
use crate::state::connection::ConnectionConfig;

use skene::api::error::ApiError;
use skene::api::types::Agent;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Outcome of a workspace file save against `PUT /api/v1/workspace/files/content`.
///
/// WHY: the viewer renders distinct UX per failure class -- a 413 is "split
/// the note", a 409 is "reload before saving", a transport error is
/// retryable. Mapping the wire status to a typed result keeps that branching
/// declarative at the call site instead of re-deriving it from raw codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveOutcome {
    /// Write succeeded.
    Saved,
    /// File exceeds the server's size cap (HTTP 413).
    TooLarge,
    /// File changed on disk since it was loaded (HTTP 409).
    Conflict,
    /// Any other failure, carrying a human-readable description.
    Failed(String),
}

/// Errors from constructing shared authenticated HTTP clients.
#[derive(Debug, Snafu)]
pub(crate) enum AuthenticatedClientError {
    /// Auth token cannot be encoded as an HTTP header value.
    #[snafu(display(
        "invalid auth token: contains characters that cannot be sent in an HTTP Authorization header. Update or clear the token in Connect or Settings > Servers."
    ))]
    InvalidToken,

    /// Failed to construct the reqwest client.
    #[snafu(display("failed to build HTTP client: {source}"))]
    ClientBuild {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },
}

impl AuthenticatedClientError {
    /// Whether this failure came from malformed auth configuration.
    #[must_use]
    pub(crate) fn is_invalid_token(&self) -> bool {
        matches!(self, Self::InvalidToken)
    }
}

/// Log a shared-client construction failure without exposing credential text.
pub(crate) fn log_authenticated_client_error(err: &AuthenticatedClientError) {
    tracing::warn!(error = %err, "failed to build authenticated HTTP client");
}

/// Errors from the startup agent-roster fetch, wrapping the underlying
/// [`skene::api::client::ApiClient`] failure directly (#7198): both call
/// sites below hit the same `GET /api/v1/nous` route the client already
/// wraps, so there is nothing proskenion-specific left to classify beyond
/// "was this a rejected credential".
#[derive(Debug)]
pub(crate) struct AgentRosterFetchError(ApiError);

impl std::fmt::Display for AgentRosterFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for AgentRosterFetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<ApiError> for AgentRosterFetchError {
    fn from(source: ApiError) -> Self {
        Self(source)
    }
}

impl AgentRosterFetchError {
    /// Whether this failure should be shown as an authentication problem
    /// instead of an empty roster.
    #[must_use]
    pub(crate) fn is_auth_failure(&self) -> bool {
        matches!(self.0, ApiError::Auth | ApiError::InvalidToken)
    }

    /// User-facing reason to place in connection state for auth failures.
    #[must_use]
    pub(crate) fn connection_failure_reason(&self) -> String {
        match self.0 {
            ApiError::Auth => {
                "Authentication failed while loading the agent roster. Check the server auth token."
                    .to_string()
            }
            ApiError::InvalidToken => {
                "Invalid auth token. Update or clear the token in Connect or Settings > Servers."
                    .to_string()
            }
            _ => "Failed to load the agent roster.".to_string(),
        }
    }
}

/// Fetch the initial sidebar agent roster via [`skene::api::client::ApiClient::agents`].
///
/// WHY(#4827): startup roster loading runs before most routed views render, but
/// it must still use the same bearer-token-bearing connection context as those
/// views. A 401/403 is returned as a typed auth error so the shell can show a
/// failed connection rather than an empty agent list.
pub(crate) async fn fetch_agent_roster(
    config: &ConnectionConfig,
) -> Result<Vec<Agent>, AgentRosterFetchError> {
    let client = skene::api::client::ApiClient::new(&config.server_url, config.auth_token.clone())?;
    Ok(client.agents().await?)
}

/// Fetch server-discovered command descriptors from the agent capability
/// payload.
///
/// WHY(#4869): Proskenion command presentation must be backed by an explicit
/// server discovery contract. Pylon already publishes per-agent tool
/// capabilities on `/api/v1/nous`; this function maps that wire contract into
/// command descriptors instead of inventing unsupported slash commands.
///
/// WHY(#7198): reuses [`skene::api::types::Agent::tools`] — already the full
/// `NousTool` shape (name/enabled/description) this mapping needs — instead
/// of a second hand-rolled `CommandDiscoveryResponse` mirror deserialized
/// from a second direct fetch of the same route.
pub(crate) async fn fetch_server_command_descriptors(
    config: &ConnectionConfig,
) -> Result<Vec<ServerCommandDescriptor>, AgentRosterFetchError> {
    let client = skene::api::client::ApiClient::new(&config.server_url, config.auth_token.clone())?;
    let agents = client.agents().await?;

    Ok(agents
        .into_iter()
        .filter(|agent| !agent.id.as_str().trim().is_empty())
        .flat_map(|agent| {
            let agent_id = agent.id.clone();
            let agent_name = agent
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| agent.id.as_str().to_string());
            agent.tools.into_iter().filter_map(move |tool| {
                let tool_name = tool.name.trim().to_string();
                if tool_name.is_empty() {
                    return None;
                }
                let description = tool
                    .description
                    .filter(|desc| !desc.trim().is_empty())
                    .unwrap_or_else(|| format!("{tool_name} server tool"));
                Some(ServerCommandDescriptor {
                    agent_id: agent_id.clone(),
                    agent_name: agent_name.clone(),
                    tool_name,
                    description,
                    enabled: tool.enabled,
                })
            })
        })
        .collect())
}

/// Persist `content` to the workspace file at `path` (relative to the vault
/// root) via [`skene::api::client::ApiClient::workspace_write_file`].
///
/// The server resolves `path` through its path-escape guard; the client only
/// ever holds workspace-relative paths. Returns a [`SaveOutcome`] mapping the
/// HTTP result to the UX-relevant cases. `if_match_mtime_ms` is not threaded
/// through from the viewer today (#7198 carried this call site behind skene
/// without changing behavior); `SaveOutcome::Conflict` stays reachable only
/// if the server independently returns 409 without a client-supplied guard.
pub(crate) async fn save_workspace_file(
    config: &ConnectionConfig,
    path: &str,
    content: &str,
) -> SaveOutcome {
    let client =
        match skene::api::client::ApiClient::new(&config.server_url, config.auth_token.clone()) {
            Ok(client) => client,
            Err(err) => return SaveOutcome::Failed(err.to_string()),
        };

    match client.workspace_write_file(path, content, None).await {
        Ok(_response) => SaveOutcome::Saved,
        Err(ApiError::Server { status: 413, .. }) => SaveOutcome::TooLarge,
        Err(ApiError::Server { status: 409, .. }) => SaveOutcome::Conflict,
        Err(err) => SaveOutcome::Failed(err.to_string()),
    }
}

/// Ask the server to open the workspace file at `path` in the operator's
/// default application via [`skene::api::client::ApiClient::workspace_open_file`].
///
/// WHY: the client never learns the absolute vault root, so opening with the
/// host's default app is a server-side action over the relative path (the
/// binary and the vault are co-located). Returns `Ok` on success or an
/// `Err` carrying a human-readable description.
pub(crate) async fn open_workspace_file(
    config: &ConnectionConfig,
    path: &str,
) -> Result<(), String> {
    let client = skene::api::client::ApiClient::new(&config.server_url, config.auth_token.clone())
        .map_err(|err| err.to_string())?;
    client
        .workspace_open_file(path)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Build a `reqwest::Client` with the Bearer token from `config` attached
/// as a default header. Views should call this instead of `Client::new()`
/// so that all API requests carry the auth token.
pub(crate) fn authenticated_client(
    config: &ConnectionConfig,
) -> Result<Client, AuthenticatedClientError> {
    build_authenticated_client(config, Some(REST_REQUEST_TIMEOUT))
}

pub(crate) fn authenticated_streaming_client(
    config: &ConnectionConfig,
) -> Result<Client, AuthenticatedClientError> {
    build_authenticated_client(config, None)
}

pub(crate) fn build_authenticated_client(
    config: &ConnectionConfig,
    timeout: Option<Duration>,
) -> Result<Client, AuthenticatedClientError> {
    let headers = default_headers(config.auth_token.as_deref())?;

    let mut builder = Client::builder()
        .cookie_store(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .default_headers(headers);
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    builder.build().context(ClientBuildSnafu)
}

fn default_headers(token: Option<&str>) -> Result<HeaderMap, AuthenticatedClientError> {
    let mut headers = HeaderMap::new();

    if let Some(token) = token {
        let value = format!("Bearer {token}");
        let header_value = HeaderValue::from_str(&value).map_err(|err| {
            tracing::debug!(kind = %err, "auth token contains invalid header characters"); // kanon:ignore SECURITY/credential-logging -- logs only the error kind, not the token
            AuthenticatedClientError::InvalidToken
        })?;
        headers.insert(AUTHORIZATION, header_value);
    }

    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    // WHY(#4823, #5059): CSRF header name/value come from the shared
    // `koina::http` constants so this client matches
    // `taxis::config::CsrfConfig::default()` without independently
    // restating the string.
    headers.insert(
        CSRF_HEADER_NAME,
        HeaderValue::from_static(DEFAULT_CSRF_HEADER_VALUE),
    );

    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn builds_client_without_token() {
        install_crypto();
        let config = ConnectionConfig::default();
        let client = match authenticated_client(&config) {
            Ok(client) => client,
            Err(err) => panic!("client without token should build: {err}"),
        };
        // WHY: ensure the client builds and is usable. The default config
        // has no token, so no Authorization header is added.
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn builds_client_with_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("test-token-123".to_string()),
            ..ConnectionConfig::default()
        };
        let client = match authenticated_client(&config) {
            Ok(client) => client,
            Err(err) => panic!("client with valid token should build: {err}"),
        };
        let debug = format!("{client:?}");
        // WHY: client builds; we cannot easily inspect default headers
        // through the public API, but a successful build covers the path.
        assert!(!debug.is_empty());
    }

    #[test]
    fn invalid_token_fails_closed_for_rest_client() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let result = authenticated_client(&config);
        assert!(matches!(
            result,
            Err(AuthenticatedClientError::InvalidToken)
        ));
    }

    #[test]
    fn empty_token_string_is_accepted() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some(String::new()),
            ..ConnectionConfig::default()
        };
        let client = match authenticated_client(&config) {
            Ok(client) => client,
            Err(err) => panic!("empty token should build: {err}"),
        };
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn streaming_client_builds_with_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("stream-token-456".to_string()),
            ..ConnectionConfig::default()
        };
        let client = match authenticated_streaming_client(&config) {
            Ok(client) => client,
            Err(err) => panic!("streaming client with valid token should build: {err}"),
        };
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn invalid_token_fails_closed_for_streaming_client() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let result = authenticated_streaming_client(&config);
        assert!(matches!(
            result,
            Err(AuthenticatedClientError::InvalidToken)
        ));
    }

    async fn spawn_auth_required_roster(
        expected_token: &'static str,
    ) -> std::io::Result<(String, JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];

            loop {
                let n = stream.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request = String::from_utf8_lossy(&request);
            let expected_header = format!("Bearer {expected_token}");
            let authorized = request.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization") && value.trim() == expected_header
                })
            });

            let body = if authorized {
                r#"{"nous":[{"id":"alice","name":"Alice","model":"test-model","emoji":"A"}]}"#
            } else {
                r#"{"error":{"code":"auth_failed","message":"missing bearer token"}}"#
            };
            let status_line = if authorized {
                "HTTP/1.1 200 OK"
            } else {
                "HTTP/1.1 401 Unauthorized"
            };
            let response = format!(
                "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );

            stream.write_all(response.as_bytes()).await?;
            Ok(())
        });

        Ok((format!("http://{addr}"), handle))
    }

    #[tokio::test]
    async fn fetch_agent_roster_sends_bearer_token() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_auth_required_roster("secret-token").await?;
        let config = ConnectionConfig {
            server_url,
            auth_token: Some("secret-token".to_string()),
            ..ConnectionConfig::default()
        };

        let agents = fetch_agent_roster(&config).await?;

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id.to_string(), "alice");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn streaming_client_sends_bearer_token() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_auth_required_roster("stream-secret").await?;
        let config = ConnectionConfig {
            server_url: server_url.clone(),
            auth_token: Some("stream-secret".to_string()),
            ..ConnectionConfig::default()
        };

        let client = match authenticated_streaming_client(&config) {
            Ok(client) => client,
            Err(err) => panic!("streaming client should build: {err}"),
        };
        let resp = client
            .get(format!("{server_url}/api/v1/events"))
            .send()
            .await?;

        assert!(resp.status().is_success());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn invalid_token_prevents_roster_request_construction() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let server_url = format!("http://{}", listener.local_addr()?);
        let config = ConnectionConfig {
            server_url,
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };

        let result = fetch_agent_roster(&config).await;

        assert!(matches!(
            result,
            Err(AgentRosterFetchError(ApiError::InvalidToken))
        ));
        let accepted = tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;
        assert!(accepted.is_err(), "invalid token must not reach the server");
        Ok(())
    }

    #[tokio::test]
    async fn fetch_agent_roster_reports_auth_failure() -> Result<(), Box<dyn Error>> {
        install_crypto();
        let (server_url, server) = spawn_auth_required_roster("secret-token").await?;
        let config = ConnectionConfig {
            server_url,
            auth_token: None,
            ..ConnectionConfig::default()
        };

        let result = fetch_agent_roster(&config).await;

        let Err(err) = result else {
            panic!("missing token should fail against auth-required roster");
        };
        assert!(err.is_auth_failure());
        assert_eq!(
            err.connection_failure_reason(),
            "Authentication failed while loading the agent roster. Check the server auth token."
        );
        server.await??;
        Ok(())
    }
}
