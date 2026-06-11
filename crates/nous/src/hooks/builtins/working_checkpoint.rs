//! Working checkpoint hook: inject agent-curated `<key_info>` into the system prompt.
//!
//! Runs in `before_query` (before the model call). Reads the most recent
//! checkpoints for the session from [`WorkingCheckpointStore`] and appends them
//! to the system prompt so the agent retains structured scratchpad state
//! across turns.
//!
//! Also runs in `on_turn_complete` to verify persistence and flag mod-N
//! boundaries for identity reinjection on the next turn.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::{debug, warn};

use crate::hooks::{HookResult, QueryContext, TurnContext, TurnHook};

/// Default maximum characters for the injected `<key_info>` + `<history>` block.
const DEFAULT_KEY_INFO_MAX_CHARS: usize = 2000;

/// Default number of historical checkpoints to include after the latest.
const DEFAULT_HISTORY_LIMIT: usize = 3;

/// Injects the latest working checkpoint (and optional history) into the system prompt.
pub(crate) struct WorkingCheckpointInjector {
    store: Option<Arc<dyn organon::types::WorkingCheckpointStore>>,
    /// Set by `on_turn_complete` when a mod-N boundary is crossed; consumed by
    /// the next `before_query` to also inject an identity reminder.
    reinject_next_turn: AtomicBool,
    /// Maximum characters for the injected block before truncation.
    key_info_max_chars: usize,
    /// Number of older checkpoints to include in the `<history>` section.
    history_limit: usize,
    /// Text injected on mod-N boundaries as the identity reminder.
    identity_reminder_text: String,
}

impl WorkingCheckpointInjector {
    /// Create a new injector with the given store.
    ///
    /// When `store` is `None`, the hook is a no-op.
    pub(crate) fn new(store: Option<Arc<dyn organon::types::WorkingCheckpointStore>>) -> Self {
        Self {
            store,
            reinject_next_turn: AtomicBool::new(false),
            key_info_max_chars: DEFAULT_KEY_INFO_MAX_CHARS,
            history_limit: DEFAULT_HISTORY_LIMIT,
            identity_reminder_text:
                "Core identity and pinned facts from the system prompt are preserved across compaction."
                    .to_owned(),
        }
    }
}

impl TurnHook for WorkingCheckpointInjector {
    fn name(&self) -> &'static str {
        "working_checkpoint_injector"
    }

    fn before_query<'a>(
        &'a self,
        context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref store) = self.store else {
                return HookResult::Continue;
            };

            // WHY: read_recent with history_limit+1 so we get the latest for
            // <key_info> plus up to history_limit older entries for <history>.
            let checkpoints =
                match store.read_recent(context.session_id, self.history_limit.saturating_add(1)) {
                    Ok(cps) => cps,
                    Err(e) => {
                        warn!(
                            nous_id = context.nous_id,
                            session_id = context.session_id,
                            error = %e,
                            "working_checkpoint_injector: failed to read recent checkpoints"
                        );
                        return HookResult::Continue;
                    }
                };

            if checkpoints.is_empty() {
                debug!(
                    nous_id = context.nous_id,
                    session_id = context.session_id,
                    turn_number = context.turn_number,
                    "working_checkpoint_injector: no checkpoint for session"
                );
                return HookResult::Continue;
            }

            let mut section = String::new();

            if let Some(latest) = checkpoints.first() {
                section.push_str("<key_info>\n");
                section.push_str(&latest.content);
                section.push_str("\n</key_info>");
            }

            if let Some(rest) = checkpoints.get(1..) {
                section.push_str("\n<history>\n");
                for cp in rest {
                    use std::fmt::Write as _;
                    let _ = writeln!(
                        section,
                        "Turn {turn}: {content}",
                        turn = cp.turn_number,
                        content = cp.content
                    );
                    section.push_str("---\n");
                }
                if section.ends_with("---\n") {
                    section.truncate(section.len().saturating_sub(4));
                }
                section.push_str("\n</history>");
            }

            if section.len() > self.key_info_max_chars {
                section.truncate(self.key_info_max_chars);
                section.push_str("\n...[truncated]");
            }

            if self.reinject_next_turn.swap(false, Ordering::SeqCst) {
                section.push_str("\n<identity_reminder>\n");
                section.push_str(&self.identity_reminder_text);
                section.push_str("\n</identity_reminder>");
            }

            let token_estimate = i64::try_from(section.len() / 4).unwrap_or(i64::MAX);

            if context.pipeline.remaining_tokens < token_estimate * 2 {
                warn!(
                    nous_id = context.nous_id,
                    session_id = context.session_id,
                    remaining = context.pipeline.remaining_tokens,
                    checkpoint_tokens = token_estimate,
                    "working_checkpoint_injector: skipping injection, insufficient token budget"
                );
                return HookResult::Continue;
            }

            if let Some(ref mut prompt) = context.pipeline.system_prompt {
                prompt.push_str("\n\n");
                prompt.push_str(&section);
            }

            context.pipeline.remaining_tokens -= token_estimate;

            debug!(
                nous_id = context.nous_id,
                session_id = context.session_id,
                turn_number = context.turn_number,
                checkpoint_count = checkpoints.len(),
                token_estimate,
                "working_checkpoint_injector: injected checkpoint into system prompt"
            );

            HookResult::Continue
        })
    }

    fn on_turn_complete<'a>(
        &'a self,
        context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            // NOTE: the pipeline signals mod-N boundaries via reinject_identity;
            // the flag defers the reminder to the next turn's injection.
            if context.reinject_identity {
                self.reinject_next_turn.store(true, Ordering::SeqCst);
                debug!(
                    nous_id = context.nous_id,
                    session_id = context.session_id,
                    turn_number = context.turn_number,
                    "working_checkpoint_injector: flagged for identity reinjection next turn"
                );
            }

            if let Some(ref store) = self.store {
                match store.read_latest(context.session_id) {
                    Ok(Some(cp)) if cp.turn_number == context.turn_number => {
                        debug!(
                            nous_id = context.nous_id,
                            session_id = context.session_id,
                            turn_number = context.turn_number,
                            "working_checkpoint_injector: checkpoint verified for turn"
                        );
                    }
                    Ok(Some(_)) => {
                        debug!(
                            nous_id = context.nous_id,
                            session_id = context.session_id,
                            turn_number = context.turn_number,
                            "working_checkpoint_injector: latest checkpoint is from an earlier turn"
                        );
                    }
                    Ok(None) => {
                        debug!(
                            nous_id = context.nous_id,
                            session_id = context.session_id,
                            turn_number = context.turn_number,
                            "working_checkpoint_injector: no checkpoint for this turn"
                        );
                    }
                    Err(e) => {
                        warn!(
                            nous_id = context.nous_id,
                            session_id = context.session_id,
                            error = %e,
                            "working_checkpoint_injector: failed to verify checkpoint"
                        );
                    }
                }
            }

            HookResult::Continue
        })
    }
}

#[cfg(test)]
#[path = "working_checkpoint_tests.rs"]
mod working_checkpoint_tests;
