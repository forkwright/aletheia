//! MCP client helpers for connecting Aletheia to external MCP servers.
//!
//! The server side of diaporeia exposes Aletheia over MCP. This module is the
//! inverse path: it lets Aletheia act as an MCP client for operator-configured
//! external servers and route those tools into organon.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use organon::sandbox::{SandboxPolicy, apply_sandbox};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation, Tool,
};
use rmcp::service::{QuitReason, RunningService};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt as _};
use tokio::sync::Mutex;
use tracing::{info, warn};

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

/// Trust posture for a stdio child-process MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdioMcpTrustPolicy {
    /// Run the child under the configured process sandbox.
    Sandboxed,
    /// Run without the process sandbox. Only for operator-audited local code.
    TrustedLocal,
}

impl fmt::Display for StdioMcpTrustPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sandboxed => f.write_str("sandboxed"),
            Self::TrustedLocal => f.write_str("trusted-local"),
        }
    }
}

/// Stdio child-process MCP server configuration.
#[derive(Debug, Clone)]
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
    /// Trust posture for the child process.
    pub trust: StdioMcpTrustPolicy,
    /// Resolved sandbox policy applied before exec when `trust == Sandboxed`.
    pub sandbox_policy: Option<SandboxPolicy>,
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
            trust: StdioMcpTrustPolicy::Sandboxed,
            sandbox_policy: None,
        }
    }

    /// Attach a resolved sandbox policy for a sandboxed stdio server.
    #[must_use]
    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.trust = StdioMcpTrustPolicy::Sandboxed;
        self.sandbox_policy = Some(policy);
        self
    }

    /// Mark this stdio server as operator-trusted local code.
    #[must_use]
    pub fn trusted_local(mut self) -> Self {
        self.trust = StdioMcpTrustPolicy::TrustedLocal;
        self.sandbox_policy = None;
        self
    }
}

fn log_stdio_mcp_posture(
    command: &str,
    cwd: Option<&PathBuf>,
    env: &HashMap<String, String>,
    trust: StdioMcpTrustPolicy,
    sandbox_policy: Option<&SandboxPolicy>,
) {
    let mut env_keys: Vec<&str> = env.keys().map(String::as_str).collect();
    env_keys.sort_unstable();

    if let Some(policy) = sandbox_policy {
        info!(
            command = %command,
            cwd = ?cwd,
            trust = %trust,
            env_policy = "clear-then-allowlist",
            configured_env_vars = ?env_keys,
            sandbox_enabled = policy.enabled,
            sandbox_enforcement = ?policy.enforcement,
            sandbox_egress = ?policy.egress,
            sandbox_read_paths = policy.read_paths.len(),
            sandbox_write_paths = policy.write_paths.len(),
            sandbox_exec_paths = policy.exec_paths.len(),
            "starting stdio MCP server"
        );
    } else {
        warn!(
            command = %command,
            cwd = ?cwd,
            trust = %trust,
            env_policy = "clear-then-allowlist",
            configured_env_vars = ?env_keys,
            sandbox_enabled = false,
            "starting stdio MCP server without process sandbox"
        );
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
        let sandbox_policy = match config.trust {
            StdioMcpTrustPolicy::Sandboxed => {
                Some(config.sandbox_policy.clone().ok_or_else(|| {
                    TransportSnafu {
                        message: format!(
                            "stdio MCP server '{command}' requires a resolved sandbox policy \
                             when trust is sandboxed"
                        ),
                    }
                    .build()
                })?)
            }
            StdioMcpTrustPolicy::TrustedLocal => {
                if config.sandbox_policy.is_some() {
                    warn!(
                        command = %command,
                        trust = %config.trust,
                        "stdio MCP server supplied a sandbox policy but trust disables sandboxing"
                    );
                }
                None
            }
        };

        log_stdio_mcp_posture(
            &command,
            cwd.as_ref(),
            &env,
            config.trust,
            sandbox_policy.as_ref(),
        );

        let mut cmd = tokio::process::Command::new(&command);
        // WHY(#5186, #5346): clear the inherited parent env first so the child
        // never receives secrets, tokens, or deployment metadata. Only the
        // explicitly configured variables are forwarded.
        cmd.env_clear();
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.envs(env);

        if let Some(policy) = sandbox_policy {
            apply_sandbox(cmd.as_std_mut(), policy).map_err(|e| {
                TransportSnafu {
                    message: format!(
                        "failed to apply sandbox for stdio MCP server '{command}': {e}"
                    ),
                }
                .build()
            })?;
        }

        let transport = TokioChildProcess::new(cmd).map_err(|e| {
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
    use std::path::Path;

    use organon::sandbox::{SandboxConfig, SandboxEnforcement, SandboxPolicy};
    use tempfile::TempDir;

    use super::*;

    fn test_sandbox_policy(dir: &Path) -> SandboxPolicy {
        SandboxConfig {
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        }
        .build_policy(dir, &[dir.to_path_buf()])
    }

    fn sandboxed_config(script: &Path, dir: &Path) -> StdioMcpServerConfig {
        let mut config = StdioMcpServerConfig::new(script.display().to_string())
            .with_sandbox_policy(test_sandbox_policy(dir));
        config.cwd = Some(dir.to_path_buf());
        config
    }

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

        let config = sandboxed_config(&script, dir.path());
        (dir, config)
    }

    #[tokio::test]
    async fn sandboxed_stdio_child_requires_resolved_policy() {
        let config = StdioMcpServerConfig::new("local-mcp");
        let err = match ExternalMcpClient::connect_stdio(&config).await {
            Ok(_client) => panic!("sandboxed stdio MCP server without policy must fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("requires a resolved sandbox policy"),
            "error should explain missing sandbox policy: {err}"
        );
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

        let config = sandboxed_config(&script, dir.path());
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

        #[expect(unsafe_code, reason = "test cleans up the env var it set above")]
        unsafe {
            std::env::remove_var("SECRET_VAR");
        }
    }
}
