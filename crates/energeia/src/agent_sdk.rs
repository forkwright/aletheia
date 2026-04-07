//! Agent SDK configuration for dispatch session execution.
//!
//! Consolidates all configuration needed to spawn CC agent sessions:
//! binary path, auth, model defaults, subprocess flags. When the Anthropic
//! Agent SDK HTTP endpoints become public, a second [`DispatchEngine`]
//! implementation will use this same config to establish direct HTTP/SSE
//! connections instead of spawning subprocesses.
//!
//! WHY: The existing `HttpEngine` hardcodes `"claude"` as the binary and
//! offers no auth or flag configuration. Energeia needs to be self-sufficient
//! — it must dispatch without kanon in the loop.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::{AgentOptions, DispatchEngine, SessionHandle, SessionSpec};
use crate::error::{self, Result};
use crate::http::session::ProcessSessionHandle;
use crate::http::stream::EventStream;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the Agent SDK execution backend.
///
/// Populated from `aletheia.toml` dispatch section or from hermeneus
/// CC provider config. Owns everything needed to spawn or connect to
/// agent sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentSdkConfig {
    /// Path to the `claude` CLI binary. Resolved from `$PATH` if absent.
    pub cc_binary: Option<PathBuf>,
    /// Default model when the dispatch spec doesn't specify one.
    pub default_model: String,
    /// OAuth token for CC authentication. Set via `CLAUDE_CODE_OAUTH_TOKEN`
    /// env var on the subprocess if present.
    pub oauth_token: Option<String>,
    /// Per-session wall-clock timeout.
    pub timeout: Duration,
    /// Maximum turns per session (default: 200).
    pub max_turns: u32,
    /// Skip CC permission prompts for headless operation.
    pub skip_permissions: bool,
    /// Disable CC plugins for faster startup.
    pub disable_plugins: bool,
    /// Working directory for agent sessions.
    pub default_cwd: Option<PathBuf>,
}

impl Default for AgentSdkConfig {
    fn default() -> Self {
        Self {
            cc_binary: None,
            default_model: "claude-sonnet-4-20250514".to_owned(),
            oauth_token: None,
            timeout: Duration::from_secs(300),
            max_turns: 200,
            skip_permissions: true,
            disable_plugins: true,
            default_cwd: None,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentSdkEngine
// ---------------------------------------------------------------------------

/// [`DispatchEngine`] backed by the Claude Code CLI subprocess.
///
/// Replaces the simpler `HttpEngine` with full CC subprocess configuration:
/// auth tokens, plugin control, permission skipping, and configurable binary
/// path. The [`DispatchEngine`] trait boundary insulates callers so this
/// implementation can be swapped for direct Agent SDK HTTP/SSE when available.
pub struct AgentSdkEngine {
    config: AgentSdkConfig,
    /// Resolved binary path (validated at construction time).
    binary: PathBuf,
}

impl AgentSdkEngine {
    /// Create a new engine from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Engine`] if the CC binary cannot be found.
    pub fn new(config: AgentSdkConfig) -> Result<Self> {
        let binary = if let Some(ref path) = config.cc_binary {
            if path.exists() {
                path.clone()
            } else {
                return Err(error::EngineSnafu {
                    detail: format!(
                        "configured claude CLI path does not exist: {}",
                        path.display()
                    ),
                }
                .build());
            }
        } else {
            which_claude()?
        };

        tracing::info!(
            binary = %binary.display(),
            model = %config.default_model,
            skip_permissions = config.skip_permissions,
            "agent SDK engine initialized"
        );

        Ok(Self { config, binary })
    }

    /// Build the CLI argument vector for a session.
    fn build_args(&self, options: &AgentOptions) -> Vec<String> {
        let mut args = Vec::new();

        args.extend(["--output-format".to_owned(), "stream-json".to_owned()]);
        args.push("--verbose".to_owned());

        let model = options.model.as_deref().unwrap_or(&self.config.default_model);
        args.extend(["--model".to_owned(), model.to_owned()]);

        if self.config.skip_permissions {
            args.push("--dangerously-skip-permissions".to_owned());
        }

        let max_turns = options.max_turns.unwrap_or(self.config.max_turns);
        args.extend(["--max-turns".to_owned(), max_turns.to_string()]);

        if let Some(ref mode) = options.permission_mode {
            args.extend(["--permission-mode".to_owned(), mode.clone()]);
        }

        if let Some(ref prompt) = options.system_prompt {
            args.extend(["--system-prompt".to_owned(), prompt.clone()]);
        }

        // WHY: empty MCP config prevents CC from loading project MCP servers
        // that may not exist in the dispatch worktree.
        args.extend([
            "--strict-mcp-config".to_owned(),
            r#"{"mcpServers":{}}"#.to_owned(),
        ]);

        args
    }

    /// Build environment variables for the subprocess.
    fn build_env(&self) -> Vec<(&str, String)> {
        let mut env = Vec::new();

        if let Some(ref token) = self.config.oauth_token {
            env.push(("CLAUDE_CODE_OAUTH_TOKEN", token.clone()));
        }

        if self.config.disable_plugins {
            env.push(("CLAUDE_CODE_DISABLE_PLUGINS", "1".to_owned()));
        }

        // WHY: CLAUDECODE must be empty string, not unset.
        // The SDK merges env on top of os.environ; leaving it unset
        // causes different behavior than setting it empty.
        env.push(("CLAUDECODE", String::new()));

        env
    }

    /// Spawn a subprocess and return a session handle.
    fn launch(&self, mut cmd: tokio::process::Command) -> Result<ProcessSessionHandle> {
        use std::process::Stdio;

        // Apply environment variables.
        for (key, value) in self.build_env() {
            cmd.env(key, value);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // INVARIANT: kill_on_drop ensures cleanup if the handle is dropped
        // without explicit wait/abort.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            error::EngineSnafu {
                detail: format!("failed to spawn agent subprocess: {e}"),
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

impl DispatchEngine for AgentSdkEngine {
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(&self.binary);
            cmd.args(["-p", &spec.prompt]);
            cmd.args(self.build_args(options));

            // WHY: cwd priority: spec > options > config default.
            if let Some(dir) = spec.cwd.as_ref().or(options.cwd.as_ref()) {
                cmd.current_dir(dir);
            } else if let Some(ref dir) = self.config.default_cwd {
                cmd.current_dir(dir);
            }

            if let Some(ref sys) = spec.system_prompt
                && options.system_prompt.is_none()
            {
                cmd.args(["--system-prompt", sys]);
            }

            tracing::debug!(
                prompt_len = spec.prompt.len(),
                model = options.model.as_deref().unwrap_or(&self.config.default_model),
                "spawning agent session"
            );

            let handle = self.launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }

    fn resume_session<'a>(
        &'a self,
        session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(&self.binary);
            cmd.args(["--resume", session_id, "-p", prompt]);
            cmd.args(self.build_args(options));

            if let Some(ref dir) = options.cwd {
                cmd.current_dir(dir);
            } else if let Some(ref dir) = self.config.default_cwd {
                cmd.current_dir(dir);
            }

            tracing::debug!(session_id, prompt_len = prompt.len(), "resuming agent session");

            let handle = self.launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

/// Locate the `claude` binary on `$PATH` or common install locations.
fn which_claude() -> Result<PathBuf> {
    // WHY: check common locations before falling back to PATH.
    // User-local installs (npm, pip) may not be on PATH in systemd services.
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let candidates = [
        home.as_ref().map(|h| h.join(".local/bin/claude")),
        home.as_ref().map(|h| h.join(".npm-global/bin/claude")),
        Some(PathBuf::from("/usr/local/bin/claude")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Fall back to PATH lookup.
    std::env::var("PATH")
        .ok()
        .and_then(|path| {
            path.split(':')
                .map(|dir| PathBuf::from(dir).join("claude"))
                .find(|p| p.exists())
        })
        .ok_or_else(|| {
            error::EngineSnafu {
                detail: "claude CLI not found in PATH or common locations",
            }
            .build()
        })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = AgentSdkConfig::default();
        assert_eq!(config.default_model, "claude-sonnet-4-20250514");
        assert_eq!(config.max_turns, 200);
        assert!(config.skip_permissions);
        assert!(config.disable_plugins);
        assert!(config.cc_binary.is_none());
    }

    #[test]
    fn build_args_includes_skip_permissions() {
        let config = AgentSdkConfig::default();
        let engine = AgentSdkEngine {
            config,
            binary: PathBuf::from("/usr/bin/claude"),
        };
        let options = AgentOptions::new();
        let args = engine.build_args(&options);
        assert!(args.contains(&"--dangerously-skip-permissions".to_owned()));
        assert!(args.contains(&"--max-turns".to_owned()));
        assert!(args.contains(&"200".to_owned()));
    }

    #[test]
    fn build_env_sets_oauth_and_plugins() {
        let config = AgentSdkConfig {
            oauth_token: Some("test-token".to_owned()),
            disable_plugins: true,
            ..AgentSdkConfig::default()
        };
        let engine = AgentSdkEngine {
            config,
            binary: PathBuf::from("/usr/bin/claude"),
        };
        let env = engine.build_env();
        assert!(env.iter().any(|(k, v)| *k == "CLAUDE_CODE_OAUTH_TOKEN" && v == "test-token"));
        assert!(env.iter().any(|(k, v)| *k == "CLAUDE_CODE_DISABLE_PLUGINS" && v == "1"));
        assert!(env.iter().any(|(k, v)| *k == "CLAUDECODE" && v.is_empty()));
    }

    #[test]
    fn build_args_model_override() {
        let config = AgentSdkConfig::default();
        let engine = AgentSdkEngine {
            config,
            binary: PathBuf::from("/usr/bin/claude"),
        };
        let options = AgentOptions::new().model("claude-opus-4-20250514");
        let args = engine.build_args(&options);
        assert!(args.contains(&"claude-opus-4-20250514".to_owned()));
    }

    #[test]
    fn config_roundtrip() {
        let config = AgentSdkConfig {
            cc_binary: Some(PathBuf::from("/opt/claude")),
            default_model: "test-model".to_owned(),
            oauth_token: Some("token".to_owned()),
            timeout: Duration::from_secs(600),
            max_turns: 100,
            skip_permissions: false,
            disable_plugins: false,
            default_cwd: Some(PathBuf::from("/tmp/work")),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AgentSdkConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.default_model, "test-model");
        assert_eq!(deserialized.max_turns, 100);
    }
}
