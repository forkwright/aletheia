// WHY: Wraps AgentOptions into session-level configuration with additional
// fields needed by the session manager (idle timeout, additional directories).
// Separates session config concerns from the raw engine options.

use std::path::PathBuf;
use std::time::Duration;

use crate::engine::AgentOptions;

// ---------------------------------------------------------------------------
// EngineConfig
// ---------------------------------------------------------------------------

/// Session-level configuration that wraps [`AgentOptions`] with additional
/// parameters the session manager needs beyond what the engine consumes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EngineConfig {
    /// Base options passed to the [`DispatchEngine`](crate::engine::DispatchEngine).
    pub options: AgentOptions,
    /// Additional directories the agent can access.
    pub additional_dirs: Vec<PathBuf>,
    /// How long to wait for session events before declaring a timeout.
    /// `None` disables timeout detection.
    pub idle_timeout: Option<Duration>,
}

impl EngineConfig {
    /// Start building an `EngineConfig` from base options.
    #[must_use]
    pub fn new(options: AgentOptions) -> Self {
        Self {
            options,
            additional_dirs: Vec::new(),
            idle_timeout: None,
        }
    }

    /// Set the LLM model identifier.
    // PUBLIC: builder method retained pub for test fixtures that exercise
    // model overrides; internal orchestrator uses defaults.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.options.model = Some(model.into());
        self
    }

    /// Set the system prompt.
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.options.system_prompt = Some(prompt.into());
        self
    }

    /// Set the working directory.
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        let path: PathBuf = cwd.into();
        self.options.cwd = Some(path.to_string_lossy().into_owned());
        self
    }

    /// Set the maximum turn count for a single session stage.
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn max_turns(mut self, turns: u32) -> Self {
        self.options.max_turns = Some(turns);
        self
    }

    /// Set the permission mode (e.g., "plan", "auto", "bypass").
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.options.permission_mode = Some(mode.into());
        self
    }

    /// Add an additional directory the agent should have access to.
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn add_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.additional_dirs.push(dir.into());
        self
    }

    /// Set the idle timeout for event stream monitoring.
    // PUBLIC: builder method retained pub for external and test fixtures.
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    /// Extract the inner [`AgentOptions`] for passing to the engine.
    #[must_use]
    pub fn to_agent_options(&self) -> AgentOptions {
        self.options.clone()
    }

    /// Create a copy of the inner options with a different `max_turns` value.
    #[must_use]
    pub fn options_with_turns(&self, turns: u32) -> AgentOptions {
        let mut opts = self.options.clone();
        opts.max_turns = Some(turns);
        opts
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new(AgentOptions::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_config_builder() {
        let config = EngineConfig::default()
            .model("claude-sonnet-4-20250514")
            .cwd("/tmp/work")
            .max_turns(50)
            .permission_mode("plan")
            .add_dir("/tmp/shared")
            .idle_timeout(Duration::from_secs(300));

        assert_eq!(
            config.options.model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(config.options.cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(config.options.max_turns, Some(50));
        assert_eq!(config.options.permission_mode.as_deref(), Some("plan"));
        assert_eq!(config.additional_dirs.len(), 1);
        assert_eq!(
            config.additional_dirs.first(),
            Some(&PathBuf::from("/tmp/shared"))
        );
        assert_eq!(config.idle_timeout, Some(Duration::from_secs(300)));
    }

    #[test]
    fn engine_config_default() {
        let config = EngineConfig::default();
        assert!(config.options.model.is_none());
        assert!(config.additional_dirs.is_empty());
        assert!(config.idle_timeout.is_none());
    }

    #[test]
    fn to_agent_options_clones_inner() {
        let config = EngineConfig::default().model("test-model").max_turns(10);
        let opts = config.to_agent_options();
        assert_eq!(opts.model.as_deref(), Some("test-model"));
        assert_eq!(opts.max_turns, Some(10));
    }

    #[test]
    fn options_with_turns_overrides_max_turns() {
        let config = EngineConfig::default().max_turns(100);
        let opts = config.options_with_turns(25);
        assert_eq!(opts.max_turns, Some(25));
    }

    #[test]
    fn multiple_add_dir() {
        let config = EngineConfig::default()
            .add_dir("/a")
            .add_dir("/b")
            .add_dir("/c");
        assert_eq!(config.additional_dirs.len(), 3);
    }

    #[test]
    fn system_prompt_builder() {
        let config = EngineConfig::default().system_prompt("you are a coding agent");
        assert_eq!(
            config.options.system_prompt.as_deref(),
            Some("you are a coding agent")
        );
    }
}
