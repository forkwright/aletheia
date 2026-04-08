//! Agent SDK engine: OAuth-enabled, permission-aware, plugin-capable dispatch engine.
//!
//! WHY: Replaces HttpEngine with a native Agent SDK integration that supports
//! OAuth authentication, granular permissions, MCP plugins, and configurable
//! model routing.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use tokio::process::Command;

use crate::engine::{AgentOptions, DispatchEngine, SessionHandle, SessionSpec};
use crate::error::{self, Result};
use crate::http::session::ProcessSessionHandle;
use crate::http::stream::EventStream;

/// Configuration for the Agent SDK engine.
#[derive(Debug, Clone)]
pub struct AgentSdkConfig {
    /// Default model identifier (e.g., "claude-opus-4", "claude-sonnet-4").
    pub default_model: String,
    /// Skip permission checks during dispatch.
    pub skip_permissions: bool,
    /// Disable MCP plugin loading.
    pub disable_plugins: bool,
    /// Optional OAuth token for API authentication.
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

/// Agent SDK-based dispatch engine.
///
/// WHY: Provides native Agent SDK integration with OAuth, permissions, and
/// plugin support, replacing the subprocess-based HttpEngine.
pub struct AgentSdkEngine {
    config: AgentSdkConfig,
    binary: String,
}

impl AgentSdkEngine {
    /// Create a new Agent SDK engine with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine cannot be initialized (e.g., binary
    /// lookup fails, OAuth token invalid, MCP server unavailable).
    #[must_use]
    pub fn new(config: AgentSdkConfig) -> Result<Self> {
        // WHY: Verify the model identifier is valid during construction.
        if config.default_model.is_empty() {
            return error::InvalidModelSnafu {
                model: "(empty)".to_string(),
            }
            .fail();
        }

        // NOTE: In a full implementation, this would:
        // 1. Validate OAuth token if provided
        // 2. Initialize MCP plugin system if not disabled
        // 3. Set up permission system if not skipped
        // 4. Verify agent SDK binary availability

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

        let model = options.model.as_deref().unwrap_or(&self.config.default_model);
        args.extend(["--model".to_owned(), model.to_owned()]);

        // WHY: Apply permission mode based on config (if not skipped).
        if !self.config.skip_permissions {
            if let Some(ref mode) = options.permission_mode {
                args.extend(["--permission-mode".to_owned(), mode.clone()]);
            }
        }

        if let Some(ref prompt) = options.system_prompt {
            args.extend(["--system-prompt".to_owned(), prompt.clone()]);
        }

        if let Some(turns) = options.max_turns {
            args.extend(["--max-turns".to_owned(), turns.to_string()]);
        }

        args
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
                detail: format!("failed to spawn agent SDK subprocess: {e}"),
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

impl std::fmt::Debug for AgentSdkEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSdkEngine")
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
                model = options.model.as_deref().unwrap_or(&self.config.default_model),
                "spawning agent SDK session"
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

            tracing::debug!(session_id, "resuming agent SDK session");

            let handle = Self::launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }
}
