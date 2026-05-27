//! MCP client helpers for connecting Aletheia to external MCP servers.
//!
//! The server side of diaporeia exposes Aletheia over MCP. This module is the
//! inverse path: it lets Aletheia act as an MCP client for operator-configured
//! external servers and route those tools into organon.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation, Tool,
};
use rmcp::service::{QuitReason, RunningService};
use rmcp::transport::{ConfigureCommandExt as _, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt as _};
use tokio::sync::Mutex;

use crate::error::{Result, TransportSnafu};

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
        let service = client_info.serve(transport).await.map_err(|e| {
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
        let transport = StreamableHttpClientTransport::from_uri(endpoint.to_owned());
        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("aletheia-diaporeia", env!("CARGO_PKG_VERSION")),
        );
        let service = client_info.serve(transport).await.map_err(|e| {
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
    /// Returns [`crate::error::Error::Transport`] when the MCP request fails.
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let peer = {
            let service = self.service.lock().await;
            service.peer().clone()
        };
        peer.list_all_tools().await.map_err(|e| {
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
    /// Returns [`crate::error::Error::Transport`] when the MCP request fails.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult> {
        let peer = {
            let service = self.service.lock().await;
            service.peer().clone()
        };
        peer.call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
            .await
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
}
