//! Session manager — creates, finds, and manages agent sessions.

use tracing::{info, instrument};

use crate::config::NousConfig;

/// Active session state held in memory.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Session ID.
    pub id: String,
    /// Agent ID.
    pub nous_id: String,
    /// Session key.
    pub session_key: String,
    /// Current model.
    pub model: String,
    /// Turn counter (sequential within session).
    pub turn: u64,
    /// Running token estimate.
    pub token_estimate: i64,
    /// Number of distillations performed.
    pub distillation_count: u32,
    /// Whether thinking is enabled.
    pub thinking_enabled: bool,
    /// Thinking token budget.
    pub thinking_budget: u32,
    /// Bootstrap context hash (for cache invalidation).
    pub bootstrap_hash: Option<String>,
}

impl SessionState {
    /// Create a new session state from config.
    #[must_use]
    pub fn new(id: String, session_key: String, config: &NousConfig) -> Self {
        Self {
            id,
            nous_id: config.id.clone(),
            session_key,
            model: config.model.clone(),
            turn: 0,
            token_estimate: 0,
            distillation_count: 0,
            thinking_enabled: config.thinking_enabled,
            thinking_budget: config.thinking_budget,
            bootstrap_hash: None,
        }
    }

    /// Advance to the next turn.
    pub fn next_turn(&mut self) -> u64 {
        self.turn += 1;
        self.turn
    }

    /// Check if context is nearing capacity.
    #[must_use]
    pub fn needs_distillation(&self, threshold_ratio: f64, context_window: u32) -> bool {
        #[allow(clippy::cast_possible_truncation)]
        let threshold = (f64::from(context_window) * threshold_ratio) as i64;
        self.token_estimate >= threshold
    }
}

/// The session manager — coordinates session lifecycle.
#[derive(Debug)]
pub struct SessionManager {
    config: NousConfig,
}

impl SessionManager {
    /// Create a new session manager.
    #[must_use]
    pub fn new(config: NousConfig) -> Self {
        Self { config }
    }

    /// Create a new session state.
    #[instrument(skip(self))]
    pub fn create_session(&self, id: &str, session_key: &str) -> SessionState {
        info!(id, session_key, nous_id = self.config.id, "creating session");
        SessionState::new(id.to_owned(), session_key.to_owned(), &self.config)
    }

    /// Get the agent configuration.
    #[must_use]
    pub fn config(&self) -> &NousConfig {
        &self.config
    }

    /// Check if a session key indicates an ephemeral session.
    #[must_use]
    pub fn is_ephemeral(session_key: &str) -> bool {
        session_key.starts_with("ask:")
            || session_key.starts_with("spawn:")
            || session_key.starts_with("dispatch:")
            || session_key.starts_with("ephemeral:")
    }

    /// Check if a session key indicates a background session.
    #[must_use]
    pub fn is_background(session_key: &str) -> bool {
        session_key.contains("prosoche")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> NousConfig {
        NousConfig {
            id: "syn".to_owned(),
            ..NousConfig::default()
        }
    }

    #[test]
    fn create_session_state() {
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &test_config());
        assert_eq!(state.nous_id, "syn");
        assert_eq!(state.turn, 0);
        assert_eq!(state.token_estimate, 0);
    }

    #[test]
    fn next_turn_increments() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &test_config());
        assert_eq!(state.next_turn(), 1);
        assert_eq!(state.next_turn(), 2);
        assert_eq!(state.next_turn(), 3);
    }

    #[test]
    fn distillation_threshold() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &test_config());
        assert!(!state.needs_distillation(0.9, 200_000)); // 0 tokens

        state.token_estimate = 180_001;
        assert!(state.needs_distillation(0.9, 200_000)); // over 90%

        state.token_estimate = 179_999;
        assert!(!state.needs_distillation(0.9, 200_000)); // under 90%
    }

    #[test]
    fn session_manager_creates() {
        let mgr = SessionManager::new(test_config());
        let state = mgr.create_session("ses-1", "main");
        assert_eq!(state.id, "ses-1");
        assert_eq!(state.nous_id, "syn");
    }

    #[test]
    fn ephemeral_detection() {
        assert!(SessionManager::is_ephemeral("ask:demiurge"));
        assert!(SessionManager::is_ephemeral("spawn:coder"));
        assert!(SessionManager::is_ephemeral("dispatch:task"));
        assert!(SessionManager::is_ephemeral("ephemeral:one-off"));
        assert!(!SessionManager::is_ephemeral("main"));
        assert!(!SessionManager::is_ephemeral("signal-group"));
    }

    #[test]
    fn background_detection() {
        assert!(SessionManager::is_background("prosoche-wake"));
        assert!(SessionManager::is_background("prosoche"));
        assert!(!SessionManager::is_background("main"));
        assert!(!SessionManager::is_background("ask:syn"));
    }
}
