//! Distillation wiring: trigger logic and orchestration.

use snafu::ResultExt;
use tracing::{info, instrument};

/// Context token count that unconditionally triggers distillation.
const CONTEXT_TOKEN_TRIGGER: u64 = 120_000;

/// Message count that unconditionally triggers distillation.
const MESSAGE_COUNT_TRIGGER: i64 = 150;

/// Days since last distillation before a session is considered stale.
const STALE_SESSION_DAYS: i64 = 7;

/// Minimum message count required for the stale-session trigger to fire.
const STALE_SESSION_MIN_MESSAGES: i64 = 20;

/// Message count that triggers distillation when a session has never been distilled.
const NEVER_DISTILLED_MESSAGE_TRIGGER: i64 = 30;

/// Minimum message count required for the legacy ratio-based trigger to fire.
const LEGACY_THRESHOLD_MIN_MESSAGES: i64 = 10;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{Content, Message as HermeneusMessage, Role as HermeneusRole};
use aletheia_melete::distill::{DistillConfig, DistillEngine, DistillResult};
use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::{Role as MnemeRole, Session, SessionType};
#[cfg(test)]
use aletheia_mneme::types::{SessionMetrics, SessionOrigin};

use crate::error;

/// Configuration for distillation triggers.
#[derive(Debug, Clone)]
pub struct DistillTriggerConfig {
    /// Fraction of context window that triggers legacy threshold (default 0.7).
    pub max_history_share: f64,
    /// Model to use for distillation.
    pub model: String,
    /// Messages to preserve verbatim at the tail (default 3).
    pub verbatim_tail: usize,
}

impl Default for DistillTriggerConfig {
    fn default() -> Self {
        Self {
            max_history_share: 0.7,
            model: "claude-sonnet-4-20250514".to_owned(),
            verbatim_tail: 3,
        }
    }
}

/// Check if a session needs distillation. Returns the trigger reason if so.
#[must_use]
pub fn should_trigger_distillation(
    session: &Session,
    context_window: u64,
    config: &DistillTriggerConfig,
) -> Option<String> {
    // WHY: never distill on the first turn: no history to summarize
    if session.metrics.message_count <= 0 {
        return None;
    }

    // WHY: ephemeral sessions (ask:, spawn:, dispatch:) are short-lived and discarded;
    // distilling them wastes LLM calls on context that will never be resumed.
    if session.session_type == SessionType::Ephemeral {
        tracing::debug!(
            session_id = %session.id,
            session_key = %session.session_key,
            "skipping distillation for ephemeral session"
        );
        return None;
    }

    // NOTE: prefer actual context from last input; fall back to estimate
    let actual_context = if session.metrics.last_input_tokens > 0 {
        session.metrics.last_input_tokens
    } else {
        session.metrics.token_count_estimate
    };

    #[expect(
        clippy::cast_sign_loss,
        reason = "token counts are non-negative in practice"
    )]
    let actual_context_u64 = actual_context as u64;

    if actual_context_u64 >= CONTEXT_TOKEN_TRIGGER {
        return Some(format!("context={actual_context} >= 120K"));
    }

    if session.metrics.message_count >= MESSAGE_COUNT_TRIGGER {
        return Some(format!(
            "message_count={} >= {MESSAGE_COUNT_TRIGGER}",
            session.metrics.message_count
        ));
    }

    if let Some(ref last) = session.metrics.last_distilled_at
        && let Ok(last_ts) = last.parse::<jiff::Timestamp>()
    {
        let age = jiff::Timestamp::now().duration_since(last_ts);
        let days = age.as_secs() / 86_400;
        if days >= STALE_SESSION_DAYS && session.metrics.message_count >= STALE_SESSION_MIN_MESSAGES
        {
            return Some(format!(
                "stale ({days}d) + {} msgs",
                session.metrics.message_count
            ));
        }
    }

    if session.metrics.distillation_count == 0
        && session.metrics.message_count >= NEVER_DISTILLED_MESSAGE_TRIGGER
    {
        return Some(format!(
            "never distilled + {} msgs",
            session.metrics.message_count
        ));
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "context_window * ratio is a rough threshold; precision/truncation acceptable"
    )]
    let threshold = (context_window as f64 * config.max_history_share) as u64;
    if actual_context_u64 >= threshold
        && session.metrics.message_count >= LEGACY_THRESHOLD_MIN_MESSAGES
    {
        return Some(format!(
            "legacy threshold ({actual_context} >= {threshold})"
        ));
    }

    None
}

/// Check if session needs distillation, run it if so.
///
/// Returns `Some(result)` if distillation ran, `None` if not needed.
#[instrument(skip(session_store, provider, config))]
pub async fn maybe_distill(
    session_store: &SessionStore,
    provider: &dyn LlmProvider,
    session_id: &str,
    nous_id: &str,
    context_window: u64,
    config: &DistillTriggerConfig,
) -> error::Result<Option<DistillResult>> {
    let Some(session) = session_store
        .find_session_by_id(session_id)
        .context(error::StoreSnafu)?
    else {
        return Ok(None);
    };

    let Some(trigger) = should_trigger_distillation(&session, context_window, config) else {
        return Ok(None);
    };

    // INVARIANT: idempotency guard: skip if distillation applied recently (< 60s) to protect against concurrent background tasks
    if let Some(ref last) = session.metrics.last_distilled_at
        && let Ok(last_ts) = last.parse::<jiff::Timestamp>()
    {
        let age_secs = jiff::Timestamp::now().duration_since(last_ts).as_secs();
        if age_secs < 60 {
            tracing::debug!(
                %session_id, age_secs,
                "distillation skipped: already distilled within 60s"
            );
            return Ok(None);
        }
    }

    info!(%session_id, %nous_id, %trigger, "triggering distillation");

    let history = session_store
        .get_history(session_id, None)
        .context(error::StoreSnafu)?;

    if history.is_empty() {
        return Ok(None);
    }

    let messages = convert_to_hermeneus_messages(&history);

    let engine = DistillEngine::new(DistillConfig {
        model: config.model.clone(),
        verbatim_tail: config.verbatim_tail,
        ..Default::default()
    });

    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "distillation_count is small non-negative"
    )]
    let distill_count = session.metrics.distillation_count as u32;
    let result = engine
        .distill(&messages, nous_id, provider, distill_count + 1)
        .await
        .context(error::DistillationSnafu)?;

    apply_distillation(session_store, session_id, &result, &history)?;

    info!(
        %session_id,
        messages_distilled = result.messages_distilled,
        tokens_before = result.tokens_before,
        tokens_after = result.tokens_after,
        "distillation complete"
    );

    Ok(Some(result))
}

/// Apply distillation result to the session store.
pub fn apply_distillation(
    store: &SessionStore,
    session_id: &str,
    result: &DistillResult,
    history: &[aletheia_mneme::types::Message],
) -> error::Result<()> {
    let distill_count = result.messages_distilled.min(history.len());
    let seqs: Vec<i64> = history[..distill_count].iter().map(|m| m.seq).collect();

    store
        .mark_messages_distilled(session_id, &seqs)
        .context(error::StoreSnafu)?;

    let summary_content = format!(
        "[Distillation #{}]\n\n{}",
        result.distillation_number, result.summary
    );
    store
        .insert_distillation_summary(session_id, &summary_content)
        .context(error::StoreSnafu)?;

    #[expect(clippy::cast_possible_wrap, reason = "token/message counts fit in i64")]
    store
        .record_distillation(
            session_id,
            distill_count as i64,
            (history.len() - distill_count) as i64,
            result.tokens_before as i64,
            result.tokens_after as i64,
            None,
        )
        .context(error::StoreSnafu)?;

    Ok(())
}

/// Convert mneme messages to hermeneus messages for the distillation engine.
pub fn convert_to_hermeneus_messages(
    history: &[aletheia_mneme::types::Message],
) -> Vec<HermeneusMessage> {
    history
        .iter()
        .map(|msg| {
            let role = match msg.role {
                MnemeRole::System => HermeneusRole::System,
                MnemeRole::User | MnemeRole::ToolResult => HermeneusRole::User,
                MnemeRole::Assistant => HermeneusRole::Assistant,
            };
            HermeneusMessage {
                role,
                content: Content::Text(msg.content.clone()),
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn test_session(overrides: impl FnOnce(&mut Session)) -> Session {
        let mut session = Session {
            id: "ses-1".to_owned(),
            nous_id: "test-nous".to_owned(),
            session_key: "main".to_owned(),
            status: aletheia_mneme::types::SessionStatus::Active,
            model: Some("test-model".to_owned()),
            session_type: aletheia_mneme::types::SessionType::Primary,
            created_at: String::new(),
            updated_at: String::new(),
            metrics: SessionMetrics {
                token_count_estimate: 1000,
                message_count: 10,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
        };
        overrides(&mut session);
        session
    }

    #[test]
    fn trigger_on_high_context() {
        let session = test_session(|s| {
            s.metrics.last_input_tokens = 130_000;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("120K"));
    }

    #[test]
    fn trigger_on_message_count() {
        let session = test_session(|s| {
            s.metrics.message_count = 160;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("150"));
    }

    #[test]
    fn trigger_on_never_distilled() {
        let session = test_session(|s| {
            s.metrics.message_count = 35;
            s.metrics.distillation_count = 0;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("never distilled"));
    }

    #[test]
    fn trigger_on_stale_session() {
        let eight_days_ago = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(8 * 24))
            .expect("valid timestamp");
        let session = test_session(|s| {
            s.metrics.message_count = 25;
            s.metrics.distillation_count = 1;
            s.metrics.last_distilled_at = Some(eight_days_ago.to_string());
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("stale"));
    }

    #[test]
    fn no_trigger_ephemeral_session() {
        let session = test_session(|s| {
            s.session_type = aletheia_mneme::types::SessionType::Ephemeral;
            s.session_key = "ask:demiurge".to_owned();
            s.metrics.last_input_tokens = 130_000;
            s.metrics.message_count = 200;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(
            result.is_none(),
            "ephemeral sessions must never trigger distillation"
        );
    }

    #[test]
    fn trigger_on_non_ephemeral_session_with_same_thresholds() {
        let session = test_session(|s| {
            s.session_type = aletheia_mneme::types::SessionType::Primary;
            s.metrics.last_input_tokens = 130_000;
            s.metrics.message_count = 200;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(
            result.is_some(),
            "non-ephemeral sessions must still trigger distillation"
        );
    }

    #[test]
    fn no_trigger_below_thresholds() {
        let session = test_session(|s| {
            s.metrics.message_count = 5;
            s.metrics.token_count_estimate = 1000;
            s.metrics.distillation_count = 0;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_none());
    }

    #[test]
    fn no_trigger_first_turn() {
        let session = test_session(|s| {
            s.metrics.message_count = 0;
            s.metrics.token_count_estimate = 200_000;
            s.metrics.last_input_tokens = 200_000;
        });
        let config = DistillTriggerConfig::default();
        let result = should_trigger_distillation(&session, 200_000, &config);
        assert!(result.is_none());
    }

    #[test]
    fn trigger_on_legacy_threshold() {
        let session = test_session(|s| {
            s.metrics.message_count = 15;
            s.metrics.token_count_estimate = 100_000;
            s.metrics.distillation_count = 1;
        });
        let config = DistillTriggerConfig {
            max_history_share: 0.7,
            ..DistillTriggerConfig::default()
        };
        // NOTE: context_window=140_000 * 0.7 = 98_000, actual=100_000 >= 98_000
        let result = should_trigger_distillation(&session, 140_000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("legacy threshold"));
    }

    /// Verify that `apply_distillation` writes a summary and marks messages distilled in the store.
    ///
    /// This exercises the trigger path that fires after `should_trigger_distillation` returns
    /// Some: confirming that the store mutation side-effects actually occur.
    #[test]
    fn apply_distillation_updates_store() {
        use aletheia_melete::distill::DistillResult;
        use aletheia_mneme::store::SessionStore;
        use aletheia_mneme::types::Role as MnemeRole;

        let store = SessionStore::open_in_memory().expect("in-memory store");
        store
            .create_session("ses-1", "agent-1", "main", None, None)
            .expect("create session");

        for i in 0..5_i64 {
            store
                .append_message(
                    "ses-1",
                    MnemeRole::User,
                    &format!("turn {i}"),
                    None,
                    None,
                    100,
                )
                .expect("append");
        }

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 5);

        // WHY: Distill all 5 messages: avoids the seq-shift conflict that occurs when
        // undistilled messages have adjacent seq numbers after partial distillation.
        let result = DistillResult {
            summary: "Summary of previous turns.".to_owned(),
            messages_distilled: history.len(),
            tokens_before: 500,
            tokens_after: 120,
            distillation_number: 1,
            timestamp: jiff::Timestamp::now().to_string(),
            verbatim_messages: vec![],
            memory_flush: aletheia_melete::flush::MemoryFlush {
                decisions: vec![],
                corrections: vec![],
                facts: vec![],
                task_state: None,
            },
        };

        apply_distillation(&store, "ses-1", &result, &history).expect("apply distillation");

        let history_after = store.get_history("ses-1", None).expect("history after");
        let has_summary = history_after
            .iter()
            .any(|m| m.content.contains("[Distillation #1]"));
        assert!(has_summary, "distillation summary message must be present");

        let session = store
            .find_session_by_id("ses-1")
            .expect("find session")
            .expect("session exists");
        assert_eq!(
            session.metrics.distillation_count, 1,
            "distillation_count should be incremented"
        );
    }

    #[test]
    fn message_conversion_maps_roles() {
        let messages = vec![
            aletheia_mneme::types::Message {
                id: 1,
                session_id: "s".to_owned(),
                seq: 1,
                role: MnemeRole::System,
                content: "system".to_owned(),
                tool_call_id: None,
                tool_name: None,
                token_estimate: 0,
                is_distilled: false,
                created_at: String::new(),
            },
            aletheia_mneme::types::Message {
                id: 2,
                session_id: "s".to_owned(),
                seq: 2,
                role: MnemeRole::User,
                content: "user".to_owned(),
                tool_call_id: None,
                tool_name: None,
                token_estimate: 0,
                is_distilled: false,
                created_at: String::new(),
            },
            aletheia_mneme::types::Message {
                id: 3,
                session_id: "s".to_owned(),
                seq: 3,
                role: MnemeRole::ToolResult,
                content: "tool output".to_owned(),
                tool_call_id: Some("tc-1".to_owned()),
                tool_name: Some("read".to_owned()),
                token_estimate: 0,
                is_distilled: false,
                created_at: String::new(),
            },
        ];
        let converted = convert_to_hermeneus_messages(&messages);
        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0].role, HermeneusRole::System);
        assert_eq!(converted[1].role, HermeneusRole::User);
        assert_eq!(converted[2].role, HermeneusRole::User);
    }
}
