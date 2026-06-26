//! MCP client helpers for connecting Aletheia to external MCP servers.
//!
//! The server side of diaporeia exposes Aletheia over MCP. This module is the
//! inverse path: it lets Aletheia act as an MCP client for operator-configured
//! external servers and route those tools into organon.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation, Tool,
};
use rmcp::service::{QuitReason, RunningService};
use rmcp::transport::{ConfigureCommandExt as _, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt as _};
use tokio::sync::Mutex;

use crate::error::{Result, TransportSnafu};

/// MCP handshake timeout: how long to wait for the `initialize` response.
///
/// WHY(#5755): an unresponsive MCP server would block forever without this guard.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for `tools/list` requests.
///
/// WHY(#5757): an unresponsive peer blocks the executor indefinitely without this guard.
const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for `tools/call` requests.
///
/// WHY(#5757): an unresponsive peer stalls an entire agent turn without this guard.
/// Generous default to accommodate long-running tools; should be sourced from config.
const CALL_TOOL_TIMEOUT: Duration = Duration::from_mins(1);

/// Stdio child-process MCP server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioMcpServerConfig {
    /// Executable to spawn.
    pub command: String,
    /// Command-line arguments passed to the executable.
    pub args: Vec<String>,
    /// Optional working directory for the child process.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables for the child process.
    ///
    /// WHY(#5186, #5346): the child environment is cleared by default; only variables
    /// listed here are passed to the child process. Secrets and deployment metadata
    /// from the parent process are never inherited.
    pub env: HashMap<String, String>,
}

impl StdioMcpServerConfig {
    /// Create a new stdio MCP server config.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
        }
    }
}

/// Authentication configuration for streamable HTTP MCP connections.
///
/// WHY(#4633): this is a transport-layer view of the auth declared in taxis
/// config; env-var resolution happens here so missing env vars surface as
/// transport errors rather than config parse errors.
#[derive(Debug, Clone)]
pub enum McpAuth {
    /// Static bearer token sent as `Authorization: Bearer <token>`.
    Bearer {
        /// Bearer token value.
        token: String,
    },
    /// Static custom header name and value.
    Header {
        /// HTTP header name.
        name: String,
        /// HTTP header value.
        value: String,
    },
    /// Read a token from an environment variable at connection time.
    EnvToken {
        /// HTTP header name that will carry the token.
        header_name: String,
        /// Environment variable to read.
        env_var: String,
    },
}

impl McpAuth {
    /// Resolve this auth config to a single `(header_name, header_value)` pair.
    ///
    /// # Errors
    ///
    /// Returns a transport error when an env-var is missing.
    pub fn header(&self) -> Result<(String, String)> {
        match self {
            Self::Bearer { token } => Ok(("Authorization".to_owned(), format!("Bearer {token}"))),
            Self::Header { name, value } => Ok((name.clone(), value.clone())),
            Self::EnvToken {
                header_name,
                env_var,
            } => {
                let token = std::env::var(env_var).map_err(|e| {
                    TransportSnafu {
                        message: format!(
                            "auth env_var '{env_var}' is not set for HTTP MCP endpoint: {e}"
                        ),
                    }
                    .build()
                })?;
                Ok((header_name.clone(), token))
            }
        }
    }
}

/// Running external MCP client connection.
#[derive(Clone)]
pub struct ExternalMcpClient {
    service: Arc<Mutex<RunningService<RoleClient, ClientInfo>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern -- WHY: tokio::sync::Mutex is used (correct for async); not std::sync::Mutex
}

impl ExternalMcpClient {
    /// Spawn a stdio MCP server and complete the MCP client handshake.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the child process cannot
    /// be spawned or the MCP handshake fails.
    pub async fn connect_stdio(config: &StdioMcpServerConfig) -> Result<Self> {
        let command = config.command.clone();
        let args = config.args.clone();
        let cwd = config.cwd.clone();
        let env = config.env.clone();
        let transport = TokioChildProcess::new(tokio::process::Command::new(&command).configure(
            move |cmd| {
                // WHY(#5186, #5346): clear the inherited parent env first so the
                // child never receives secrets, tokens, or deployment metadata.
                // Only the explicitly configured variables are forwarded.
                cmd.env_clear();
                cmd.args(args);
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                cmd.envs(env);
            },
        ))
        .map_err(|e| {
            TransportSnafu {
                message: format!("failed to spawn stdio MCP server '{command}': {e}"),
            }
            .build()
        })?;

        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("aletheia-diaporeia", env!("CARGO_PKG_VERSION")),
        );

        // WHY(#5755): without a timeout the handshake blocks forever when the child
        // never emits the `initialize` response.
        let service = tokio::time::timeout(HANDSHAKE_TIMEOUT, client_info.serve(transport))
            .await
            .map_err(|_e| {
                TransportSnafu {
                    message: format!(
                        "stdio MCP server '{command}' did not respond to handshake within {}s",
                        HANDSHAKE_TIMEOUT.as_secs()
                    ),
                }
                .build()
            })?
            .map_err(|e| {
                TransportSnafu {
                    message: format!("failed to initialize stdio MCP server '{command}': {e}"),
                }
                .build()
            })?;

        Ok(Self {
            service: Arc::new(Mutex::new(service)), // kanon:ignore RUST/no-arc-mutex-anti-pattern -- WHY: tokio::sync::Mutex is used (correct for async); not std::sync::Mutex
        })
    }

    /// Connect to a streamable HTTP MCP server and complete the MCP client handshake.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the MCP handshake fails.
    pub async fn connect_streamable_http(endpoint: &str) -> Result<Self> {
        Self::connect_streamable_http_with_auth(endpoint, None).await
    }

    /// Connect to a streamable HTTP MCP server with optional authentication.
    ///
    /// WHY(#4633): authenticated MCP servers need bearer tokens, static custom
    /// headers, or env-var tokens without embedding secrets in the URL.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the MCP handshake fails, a
    /// header cannot be parsed, or an env-var token is missing.
    pub async fn connect_streamable_http_with_auth(
        endpoint: &str,
        auth: Option<&McpAuth>,
    ) -> Result<Self> {
        use http::{HeaderName, HeaderValue};
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let mut custom_headers = HashMap::new();
        if let Some(auth) = auth {
            let (name, value) = auth.header()?;
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
                TransportSnafu {
                    message: format!("invalid HTTP header name '{name}': {e}"),
                }
                .build()
            })?;
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                TransportSnafu {
                    message: format!("invalid HTTP header value for '{name}': {e}"),
                }
                .build()
            })?;
            custom_headers.insert(header_name, header_value);
        }

        let config = StreamableHttpClientTransportConfig::with_uri(endpoint.to_owned())
            .custom_headers(custom_headers);
        let transport = StreamableHttpClientTransport::from_config(config);
        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("aletheia-diaporeia", env!("CARGO_PKG_VERSION")),
        );

        // WHY(#5755): without a timeout the handshake blocks forever when the endpoint
        // never responds to the `initialize` request.
        let service = tokio::time::timeout(HANDSHAKE_TIMEOUT, client_info.serve(transport))
            .await
            .map_err(|_e| {
                TransportSnafu {
                    message: format!(
                        "streamable HTTP MCP server '{endpoint}' did not respond to handshake within {}s",
                        HANDSHAKE_TIMEOUT.as_secs()
                    ),
                }
                .build()
            })?
            .map_err(|e| {
                TransportSnafu {
                    message: format!(
                        "failed to initialize streamable HTTP MCP server '{endpoint}': {e}"
                    ),
                }
                .build()
            })?;

        Ok(Self {
            service: Arc::new(Mutex::new(service)), // kanon:ignore RUST/no-arc-mutex-anti-pattern -- WHY: tokio::sync::Mutex is used (correct for async); not std::sync::Mutex
        })
    }

    /// List all tools exposed by the connected MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the MCP request fails or times out.
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let peer = {
            let service = self.service.lock().await;
            service.peer().clone()
        };
        // WHY(#5757): without a timeout a stalled peer blocks the caller indefinitely.
        tokio::time::timeout(LIST_TOOLS_TIMEOUT, peer.list_all_tools())
            .await
            .map_err(|_e| {
                TransportSnafu {
                    message: format!(
                        "MCP tools/list timed out after {}s",
                        LIST_TOOLS_TIMEOUT.as_secs()
                    ),
                }
                .build()
            })?
            .map_err(|e| {
                TransportSnafu {
                    message: format!("MCP tools/list failed: {e}"),
                }
                .build()
            })
    }

    /// Call a tool on the connected MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the MCP request fails or times out.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult> {
        let peer = {
            let service = self.service.lock().await;
            service.peer().clone()
        };
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        // WHY(#5757): without a timeout a hung peer stalls an entire agent turn.
        tokio::time::timeout(CALL_TOOL_TIMEOUT, peer.call_tool(params))
            .await
            .map_err(|_e| {
                TransportSnafu {
                    message: format!(
                        "MCP tools/call '{name}' timed out after {}s",
                        CALL_TOOL_TIMEOUT.as_secs()
                    ),
                }
                .build()
            })?
            .map_err(|e| {
                TransportSnafu {
                    message: format!("MCP tools/call '{name}' failed: {e}"),
                }
                .build()
            })
    }

    /// Close the client connection and underlying child process.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Transport`] when the service task cannot
    /// be joined cleanly.
    pub async fn close(&self) -> Result<QuitReason> {
        let mut service = self.service.lock().await;
        service.close().await.map_err(|e| {
            TransportSnafu {
                message: format!("failed to close MCP client: {e}"),
            }
            .build()
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use tempfile::TempDir;

    use super::*;

    fn fake_mcp_server() -> (TempDir, StdioMcpServerConfig) {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("fake-mcp.sh");
        let mut file = fs::File::create(&script).expect("create script");
        file.write_all(
            br#"#!/bin/sh
IFS= read -r init
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"0.1.0"}}}'
IFS= read -r initialized
IFS= read -r list
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object","properties":{"message":{"type":"string","description":"Message"}},"required":["message"]}}]}}'
while IFS= read -r line; do
  case "$line" in
    *'"method":"tools/call"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"called"}],"isError":false}}'
      ;;
  esac
done
"#,
        )
        .expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("chmod");
        }

        let config = StdioMcpServerConfig::new(script.display().to_string());
        (dir, config)
    }

    #[tokio::test]
    async fn stdio_child_process_lists_tools_and_closes() {
        let (_dir, config) = fake_mcp_server();
        let client = ExternalMcpClient::connect_stdio(&config)
            .await
            .expect("connect");

        let tools = client.list_tools().await.expect("list tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools.first().expect("one tool").name.as_ref(), "echo");

        let _reason = client.close().await.expect("close");
    }

    #[tokio::test]
    async fn stdio_child_inherits_no_parent_env_variables() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("env-check-mcp.sh");
        let mut file = fs::File::create(&script).expect("create script");
        // WHY: this server echoes back the value of SECRET_VAR from its environment.
        // After connect_stdio applies env_clear, that variable must be absent.
        file.write_all(
            br#"#!/bin/sh
IFS= read -r init
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"env-check","version":"0.1.0"}}}'
IFS= read -r initialized
IFS= read -r list
secret_val="${SECRET_VAR:-__absent__}"
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"check\",\"description\":\"$secret_val\",\"inputSchema\":{\"type\":\"object\"}}]}}"
while IFS= read -r line; do :; done
"#,
        )
        .expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = fs::metadata(&script).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("chmod");
        }
        // WHY: close the script file before executing it. On Unix a writable file
        // handle left open causes ETXTBSY ("Text file busy") when trying to spawn
        // the script as an executable.
        drop(file);

        // Set a "secret" in the test process environment.
        // WHY: std::env::set_var in a test is acceptable since tests run in-process
        // and env isolation is the point being tested.
        #[expect(
            unsafe_code,
            reason = "std::env::set_var is unsafe in multi-threaded contexts; safe here because we are the only writer in this test"
        )]
        unsafe {
            std::env::set_var("SECRET_VAR", "should-not-leak");
        }

        let config = StdioMcpServerConfig::new(script.display().to_string());
        let client = ExternalMcpClient::connect_stdio(&config)
            .await
            .expect("connect");

        let tools = client.list_tools().await.expect("list tools");
        let description = tools
            .first()
            .expect("one tool")
            .description
            .as_deref()
            .unwrap_or("");
        assert_eq!(
            description, "__absent__",
            "child must not inherit SECRET_VAR from parent: got '{description}'"
        );

        let _reason = client.close().await.expect("close");
    }
}
