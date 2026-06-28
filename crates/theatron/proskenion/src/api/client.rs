//! Shared authenticated HTTP client for view-layer API requests.

use std::time::Duration;

use reqwest::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use snafu::{ResultExt, Snafu};

use crate::state::commands::ServerCommandDescriptor;
use crate::state::connection::ConnectionConfig;

use skene::api::types::{Agent, AgentsResponse};

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

/// Errors from the startup agent-roster fetch.
#[derive(Debug, Snafu)]
pub(crate) enum AgentRosterFetchError {
    /// The server rejected the configured credentials.
    #[snafu(display("authentication failed while loading the agent roster"))]
    Auth,

    /// The request failed before a response was received.
    #[snafu(display("failed to request agent roster: {source}"))]
    Request {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },

    /// The server returned a non-success response other than auth failure.
    #[snafu(display("agent roster request returned {status}: {message}"))]
    Server {
        /// HTTP status code.
        status: u16,
        /// Human-readable server response.
        message: String,
    },

    /// The server returned success with an unparseable response body.
    #[snafu(display("failed to decode agent roster response: {source}"))]
    Decode {
        /// Underlying decode error.
        source: reqwest::Error,
    },
}

impl AgentRosterFetchError {
    /// Whether this failure should be shown as an authentication problem
    /// instead of an empty roster.
    #[must_use]
    pub(crate) fn is_auth_failure(&self) -> bool {
        matches!(self, Self::Auth)
    }

    /// User-facing reason to place in connection state for auth failures.
    #[must_use]
    pub(crate) fn connection_failure_reason(&self) -> &'static str {
        match self {
            Self::Auth => {
                "Authentication failed while loading the agent roster. Check the server auth token."
            }
            Self::Request { .. } | Self::Server { .. } | Self::Decode { .. } => {
                "Failed to load the agent roster."
            }
        }
    }
}

/// Fetch the initial sidebar agent roster using the shared authenticated
/// request builder.
///
/// WHY(#4827): startup roster loading runs before most routed views render, but
/// it must still use the same bearer-token-bearing connection context as those
/// views. A 401/403 is returned as a typed auth error so the shell can show a
/// failed connection rather than an empty agent list.
pub(crate) async fn fetch_agent_roster(
    config: &ConnectionConfig,
) -> Result<Vec<Agent>, AgentRosterFetchError> {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/nous");

    let resp = client.get(&url).send().await.context(RequestSnafu)?;
    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return AuthSnafu.fail();
    }

    if !status.is_success() {
        let status_code = status.as_u16();
        let detail = match resp.text().await {
            Ok(text) => text,
            Err(err) => err.to_string(),
        };
        let message = skene::api::error::parse_pylon_error_body(&detail).map_or_else(
            || {
                let trimmed = detail.trim();
                if trimmed.is_empty() {
                    status.to_string()
                } else {
                    trimmed.to_string()
                }
            },
            |detail| detail.message,
        );
        return ServerSnafu {
            status: status_code,
            message,
        }
        .fail();
    }

    let wrapper: AgentsResponse = resp.json().await.context(DecodeSnafu)?;
    Ok(wrapper.nous)
}

/// Fetch server-discovered command descriptors from the agent capability
/// payload.
///
/// WHY(#4869): Proskenion command presentation must be backed by an explicit
/// server discovery contract. Pylon already publishes per-agent tool
/// capabilities on `/api/v1/nous`; this function maps that wire contract into
/// command descriptors instead of inventing unsupported slash commands.
pub(crate) async fn fetch_server_command_descriptors(
    config: &ConnectionConfig,
) -> Result<Vec<ServerCommandDescriptor>, AgentRosterFetchError> {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/nous");

    let resp = client.get(&url).send().await.context(RequestSnafu)?;
    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return AuthSnafu.fail();
    }

    if !status.is_success() {
        let status_code = status.as_u16();
        let detail = match resp.text().await {
            Ok(text) => text,
            Err(err) => err.to_string(),
        };
        let message = skene::api::error::parse_pylon_error_body(&detail).map_or_else(
            || {
                let trimmed = detail.trim();
                if trimmed.is_empty() {
                    status.to_string()
                } else {
                    trimmed.to_string()
                }
            },
            |detail| detail.message,
        );
        return ServerSnafu {
            status: status_code,
            message,
        }
        .fail();
    }

    let wrapper: CommandDiscoveryResponse = resp.json().await.context(DecodeSnafu)?;
    Ok(wrapper.into_descriptors())
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CommandDiscoveryResponse {
    #[serde(default, alias = "agents")]
    nous: Vec<CommandDiscoveryAgent>,
}

impl CommandDiscoveryResponse {
    fn into_descriptors(self) -> Vec<ServerCommandDescriptor> {
        self.nous
            .into_iter()
            .filter(|agent| !agent.id.trim().is_empty())
            .flat_map(|agent| {
                let agent_id: skene::id::NousId = agent.id.as_str().into();
                let agent_name = agent
                    .name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(agent.id);
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
            .collect()
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CommandDiscoveryAgent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tools: Vec<CommandDiscoveryTool>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CommandDiscoveryTool {
    #[serde(default)]
    name: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    description: Option<String>,
}

/// Persist `content` to the workspace file at `path` (relative to the vault
/// root) via the workspace content write endpoint.
///
/// The server resolves `path` through its path-escape guard; the client only
/// ever holds workspace-relative paths. Returns a [`SaveOutcome`] mapping the
/// HTTP result to the UX-relevant cases.
pub(crate) async fn save_workspace_file(
    config: &ConnectionConfig,
    path: &str,
    content: &str,
) -> SaveOutcome {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/workspace/files/content");
    let body = serde_json::json!({ "path": path, "content": content });

    match client.put(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => SaveOutcome::Saved,
        Ok(resp) if resp.status().as_u16() == 413 => SaveOutcome::TooLarge,
        Ok(resp) if resp.status().as_u16() == 409 => SaveOutcome::Conflict,
        Ok(resp) => {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            if detail.is_empty() {
                SaveOutcome::Failed(format!("server returned {status}"))
            } else {
                SaveOutcome::Failed(format!("server returned {status}: {}", detail.trim()))
            }
        }
        Err(e) => SaveOutcome::Failed(format!("connection error: {e}")),
    }
}

/// Ask the server to open the workspace file at `path` in the operator's
/// default application via `POST /api/v1/workspace/open`.
///
/// WHY: the client never learns the absolute vault root, so opening with the
/// host's default app is a server-side action over the relative path (the
/// binary and the vault are co-located). Returns `Ok` on success or an
/// `Err` carrying a human-readable description.
pub(crate) async fn open_workspace_file(
    config: &ConnectionConfig,
    path: &str,
) -> Result<(), String> {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/workspace/open");
    let body = serde_json::json!({ "path": path });

    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            if detail.is_empty() {
                Err(format!("server returned {status}"))
            } else {
                Err(format!("server returned {status}: {}", detail.trim()))
            }
        }
        Err(e) => Err(format!("connection error: {e}")),
    }
}

/// Build a `reqwest::Client` with the Bearer token from `config` attached
/// as a default header. Views should call this instead of `Client::new()`
/// so that all API requests carry the auth token.
pub(crate) fn authenticated_client(config: &ConnectionConfig) -> Client {
    build_authenticated_client(config, Some(Duration::from_secs(30)))
}

pub(crate) fn authenticated_streaming_client(config: &ConnectionConfig) -> Client {
    build_authenticated_client(config, None)
}

fn build_authenticated_client(config: &ConnectionConfig, timeout: Option<Duration>) -> Client {
    let mut headers = HeaderMap::new();

    if let Some(ref token) = config.auth_token
        && let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(AUTHORIZATION, val);
    }

    // WHY: fall back to default client if builder fails (e.g. no TLS provider
    // installed yet); views already handle HTTP errors gracefully.
    let mut builder = Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(30));
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    builder.build().unwrap_or_else(|err| {
        tracing::warn!(error = %err, "failed to build authenticated HTTP client");
        Client::new()
    })
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
        let client = authenticated_client(&config);
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
        let client = authenticated_client(&config);
        let debug = format!("{client:?}");
        // WHY: client builds; we cannot easily inspect default headers
        // through the public API, but a successful build covers the path.
        assert!(!debug.is_empty());
    }

    #[test]
    fn invalid_token_falls_through_to_default() {
        install_crypto();
        // NOTE: A token with non-ASCII bytes triggers HeaderValue::from_str
        // failure and the function silently skips adding the header.
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn empty_token_string_is_accepted() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some(String::new()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_client(&config);
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
        let client = authenticated_streaming_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn streaming_client_ignores_invalid_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_streaming_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
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
