// WHY: Wraps AgentOptions into session-level configuration with additional
// fields needed by the session manager (idle timeout, additional directories).
// Separates session config concerns from the raw engine options.

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::engine::AgentOptions;
use crate::routing::DispatchRoutingConfig;
use crate::types::SessionStatus;

/// Progress bridge event for a child prompt session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct ChildSessionProgress {
    /// Prompt number that owns the child session.
    pub prompt_number: u32,
    /// Current child-session lifecycle state.
    pub status: ChildSessionProgressStatus,
    /// Agent SDK session identifier.
    // kanon:ignore RUST/primitive-for-domain-id — public progress-reporting type; changing to newtype would be a breaking API change across crates
    pub child_session_id: String,
    /// Bounded text excerpt observed from the child session, when available.
    pub output_excerpt: Option<String>,
}

/// Lifecycle state reported by [`ChildSessionProgress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ChildSessionProgressStatus {
    /// A child session has been spawned.
    Started,
    /// A child session reached a terminal energeia session status.
    Finished(SessionStatus),
}

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
    pub(crate) routing: DispatchRoutingConfig,
    pub(crate) after_action_log_dir: Option<PathBuf>,
    /// Cancellation token shared by the dispatch group.
    pub cancel: Option<CancellationToken>,
    /// Optional parent bridge for child-session progress events.
    pub child_progress_tx: Option<mpsc::UnboundedSender<ChildSessionProgress>>,
}

// NOTE: Builder methods stay `pub` for external callers and test fixtures;
// the internal orchestrator uses defaults.
impl EngineConfig {
    /// Start building an `EngineConfig` from base options.
    #[must_use]
    pub fn new(options: AgentOptions) -> Self {
        Self {
            options,
            additional_dirs: Vec::new(),
            idle_timeout: None,
            routing: DispatchRoutingConfig::default(),
            after_action_log_dir: None,
            cancel: None,
            child_progress_tx: None,
        }
    }

    /// Set the LLM model identifier.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.options.model = Some(model.into());
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.options.system_prompt = Some(prompt.into());
        self
    }

    /// Set the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        let path: PathBuf = cwd.into();
        self.options.cwd = Some(path.to_string_lossy().into_owned());
        self
    }

    /// Set the maximum turn count for a single session stage.
    #[must_use]
    pub fn max_turns(mut self, turns: u32) -> Self {
        self.options.max_turns = Some(turns);
        self
    }

    /// Set the permission mode (e.g., "plan", "auto", "bypass").
    #[must_use]
    pub fn permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.options.permission_mode = Some(mode.into());
        self
    }

    /// Add an additional directory the agent should have access to.
    #[must_use]
    pub fn add_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.additional_dirs.push(dir.into());
        self
    }

    /// Set the idle timeout for event stream monitoring.
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    #[must_use]
    pub(crate) fn routing(mut self, routing: DispatchRoutingConfig) -> Self {
        self.routing = routing;
        self
    }

    #[must_use]
    pub(crate) fn after_action_log_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.after_action_log_dir = dir;
        self
    }

    /// Set the cancellation token for this session.
    #[must_use]
    pub fn cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Set the parent bridge for child-session progress events.
    #[must_use]
    pub fn child_progress_tx(mut self, tx: mpsc::UnboundedSender<ChildSessionProgress>) -> Self {
        self.child_progress_tx = Some(tx);
        self
    }

    /// Extract the inner [`AgentOptions`] for passing to the engine.
    #[must_use]
    pub fn to_agent_options(&self) -> AgentOptions {
        let mut options = self.options.clone();
        options.additional_dirs.clone_from(&self.additional_dirs);
        options
    }

    /// Create a copy of the inner options with a different `max_turns` value.
    #[must_use]
    pub fn options_with_turns(&self, turns: u32) -> AgentOptions {
        let mut opts = self.options.clone();
        opts.max_turns = Some(turns);
        opts.additional_dirs.clone_from(&self.additional_dirs);
        opts
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new(AgentOptions::default())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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
            .idle_timeout(Duration::from_mins(5));

        assert_eq!(
            config.options.model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(config.options.cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(config.options.max_turns, Some(50));
        assert_eq!(config.options.permission_mode.as_deref(), Some("plan"));
        assert_eq!(config.additional_dirs.len(), 1);
        assert!(config.after_action_log_dir.is_none());
        assert_eq!(
            config.additional_dirs.first(),
            Some(&PathBuf::from("/tmp/shared"))
        );
        assert_eq!(config.idle_timeout, Some(Duration::from_mins(5)));
        assert!(config.child_progress_tx.is_none());
    }

    #[test]
    fn engine_config_default() {
        let config = EngineConfig::default();
        assert!(config.options.model.is_none());
        assert!(config.additional_dirs.is_empty());
        assert!(config.idle_timeout.is_none());
        assert!(config.after_action_log_dir.is_none());
        assert!(config.child_progress_tx.is_none());
    }

    #[test]
    fn child_progress_tx_builder_sets_bridge() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let config = EngineConfig::default().child_progress_tx(tx);
        assert!(config.child_progress_tx.is_some());
    }

    #[test]
    fn child_progress_serializes_as_structured_record() {
        let progress = ChildSessionProgress {
            prompt_number: 7,
            status: ChildSessionProgressStatus::Finished(SessionStatus::Success),
            child_session_id: "sess-7".to_owned(),
            output_excerpt: Some("done".to_owned()),
        };

        let json = serde_json::to_string(&progress).expect("serialize child progress");
        let decoded: ChildSessionProgress =
            serde_json::from_str(&json).expect("deserialize child progress");

        assert_eq!(decoded, progress);
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
