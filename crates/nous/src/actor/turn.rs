//! Turn execution: handles individual turns with panic boundary protection.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use koina::id::{NousId, SessionId};
use organon::surface::SurfaceInputs;
use organon::types::ToolContext;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, warn};

use super::{NousActor, NousLifecycle};
use crate::pipeline::TurnResult;
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

/// Drop guard that drops the streaming sender when the turn completes or is cancelled.
/// Signals the receiver that no more data is coming, preventing hung SSE connections.
struct StreamSenderGuard(Option<mpsc::Sender<TurnStreamEvent>>);

impl Drop for StreamSenderGuard {
    fn drop(&mut self) {
        // WHY: explicitly take and drop the sender so the receiver sees a closed channel
        // even if the turn was cancelled via cancellation token or task abort.
        drop(self.0.take());
    }
}

pub(super) struct StreamingTurnRequest {
    // kanon:ignore RUST/plain-string-secret — session_key is a HashMap lookup identifier (not an auth secret)
    pub session_key: String,
    pub session_id: Option<String>,
    pub content: String,
    pub stream_tx: mpsc::Sender<TurnStreamEvent>,
    /// Operator approval gate for reversibility-class tool calls (#3958).
    pub approval_gate: Option<crate::approval::ApprovalGate>,
    pub caller_span: tracing::Span,
    pub turn_cancel: CancellationToken,
    pub reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
}

impl NousActor {
    /// Wake from dormant if needed, mark the turn active.
    pub(super) fn mark_turn_active(&mut self, session_key: &str) {
        if self.channel.status == NousLifecycle::Dormant {
            debug!("auto-waking from dormant for turn");
            self.channel.status = NousLifecycle::Idle;
        }
        self.channel.status = NousLifecycle::Active;
        self.active_session = Some(session_key.to_owned());
        self.runtime.active_turn.store(true, Ordering::Release);

        // WHY: reset consecutive-mistake brake on operator intervention (new turn).
        // The brake is not a permanent terminator; any new user message clears it.
        if let Some(session) = self.sessions.get_mut(session_key)
            && session.brake_tripped
        {
            info!(session_key = %session_key, "brake reset by operator intervention");
            session.consecutive_no_progress_count = 0;
            session.brake_tripped = false;
            session.loop_guard.reset_on_user_message();
        }
        // WHY: record when the turn started so the health check can detect turns
        // stuck longer than `stuck_turn_timeout_secs`, even when active_turn is
        // true. Uses millis-since-started_at to avoid wrapping issues. (#3254)
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: actor uptime in ms won't exceed u64::MAX"
        )]
        let elapsed_ms = self.runtime.started_at.elapsed().as_millis() as u64; // kanon:ignore RUST/as-cast
        self.runtime
            .turn_started_at_ms
            .store(elapsed_ms, Ordering::Release);
    }

    /// Finalize turn: update session tokens, check drift, spawn side-effects, reset state.
    async fn finalize_turn(
        &mut self,
        session_key: &str,
        content: &str,
        result: &crate::error::Result<TurnResult>,
    ) {
        if let Ok(turn_result) = result {
            if let Some(session) = self.sessions.get_mut(session_key) {
                session.cumulative_tokens = session
                    .cumulative_tokens
                    .saturating_add(turn_result.usage.input_tokens)
                    .saturating_add(turn_result.usage.output_tokens);
            }

            // WHY: drift detection runs after token accounting but before
            // side-effect spawning so drift events are available for logging
            // before any async work begins.
            self.record_drift_metrics(session_key, turn_result);

            // WHY: record the turn outcome in the empirical router so both the
            // dispatch path (energeia) and the interactive path benefit from the
            // same success-rate signal. The success heuristic is: the turn
            // completed without degradation (i.e. the LLM was reachable and
            // returned a result). A more precise signal (e.g. user acceptance)
            // requires a future hook.
            self.record_router_outcome(content, turn_result);

            self.maybe_spawn_extraction(
                content,
                &turn_result.content,
                &turn_result.tool_calls,
                &turn_result.reasoning,
            );
            let source_session_id = self
                .sessions
                .get(session_key)
                .map_or_else(|| session_key.to_owned(), |session| session.id.clone());
            self.maybe_spawn_skill_analysis(&turn_result.tool_calls, &source_session_id);
            self.maybe_spawn_distillation(session_key).await;
            #[cfg(feature = "knowledge-store")]
            self.maybe_run_auto_dream().await;
        }

        self.active_session = None;
        // WHY: only reset to Idle if not degraded: preserve degraded state
        if self.channel.status != NousLifecycle::Degraded {
            self.channel.status = NousLifecycle::Idle;
        }
        self.runtime.active_turn.store(false, Ordering::Release);
        self.runtime.turn_started_at_ms.store(0, Ordering::Release);
    }

    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation. Sets `lifecycle = Active` and
    /// `active_session` before awaiting `execute_turn`. If the future
    /// were dropped mid-await, those fields would not be reset. In
    /// practice this is only called from the sequential actor loop, so
    /// cancellation only occurs at shutdown when the actor is consumed.
    ///
    /// The panic boundary in `execute_turn_with_panic_boundary` ensures
    /// that even if the pipeline panics, the actor remains in a consistent
    /// state and can process subsequent messages.
    pub(super) async fn handle_turn(
        &mut self,
        session_key: String, // kanon:ignore RUST/plain-string-secret
        session_id: Option<String>,
        content: String,
        caller_span: tracing::Span,
        turn_cancel: CancellationToken,
        reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
    ) {
        self.mark_turn_active(&session_key);

        let mut result = self
            .execute_turn_with_panic_boundary(
                &session_key,
                session_id.as_deref(),
                &content,
                caller_span,
                turn_cancel,
            )
            .await;

        self.apply_mistake_brake(&session_key, &mut result);
        self.apply_loop_guard(&session_key, &mut result);
        self.finalize_turn(&session_key, &content, &result).await;

        // WHY: ignore send error: caller may have dropped the receiver
        if let Err(_e) = reply.send(result) {
            debug!("caller dropped the receiver");
        }
    }

    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation: same profile as `handle_turn`.
    /// Sets `lifecycle` and `active_session` before the `.await` point.
    /// Only called from the sequential actor loop.
    pub(super) async fn handle_streaming_turn(&mut self, request: StreamingTurnRequest) {
        let StreamingTurnRequest {
            session_key,
            session_id,
            content,
            stream_tx,
            approval_gate,
            caller_span,
            turn_cancel,
            reply,
        } = request;
        // WHY: wrap sender in a Drop guard so it is dropped (closing the channel) even
        // if the turn is cancelled via cancellation token or panic, preventing hung
        // SSE connections on the receiver side.
        let _stream_guard = StreamSenderGuard(Some(stream_tx.clone()));

        self.mark_turn_active(&session_key);

        let mut result = self
            .execute_streaming_turn_with_panic_boundary(
                &session_key,
                session_id.as_deref(),
                &content,
                &stream_tx,
                approval_gate,
                caller_span,
                turn_cancel,
            )
            .await;

        self.apply_mistake_brake(&session_key, &mut result);
        self.apply_loop_guard(&session_key, &mut result);
        self.finalize_turn(&session_key, &content, &result).await;

        // WHY: ignore send error: caller may have dropped the receiver
        if let Err(_e) = reply.send(result) {
            debug!("caller dropped the receiver");
        }
        // NOTE: _stream_guard drops here, closing the channel if no other senders remain
    }

    /// Execute a turn with a panic boundary. If the pipeline panics, the panic
    /// is caught, logged, and an error is returned to the caller. The actor
    /// continues processing subsequent messages.
    ///
    /// Pipeline panics are isolated to a spawned task so they don't
    /// crash the actor. This is essential for long-running agents where
    /// a single malformed input or tool bug shouldn't terminate the service.
    pub(super) async fn execute_turn_with_panic_boundary(
        &mut self,
        session_key: &str,
        session_id: Option<&str>,
        content: &str,
        caller_span: tracing::Span,
        turn_cancel: CancellationToken,
    ) -> crate::error::Result<TurnResult> {
        // WHY: pipeline spawned in separate task so panics are caught by JoinHandle, not the actor loop
        let result = self
            .spawn_pipeline_task(
                session_key,
                session_id,
                content,
                None,
                None,
                caller_span,
                turn_cancel,
            )
            .await;
        self.handle_pipeline_result(result, session_key)
    }

    /// Execute a streaming turn with a panic boundary.
    #[expect(
        clippy::too_many_arguments,
        reason = "streaming turn entrypoint plumbs session, content, stream, gate, span, and cancel; splitting hides the call shape"
    )]
    async fn execute_streaming_turn_with_panic_boundary(
        &mut self,
        session_key: &str,
        session_id: Option<&str>,
        content: &str,
        stream_tx: &mpsc::Sender<TurnStreamEvent>,
        approval_gate: Option<crate::approval::ApprovalGate>,
        caller_span: tracing::Span,
        turn_cancel: CancellationToken,
    ) -> crate::error::Result<TurnResult> {
        let result = self
            .spawn_pipeline_task(
                session_key,
                session_id,
                content,
                Some(stream_tx.clone()),
                approval_gate,
                caller_span,
                turn_cancel,
            )
            .await;
        self.handle_pipeline_result(result, session_key)
    }

    // NOTE(#940): 113 lines: setup and spawn a single pipeline task with streaming
    // bridge. The sequential setup + spawn is one cohesive operation.
    /// Spawn the pipeline as a separate tokio task to catch panics.
    ///
    /// When `db_session_id` is `Some`, the actor adopts that ID for the
    /// in-memory `SessionState` instead of generating a new ULID. This
    /// ensures the actor's session ID matches the database row created by
    /// pylon, preventing FK constraint failures in finalize and tools.
    ///
    /// The session is persisted BEFORE spawning the pipeline task (#2160):
    /// if the actor crashes mid-pipeline, the `session_id` survives in fjall
    /// for recovery instead of being lost with the in-memory `HashMap`.
    #[expect(
        clippy::too_many_lines,
        reason = "pipeline setup is sequential and cohesive; splitting adds indirection"
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "pipeline spawn plumbs session, content, stream, gate, span, and cancel; splitting hides the call shape"
    )]
    pub(super) async fn spawn_pipeline_task(
        &mut self,
        session_key: &str,
        db_session_id: Option<&str>,
        content: &str,
        stream_tx: Option<mpsc::Sender<TurnStreamEvent>>,
        approval_gate: Option<crate::approval::ApprovalGate>,
        caller_span: tracing::Span,
        turn_cancel: CancellationToken,
    ) -> Result<crate::error::Result<TurnResult>, tokio::task::JoinError> {
        self.evict_oldest_session_if_needed();
        let session = self
            .sessions
            .entry(session_key.to_owned())
            .or_insert_with(|| {
                let id =
                    db_session_id.map_or_else(|| SessionId::new().to_string(), ToOwned::to_owned);
                debug!(session_key, session_id = %id, "creating new session");
                SessionState::new(id, session_key.to_owned(), &self.config)
            });

        session.next_turn();

        // WHY: surprise is episodic — advance the running session prior with
        // this turn's content here, on the authoritative SessionState, so the
        // EMA persists across turns. The pipeline below scores candidates
        // read-only against the clone (mutations inside the spawned task are
        // discarded). Skipped entirely when surprise scoring is inert.
        if self.config.recall.surprise_weight > f64::EPSILON {
            session.surprise_calculator.compute_surprise(content);
        }

        // INVARIANT(#2160): the session is persisted BEFORE the pipeline task spawns.
        if let Some(ref store) = self.stores.session_store {
            let guard = store.lock().await;
            match guard.find_or_create_session(
                &session.id,
                &session.nous_id,
                &session.session_key,
                Some(&session.model),
                None,
            ) {
                Ok(db_session) if db_session.id != session.id => {
                    // WHY(#3103): The DB already had a session for (nous_id, session_key)
                    // with a different ID (e.g., from a previous cycle or a restart).
                    // find_or_create_session returns the canonical DB ID via
                    // ON CONFLICT DO NOTHING + SELECT. If we kept the actor's generated
                    // ID, finalize would call append_message with an ID that has no
                    // DB row → FOREIGN KEY constraint failure and silent data loss.
                    // Adopt the DB session ID so the actor and DB stay in sync.
                    debug!(
                        actor_id = %session.id,
                        db_id = %db_session.id,
                        session_key = %session.session_key,
                        "adopting DB session ID — actor ID diverged from DB"
                    );
                    session.id.clone_from(&db_session.id);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(
                        session_id = %session.id,
                        error = %e,
                        "failed to pre-persist session — pipeline will retry in finalize"
                    );
                }
            }
        }

        let input = crate::pipeline::PipelineInput {
            content: content.to_owned(),
            session: session.clone(),
            config: self.pipeline_config.clone(),
        };

        let nous_id = NousId::new(&self.id).map_err(|e| {
            crate::error::ConfigSnafu {
                message: format!("invalid nous id: {e}"),
            }
            .build()
        });

        let nous_id = match nous_id {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };

        let session_id = match SessionId::parse(session.id.as_str()) {
            Ok(id) => id,
            Err(e) => {
                return Ok(Err(crate::error::ConfigSnafu {
                    message: format!("invalid session id '{}': {e}", session.id),
                }
                .build()));
            }
        };

        let tool_ctx = ToolContext {
            nous_id,
            session_id,
            turn_number: session.turn,
            workspace: self.config.workspace.clone(),
            allowed_roots: self.config.allowed_roots.clone(),
            services: self.services.tool_services.clone(),
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
            tool_config: self.services.tool_config.clone(),
        };

        let mut extra_bootstrap = self.extra_bootstrap.clone();
        extra_bootstrap.extend(self.resolve_intent_sections());
        extra_bootstrap.extend(self.resolve_skill_sections(content).await);
        let tool_estimator =
            crate::budget::CharEstimator::new(u64::from(self.config.generation.chars_per_token));
        let active_snapshot = tool_ctx.active_tools.read().map_or_else(
            |poisoned| poisoned.into_inner().clone(),
            |guard| guard.clone(),
        );
        let active = if active_snapshot.is_empty() {
            HashSet::new()
        } else {
            active_snapshot
        };
        let surface = self.services.tools.effective_surface(SurfaceInputs {
            policy: &self.config.tool_groups,
            allowlist: self.config.tool_allowlist.as_deref(),
            active: &active,
            server_tools: &self.config.server_tools,
            server_tool_config: self
                .services
                .tool_services
                .as_deref()
                .map(|services| &services.server_tool_config),
        });
        if let Some(section) =
            crate::bootstrap::tools::tool_summary_bootstrap_section(&surface, &tool_estimator)
        {
            extra_bootstrap.push(section);
        }

        // WHY: create hook registry from config so hooks run inside the spawned pipeline task
        let mut hook_registry = crate::hooks::registry::HookRegistry::new();
        let workspace = self.config.workspace.clone();
        let working_checkpoint_store = self
            .services
            .tool_services
            .as_ref()
            .and_then(|ts| ts.working_checkpoint_store.clone());
        crate::hooks::builtins::register_builtin_hooks(
            &mut hook_registry,
            &self.config.hooks,
            &workspace,
            working_checkpoint_store,
        );

        let oikos = Arc::clone(&self.services.oikos);
        let config = self.config.clone();
        let pipeline_config = self.pipeline_config.clone();
        let providers = Arc::clone(&self.services.providers);
        let tools = Arc::clone(&self.services.tools);
        let embedding_provider = self.services.embedding_provider.clone();
        let vector_search = self.stores.vector_search.clone();
        #[cfg(feature = "knowledge-store")]
        let text_search = self.stores.text_search.clone();
        let session_store = self.stores.session_store.clone();
        // WHY: share the bootstrap file cache across the spawned pipeline task
        // so cached workspace reads are reused turn-to-turn (#3388).
        let bootstrap_cache = Arc::clone(&self.services.bootstrap_cache);
        let audit_log = self.services.audit_log.clone();
        let scoped_turn_cancel = turn_cancel.clone();
        let mut pipeline_task = tokio::spawn(
            async move {
                Box::pin(ToolContext::scope_turn_cancel(
                    scoped_turn_cancel,
                    async move {
                        #[cfg(feature = "knowledge-store")]
                        let text_search_ref: Option<
                            &dyn crate::recall::TextSearch,
                        > = text_search.as_deref();
                        #[cfg(not(feature = "knowledge-store"))]
                        let text_search_ref: Option<
                            &dyn crate::recall::TextSearch,
                        > = None;

                        match stream_tx {
                            Some(ref stx) => {
                                crate::pipeline::run_pipeline(
                                    input,
                                    &oikos,
                                    &config,
                                    &pipeline_config,
                                    &providers,
                                    &tools,
                                    &tool_ctx,
                                    embedding_provider.as_deref(),
                                    vector_search.as_deref(),
                                    text_search_ref,
                                    session_store.as_deref(),
                                    extra_bootstrap,
                                    Some(stx),
                                    approval_gate.as_ref(),
                                    None,
                                    Some(&hook_registry),
                                    Some(bootstrap_cache.as_ref()),
                                    audit_log.as_deref(),
                                )
                                .await
                            }
                            None => {
                                crate::pipeline::run_pipeline(
                                    input,
                                    &oikos,
                                    &config,
                                    &pipeline_config,
                                    &providers,
                                    &tools,
                                    &tool_ctx,
                                    embedding_provider.as_deref(),
                                    vector_search.as_deref(),
                                    text_search_ref,
                                    session_store.as_deref(),
                                    extra_bootstrap,
                                    None,
                                    None,
                                    None,
                                    Some(&hook_registry),
                                    Some(bootstrap_cache.as_ref()),
                                    audit_log.as_deref(),
                                )
                                .await
                            }
                        }
                    },
                ))
                .await
            }
            .instrument(caller_span),
        );

        tokio::select! {
            result = &mut pipeline_task => result,
            () = turn_cancel.cancelled() => {
                // WHY(#4713): request cancellation is a coarse signal, not a
                // cooperative checkpoint inside the pipeline. Aborting the task
                // reverts the in-memory turn counter, so persisted session
                // history remains consistent (the turn is not finalized). Any
                // LLM calls or tool side effects already in flight are
                // best-effort and may complete after cancellation; callers
                // receive `TurnCancelled` and must treat in-flight side effects
                // as unobserved for this turn.
                pipeline_task.abort();
                let _ = pipeline_task.await;
                // WHY: the turn counter was advanced before the pipeline task
                // was spawned. Since the task never completed, revert the
                // in-memory increment so the next successful turn does not
                // leave a visible gap in the session's turn numbering.
                if let Some(session) = self.sessions.get_mut(session_key) {
                    session.revert_turn();
                }
                Ok(Err(crate::error::TurnCancelledSnafu {
                    reason: "request cancelled".to_owned(),
                }.build()))
            }
        }
    }

    /// Convert a spawned pipeline result (which may be a panic) into an `error::Result`.
    /// Records panic if one occurred and potentially enters degraded mode.
    fn handle_pipeline_result(
        &mut self,
        result: Result<crate::error::Result<TurnResult>, tokio::task::JoinError>,
        session_key: &str,
    ) -> crate::error::Result<TurnResult> {
        match result {
            Ok(inner) => inner,
            Err(join_error) => {
                self.record_pipeline_panic();

                let panic_msg = if join_error.is_panic() {
                    let panic_payload = join_error.into_panic();
                    if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        (*s).to_owned()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_owned()
                    }
                } else {
                    format!("task cancelled: {join_error}")
                };

                error!(
                    nous_id = %self.id,
                    session_key = %session_key,
                    panic_count = self.runtime.pipeline_panic_count,
                    message = %panic_msg,
                    "pipeline panicked — actor continues"
                );

                Err(crate::error::PipelinePanicSnafu {
                    nous_id: self.id.clone(),
                    message: panic_msg,
                }
                .build())
            }
        }
    }

    /// Record a pipeline panic occurrence. Enters degraded mode if too many panics in the window.
    pub(super) fn record_pipeline_panic(&mut self) {
        self.runtime.pipeline_panic_count += 1;
        let now = std::time::Instant::now();
        self.runtime.pipeline_panic_timestamps.push(now);
        self.runtime.last_panic_at = Some(now);

        let degraded_window = Duration::from_secs(self.nous_behavior.degraded_window_secs);
        let cutoff = std::time::Instant::now()
            .checked_sub(degraded_window)
            .unwrap_or(self.runtime.started_at);
        self.runtime
            .pipeline_panic_timestamps
            .retain(|t| *t > cutoff);

        let threshold = self.nous_behavior.degraded_panic_threshold;
        tracing::debug!(
            degraded_panic_threshold = threshold,
            degraded_window_secs = self.nous_behavior.degraded_window_secs,
            recent_panics = self.runtime.pipeline_panic_timestamps.len(),
            "record_pipeline_panic: checking degraded threshold"
        );

        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: degraded_panic_threshold is a small constant, fits in usize"
        )]
        if self.runtime.pipeline_panic_timestamps.len() >= threshold as usize {
            warn!(
                nous_id = %self.id,
                panic_count = self.runtime.pipeline_panic_count,
                recent_panics = self.runtime.pipeline_panic_timestamps.len(),
                "entering degraded mode — too many panics in window"
            );
            self.channel.status = NousLifecycle::Degraded;
        }
    }
}
