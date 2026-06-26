//! Session manager: creates, finds, and manages agent sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use koina::ulid::Ulid;
use organon::receipts::{ReceiptLedger, ReceiptSigner};
use organon::types::ToolGroupId;
use tracing::{info, instrument};

use mneme::types::parse_session_or_agent_id;

use crate::config::NousConfig;

/// Active session state held in memory.
#[derive(Debug, Clone)]
#[expect(
    missing_docs,
    reason = "session state fields are self-documenting by name"
)]
pub struct SessionState {
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub id: String,
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub nous_id: String,
    pub session_key: String, // kanon:ignore RUST/plain-string-secret

    /// Configured default model for this session.
    pub model: String,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,

    pub turn: u64,
    /// Generated fresh on every [`next_turn`](Self::next_turn) call.
    /// Used by the finalize stage as a globally unique dedup key.
    pub turn_id: Ulid,
    pub token_estimate: i64,
    pub cumulative_tokens: u64,
    pub distillation_count: u32,
    pub bootstrap_hash: Option<String>,
    /// Last time the session was accessed. Used for LRU eviction.
    pub last_accessed: Instant,
    /// Consecutive turns with no tool calls (global no-progress counter).
    pub consecutive_no_progress_count: u32,
    /// Per-tool-group consecutive mistake counters.
    pub consecutive_mistake_counts: HashMap<ToolGroupId, u32>,
    /// Whether the consecutive-mistake brake is currently tripped for this session.
    pub brake_tripped: bool,
    /// Per-session ephemeral HMAC-SHA256 signer for tool-call receipts.
    pub receipt_signer: ReceiptSigner,
    /// Per-session in-memory ledger of all emitted tool receipts.
    pub receipt_ledger: Arc<Mutex<ReceiptLedger>>,
    /// Extended loop detector: doom-loop, ping-pong, and no-progress.
    ///
    /// WHY: persisted per-session so patterns are tracked across turns.
    /// Reset on operator intervention via `reset_on_user_message`.
    pub loop_guard: hermeneus::loop_detector::LoopGuard,
    /// Running Bayesian-surprise distribution for this session.
    ///
    /// WHY: surprise is episodic — the prior must accumulate across turns to
    /// detect topic shifts. Advanced once per turn (actor-side, before the
    /// pipeline spawns) with the user content, then read in recall scoring to
    /// rank candidates by how much they diverge from the session topic. Inert
    /// unless `recall.surprise_weight > 0`.
    pub surprise_calculator: mneme::surprise::SurpriseCalculator,
}

impl SessionState {
    /// Create a new session state from config.
    ///
    /// This constructor performs no reserved-prefix validation. It is the
    /// internal bypass for callers that legitimately mint internal keys such
    /// as `cross:`. User-facing creation must go through [`Self::try_new`] or
    /// [`SessionManager::create_session`].
    #[must_use]
    pub fn new(id: String, session_key: String, config: &NousConfig) -> Self {
        Self {
            id,
            nous_id: config.id.to_string(),
            session_key,
            model: config.generation.model.clone(),
            turn: 0,
            turn_id: Ulid::new(),
            token_estimate: 0,
            distillation_count: 0,
            thinking_enabled: config.generation.thinking_enabled,
            thinking_budget: config.generation.thinking_budget,
            bootstrap_hash: None,
            cumulative_tokens: 0,
            last_accessed: Instant::now(),
            consecutive_no_progress_count: 0,
            consecutive_mistake_counts: HashMap::new(),
            brake_tripped: false,
            receipt_signer: ReceiptSigner::new_session(),
            receipt_ledger: Arc::new(Mutex::new(ReceiptLedger::default())),
            loop_guard: hermeneus::loop_detector::LoopGuard::new(),
            surprise_calculator: mneme::surprise::SurpriseCalculator::with_alpha(
                config.recall.surprise_ema_alpha,
            ),
        }
    }

    /// Create a new session state from config, validating that the supplied
    /// `id` and `session_key` do not use reserved internal prefixes.
    ///
    /// # Errors
    ///
    /// Returns [`mneme::types::ReservedIdPrefixError`] when `id` or
    /// `session_key` starts with a reserved prefix such as `cross:`.
    pub fn try_new(
        id: String,
        session_key: String,
        config: &NousConfig,
    ) -> Result<Self, mneme::types::ReservedIdPrefixError> {
        parse_session_or_agent_id(&id)?;
        parse_session_or_agent_id(&session_key)?;
        Ok(Self::new(id, session_key, config))
    }

    /// Internal bypass constructor for reserved session keys.
    ///
    /// WHY: cross-nous coordination mints `cross:`-prefixed session keys. Those
    /// keys must not be constructible from ordinary user creation paths, so
    /// this constructor is explicit and named `internal`.
    #[must_use]
    pub fn new_internal(id: String, session_key: String, config: &NousConfig) -> Self {
        Self::new(id, session_key, config)
    }

    /// Advance to the next turn.
    ///
    /// Generates a fresh [`Ulid`] as `turn_id` so each invocation has a
    /// globally unique dedup key, even after actor restarts with session
    /// adoption from the database.
    pub fn next_turn(&mut self) -> u64 {
        self.turn += 1;
        self.turn_id = Ulid::new();
        // WHY: update last_accessed so LRU eviction correctly keeps active sessions
        // over idle ones. Without this, eviction is effectively creation-time ordering
        // because Instant::now() is only set in new(). (#3253)
        self.last_accessed = Instant::now();
        self.turn
    }

    /// Revert the most recent [`next_turn`](Self::next_turn) call.
    ///
    /// WHY: when a turn is cancelled before the pipeline task runs to
    /// completion, the actor's in-memory counter must be rolled back so the
    /// next successful turn does not leave a visible gap in turn numbering.
    /// The `turn_id` is intentionally not restored to a previous ULID;
    /// `next_turn()` always generates a fresh dedup key, so the cancelled ID
    /// is never reused.
    ///
    /// # Panics
    ///
    /// Debug builds panic when `turn == 0`, which indicates a caller bug
    /// because `revert_turn` is only valid immediately after `next_turn`.
    pub fn revert_turn(&mut self) -> u64 {
        // WHY: revert_turn is only valid immediately after next_turn();
        // underflow means the caller's cancellation logic is out of sync with
        // turn advancement.
        debug_assert!(self.turn > 0, "revert_turn called before next_turn");
        self.turn = self.turn.saturating_sub(1);
        self.turn
    }

    /// Check if context is nearing capacity.
    #[must_use]
    pub fn needs_distillation(&self, threshold_ratio: f64, context_window: u32) -> bool {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "f64→i64: threshold product fits in i64"
        )]
        let threshold = (f64::from(context_window) * threshold_ratio) as i64; // kanon:ignore RUST/as-cast
        self.token_estimate >= threshold
    }
}

/// The session manager: coordinates session lifecycle.
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
    ///
    /// # Errors
    ///
    /// Returns [`mneme::types::ReservedIdPrefixError`] when `id` or
    /// `session_key` uses a reserved internal prefix such as `cross:`.
    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        id: &str,
        session_key: &str,
    ) -> Result<SessionState, mneme::types::ReservedIdPrefixError> {
        info!(
            id,
            session_key,
            nous_id = self.config.id.as_ref(),
            "creating session"
        );
        SessionState::try_new(id.to_owned(), session_key.to_owned(), &self.config)
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
    use std::sync::Arc;

    use super::*;

    fn make_config() -> NousConfig {
        NousConfig {
            id: Arc::from("syn"),
            ..NousConfig::default()
        }
    }

    #[test]
    fn create_session_state() {
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        assert_eq!(state.nous_id, "syn");
        assert_eq!(state.turn, 0);
        assert_eq!(state.token_estimate, 0);
    }

    #[test]
    fn next_turn_increments() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        assert_eq!(state.next_turn(), 1);
        assert_eq!(state.next_turn(), 2);
        assert_eq!(state.next_turn(), 3);
    }

    #[test]
    fn revert_turn_rolls_back_increment() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        let initial_turn_id = state.turn_id;
        assert_eq!(state.next_turn(), 1);
        assert_eq!(state.revert_turn(), 0);
        assert_eq!(state.turn, 0);
        // WHY: next_turn() always mints a fresh turn_id, so the cancelled
        // turn's id is discarded; we only assert the counter is restored.
        assert_ne!(state.turn_id, initial_turn_id);
        assert_eq!(state.next_turn(), 1);
    }

    #[test]
    fn distillation_threshold() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        assert!(!state.needs_distillation(0.9, 200_000)); // 0 tokens

        state.token_estimate = 180_001;
        assert!(state.needs_distillation(0.9, 200_000)); // over 90%

        state.token_estimate = 179_999;
        assert!(!state.needs_distillation(0.9, 200_000)); // under 90%
    }

    #[test]
    fn session_manager_creates() {
        let mgr = SessionManager::new(make_config());
        let Ok(state) = mgr.create_session("ses-1", "main") else {
            panic!("valid session should be created");
        };
        assert_eq!(state.id, "ses-1");
        assert_eq!(state.nous_id, "syn");
    }

    #[test]
    fn create_session_rejects_reserved_cross_prefix() {
        let mgr = SessionManager::new(make_config());
        assert!(
            mgr.create_session("cross:alice", "main").is_err(),
            "user-supplied ids must not use the cross: namespace"
        );
        assert!(
            mgr.create_session("ses-1", "cross:alice").is_err(),
            "user-supplied session keys must not use the cross: namespace"
        );
    }

    #[test]
    fn internal_cross_session_key_minting_works() {
        let config = make_config();
        let state =
            SessionState::new_internal("ses-cross-1".to_owned(), "cross:alice".to_owned(), &config);
        assert_eq!(state.session_key, "cross:alice");
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

    #[test]
    fn distillation_exact_threshold() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        state.token_estimate = 180_000;
        assert!(state.needs_distillation(0.9, 200_000));
    }

    #[test]
    fn distillation_zero_ratio_always_triggers() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        state.token_estimate = 1;
        assert!(state.needs_distillation(0.0, 200_000));
    }

    #[test]
    fn ephemeral_empty_string() {
        assert!(!SessionManager::is_ephemeral(""));
    }

    #[test]
    fn ephemeral_prefix_substring_not_matched() {
        assert!(!SessionManager::is_ephemeral("asking"));
        assert!(!SessionManager::is_ephemeral("spawning"));
        assert!(!SessionManager::is_ephemeral("dispatch"));
    }

    #[test]
    fn next_turn_monotonic() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        let mut prev = 0;
        for _ in 0..20 {
            let next = state.next_turn();
            assert!(next > prev);
            prev = next;
        }
    }

    #[test]
    fn session_state_initial_values() {
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        assert_eq!(state.id, "ses-1");
        assert_eq!(state.session_key, "main");
        assert_eq!(state.distillation_count, 0);
        assert!(state.bootstrap_hash.is_none());
    }

    #[test]
    fn distillation_ratio_one_always_triggers_with_tokens() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        state.token_estimate = 200_000;
        assert!(state.needs_distillation(1.0, 200_000));
    }

    #[test]
    fn distillation_zero_tokens_never_triggers() {
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        assert!(!state.needs_distillation(0.5, 200_000));
    }

    #[test]
    fn distillation_negative_tokens() {
        let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &make_config());
        state.token_estimate = -100;
        assert!(!state.needs_distillation(0.9, 200_000));
    }

    #[test]
    fn session_manager_config_accessor() {
        let config = make_config();
        let mgr = SessionManager::new(config);
        assert_eq!(mgr.config().id.as_ref(), "syn");
    }

    #[test]
    fn session_state_model_from_config() {
        let mut config = make_config();
        config.generation.model = "claude-haiku-4-5-20251001".to_owned();
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
        assert_eq!(state.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn session_state_thinking_from_config() {
        let mut config = make_config();
        config.generation.thinking_enabled = true;
        config.generation.thinking_budget = 5_000;
        let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
        assert!(state.thinking_enabled);
        assert_eq!(state.thinking_budget, 5_000);
    }

    #[test]
    fn ephemeral_case_sensitivity() {
        assert!(!SessionManager::is_ephemeral("Ask:something"));
        assert!(!SessionManager::is_ephemeral("Spawn:worker"));
    }

    #[test]
    fn background_empty_string() {
        assert!(!SessionManager::is_background(""));
    }

    #[test]
    fn background_substring_matches() {
        assert!(SessionManager::is_background("custom-prosoche-wake"));
        assert!(SessionManager::is_background("prefix:prosoche:suffix"));
    }
}
