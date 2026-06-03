//! Experimental Claude CLI subprocess engine with OAuth token injection.
//!
//! WHY: This engine uses the `claude` CLI transport while preserving the
//! dispatch-engine boundary for future native SDK integration. It is not a
//! native HTTP/SSE Agent SDK client yet.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use serde::Serialize;
use tokio::process::Command;

use crate::engine::{AgentOptions, DispatchEngine, SessionHandle, SessionSpec};
use crate::error::{self, Result};
use crate::http::session::ProcessSessionHandle;
use crate::http::stream::EventStream;

/// Configuration for the experimental Claude CLI bridge.
#[derive(Clone)]
pub struct AgentSdkConfig {
    /// Default model identifier (e.g., "claude-opus-4", "claude-sonnet-4").
    pub default_model: String,
    /// Skip permission checks during dispatch.
    pub skip_permissions: bool,
    /// Disable MCP plugin loading.
    pub disable_plugins: bool,
    /// Optional `OAuth` token for API authentication.
    pub oauth_token: Option<String>,
    /// Optional MCP server configurations.
    pub mcp_servers: Vec<McpServerConfig>,
}

impl Default for AgentSdkConfig {
    fn default() -> Self {
        Self {
            default_model: "claude-sonnet-4".to_string(),
            skip_permissions: false,
            disable_plugins: false,
            oauth_token: None,
            mcp_servers: Vec::new(),
        }
    }
}

impl std::fmt::Debug for AgentSdkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSdkConfig")
            .field("default_model", &self.default_model)
            .field("skip_permissions", &self.skip_permissions)
            .field("disable_plugins", &self.disable_plugins)
            .field("has_oauth", &self.oauth_token.is_some())
            .field("mcp_server_count", &self.mcp_servers.len())
            .finish()
    }
}

/// MCP server configuration.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Server name/identifier.
    pub name: String,
    /// Server command to execute.
    pub command: String,
    /// Arguments for the server command.
    pub args: Vec<String>,
    /// Environment variables.
    pub env: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct McpConfigFile {
    #[serde(rename = "mcpServers")]
    mcp_servers: BTreeMap<String, McpServerEntry>,
}

#[derive(Debug, Serialize)]
struct McpServerEntry {
    command: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}

/// Experimental Claude CLI dispatch engine.
///
/// WHY: Provides CLI subprocess integration with `OAuth` token injection,
/// permissions, and MCP configuration fields while the native SDK path remains
/// unwired. The public type name is kept for compatibility with existing
/// configuration code, but the current transport is a `claude` CLI subprocess,
/// not a native Agent SDK client.
pub struct AgentSdkEngine {
    config: AgentSdkConfig,
    binary: String,
}

impl AgentSdkEngine {
    /// Create a new experimental Claude CLI bridge with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine cannot be initialized.
    ///
    /// Current validation is intentionally narrow: only the default model ID is
    /// checked here. `OAuth` token validation, MCP plugin wiring, permission
    /// enforcement, and native SDK availability checks remain future work.
    pub fn new(config: AgentSdkConfig) -> Result<Self> {
        // WHY: Verify the model identifier is valid during construction.
        if config.default_model.is_empty() {
            return error::InvalidModelSnafu {
                model: "(empty)".to_string(),
            }
            .fail();
        }

        // Future-work: validate OAuth token, init MCP plugin system, set up
        // permission system, and replace the CLI subprocess with a native SDK
        // transport when that integration exists.
        Ok(Self {
            config,
            binary: "claude".to_owned(),
        })
    }

    /// Get the configured default model.
    #[must_use]
    pub fn default_model(&self) -> &str {
        &self.config.default_model
    }

    /// Check if permissions are enabled.
    #[must_use]
    pub fn permissions_enabled(&self) -> bool {
        !self.config.skip_permissions
    }

    /// Check if plugins are enabled.
    #[must_use]
    pub fn plugins_enabled(&self) -> bool {
        !self.config.disable_plugins
    }

    /// Build the CLI argument vector for a session.
    fn build_args(&self, options: &AgentOptions) -> Vec<String> {
        let mut args = Vec::new();

        args.extend(["--output-format".to_owned(), "stream-json".to_owned()]);
        args.push("--verbose".to_owned());

        let model = options
            .model
            .as_deref()
            .unwrap_or(&self.config.default_model);
        args.extend(["--model".to_owned(), model.to_owned()]);

        // WHY: Apply permission mode based on config (if not skipped).
        if !self.config.skip_permissions
            && let Some(ref mode) = options.permission_mode
        {
            args.extend(["--permission-mode".to_owned(), mode.clone()]);
        }

        if let Some(ref prompt) = options.system_prompt {
            args.extend(["--system-prompt".to_owned(), prompt.clone()]);
        }

        if let Some(turns) = options.max_turns {
            args.extend(["--max-turns".to_owned(), turns.to_string()]);
        }

        for dir in &options.additional_dirs {
            args.extend(["--add-dir".to_owned(), dir.to_string_lossy().into_owned()]);
        }

        if !self.config.mcp_servers.is_empty() {
            if let Some(mcp_config) = self.mcp_config_json() {
                // WHY: Claude CLI loads MCP servers from a session-local JSON config string.
                args.extend(["--mcp-config".to_owned(), mcp_config]);
            } else {
                tracing::warn!(
                    mcp_server_count = self.config.mcp_servers.len(),
                    "failed to serialize MCP server configuration; skipping --mcp-config"
                );
            }
        }

        args
    }

    fn mcp_config_json(&self) -> Option<String> {
        let mcp_servers = self
            .config
            .mcp_servers
            .iter()
            .map(|server| {
                let env = server
                    .env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();

                (
                    server.name.clone(),
                    McpServerEntry {
                        command: server.command.clone(),
                        args: server.args.clone(),
                        env,
                    },
                )
            })
            .collect();

        serde_json::to_string(&McpConfigFile { mcp_servers }).ok()
    }

    /// Spawn a subprocess and return a session handle.
    fn launch(mut cmd: Command) -> Result<ProcessSessionHandle> {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // INVARIANT: kill_on_drop ensures cleanup if the handle is dropped
        // without explicit wait/abort.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            error::EngineSnafu {
                detail: format!("failed to spawn claude CLI subprocess: {e}"),
            }
            .build()
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            error::EngineSnafu {
                detail: "stdout not captured from subprocess",
            }
            .build()
        })?;

        let stream = EventStream::new(stdout);
        Ok(ProcessSessionHandle::new(child, stream, String::new()))
    }
}

// WHY: manual Debug impl rather than `#[derive]` because the config holds an
// `oauth_token: Option<String>` we must never log. The summary form shows
// `has_oauth: bool` and `mcp_server_count: usize` instead of the raw values.
impl std::fmt::Debug for AgentSdkEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSdkEngine")
            .field("binary", &self.binary)
            .field("default_model", &self.config.default_model)
            .field("skip_permissions", &self.config.skip_permissions)
            .field("disable_plugins", &self.config.disable_plugins)
            .field("has_oauth", &self.config.oauth_token.is_some())
            .field("mcp_server_count", &self.config.mcp_servers.len())
            .finish()
    }
}

impl DispatchEngine for AgentSdkEngine {
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let mut cmd = Command::new(&self.binary);
            cmd.args(["-p", &spec.prompt]);
            cmd.args(self.build_args(options));

            // WHY: cwd is set on the Command, not as a CLI flag.
            let cwd = spec.cwd.as_ref().or(options.cwd.as_ref());
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }

            // When prompt_components is present, the static prefix is already
            // in system_prompt and the dynamic suffix is already in prompt.
            if let Some(ref sys) = spec.system_prompt
                && options.system_prompt.is_none()
            {
                cmd.args(["--system-prompt", sys]);
            }

            // WHY: Inject OAuth token if configured.
            if let Some(ref token) = self.config.oauth_token {
                cmd.env("CLAUDE_API_TOKEN", token);
            }

            tracing::debug!(
                prompt_len = spec.prompt.len(),
                model = options
                    .model
                    .as_deref()
                    .unwrap_or(&self.config.default_model),
                "spawning claude CLI subprocess session"
            );

            let handle = Self::launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }

    fn resume_session<'a>(
        &'a self,
        session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let mut cmd = Command::new(&self.binary);
            cmd.args(["--resume", session_id, "-p", prompt]);
            cmd.args(self.build_args(options));

            // WHY: Inject OAuth token if configured.
            if let Some(ref token) = self.config.oauth_token {
                cmd.env("CLAUDE_API_TOKEN", token);
            }

            tracing::debug!(session_id, "resuming claude CLI session");

            let handle = Self::launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions and helpers"
)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::engine::AgentOptions;

    fn test_engine(mcp_servers: Vec<McpServerConfig>) -> AgentSdkEngine {
        AgentSdkEngine::new(AgentSdkConfig {
            default_model: "claude-sonnet-4-20250514".to_owned(),
            skip_permissions: false,
            disable_plugins: false,
            oauth_token: None,
            mcp_servers,
        })
        .expect("engine should initialize")
    }

    #[test]
    fn build_args_includes_mcp_config() {
        let mut env = HashMap::new();
        env.insert("CACHE_DIR".to_owned(), "/tmp/cache".to_owned());

        let engine = test_engine(vec![
            McpServerConfig {
                name: "filesystem".to_owned(),
                command: "npx".to_owned(),
                args: vec![
                    "-y".to_owned(),
                    "@modelcontextprotocol/server-filesystem".to_owned(),
                    "/Users/me/projects".to_owned(),
                ],
                env,
            },
            McpServerConfig {
                name: "docs".to_owned(),
                command: "python".to_owned(),
                args: vec!["server.py".to_owned()],
                env: HashMap::new(),
            },
        ]);

        let args = engine.build_args(&AgentOptions::new().model("claude-opus-4-20250514"));
        let mcp_idx = args
            .iter()
            .position(|arg| arg == "--mcp-config")
            .expect("mcp config flag");
        let config: serde_json::Value =
            serde_json::from_str(&args[mcp_idx + 1]).expect("parse mcp config");

        assert_eq!(
            config,
            serde_json::json!({
                "mcpServers": {
                    "docs": {
                        "command": "python",
                        "args": ["server.py"]
                    },
                    "filesystem": {
                        "command": "npx",
                        "args": [
                            "-y",
                            "@modelcontextprotocol/server-filesystem",
                            "/Users/me/projects"
                        ],
                        "env": {
                            "CACHE_DIR": "/tmp/cache"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn build_args_omits_mcp_config_when_empty() {
        let engine = test_engine(Vec::new());
        let args = engine.build_args(&AgentOptions::new());

        assert!(!args.iter().any(|arg| arg == "--mcp-config"));
    }
}
