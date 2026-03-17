//! Turn execution: handles individual turns with panic boundary protection.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;
use tracing::{Instrument, debug, error, warn};

use aletheia_koina::id::{NousId, SessionId};
use aletheia_organon::types::ToolContext;

use crate::pipeline::TurnResult;
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

use super::{DEGRADED_PANIC_THRESHOLD, DEGRADED_WINDOW, NousActor, NousLifecycle};

impl NousActor {
    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation. Sets `lifecycle = Active` and
    /// `active_session` before awaiting `execute_turn`. If the future
    /// were dropped mid-await, those fields would not be reset. In
    /// practice this is only called from the sequential actor loop, so
    /// cancellation only occurs at shutdown when the actor is consumed.
    pub(super) async fn handle_turn(
        &mut self,
        session_key: String,
        session_id: Option<String>,
        content: String,
        caller_span: tracing::Span,
        reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
    ) {
        if self.channel.status == NousLifecycle::Dormant {
            debug!("auto-waking from dormant for turn");
            self.channel.status = NousLifecycle::Idle;
        }

        self.channel.status = NousLifecycle::Active;
        self.active_session = Some(session_key.clone());
        self.runtime.active_turn.store(true, Ordering::Release);

        let result = self
            .execute_turn_with_panic_boundary(
                &session_key,
                session_id.as_deref(),
                &content,
                caller_span,
            )
            .await;

        if let Ok(ref turn_result) = result {
            if let Some(session) = self.sessions.get_mut(&session_key) {
                session.cumulative_tokens = session
                    .cumulative_tokens
                    .saturating_add(turn_result.usage.input_tokens)
                    .saturating_add(turn_result.usage.output_tokens);
            }
            self.maybe_spawn_extraction(&content, &turn_result.content);
            self.maybe_spawn_skill_analysis(&turn_result.tool_calls, &session_key);
            self.maybe_spawn_distillation(&session_key).await;
        }

        self.active_session = None;
        // WHY: only reset to Idle if not degraded: preserve degraded state
        if self.channel.status != NousLifecycle::Degraded {
            self.channel.status = NousLifecycle::Idle;
        }
        self.runtime.active_turn.store(false, Ordering::Release);

        // WHY: ignore send error: caller may have dropped the receiver
        let _ = reply.send(result);
    }

    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation: same profile as `handle_turn`.
    /// Sets `lifecycle` and `active_session` before the `.await` point.
    /// Only called from the sequential actor loop.
    pub(super) async fn handle_streaming_turn(
        &mut self,
        session_key: String,
        session_id: Option<String>,
        content: String,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        caller_span: tracing::Span,
        reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
    ) {
        if self.channel.status == NousLifecycle::Dormant {
            debug!("auto-waking from dormant for streaming turn");
            self.channel.status = NousLifecycle::Idle;
        }

        self.channel.status = NousLifecycle::Active;
        self.active_session = Some(session_key.clone());
        self.runtime.active_turn.store(true, Ordering::Release);

        let result = self
            .execute_streaming_turn_with_panic_boundary(
                &session_key,
                session_id.as_deref(),
                &content,
                &stream_tx,
                caller_span,
            )
            .await;

        if let Ok(ref turn_result) = result {
            if let Some(session) = self.sessions.get_mut(&session_key) {
                session.cumulative_tokens = session
                    .cumulative_tokens
                    .saturating_add(turn_result.usage.input_tokens)
                    .saturating_add(turn_result.usage.output_tokens);
            }
            self.maybe_spawn_extraction(&content, &turn_result.content);
            self.maybe_spawn_skill_analysis(&turn_result.tool_calls, &session_key);
            self.maybe_spawn_distillation(&session_key).await;
        }

        self.active_session = None;
        // WHY: only reset to Idle if not degraded: preserve degraded state
        if self.channel.status != NousLifecycle::Degraded {
            self.channel.status = NousLifecycle::Idle;
        }
        self.runtime.active_turn.store(false, Ordering::Release);

        // WHY: ignore send error: caller may have dropped the receiver
        let _ = reply.send(result);
    }

    /// Execute a turn with a panic boundary. If the pipeline panics, the panic
    /// is caught, logged, and an error is returned to the caller. The actor
    /// continues processing subsequent messages.
    async fn execute_turn_with_panic_boundary(
        &mut self,
        session_key: &str,
        session_id: Option<&str>,
        content: &str,
        caller_span: tracing::Span,
    ) -> crate::error::Result<TurnResult> {
        // WHY: pipeline spawned in separate task so panics are caught by JoinHandle, not the actor loop
        let result = self
            .spawn_pipeline_task(session_key, session_id, content, None, caller_span)
            .await;
        self.handle_pipeline_result(result, session_key)
    }

    /// Execute a streaming turn with a panic boundary.
    async fn execute_streaming_turn_with_panic_boundary(
        &mut self,
        session_key: &str,
        session_id: Option<&str>,
        content: &str,
        stream_tx: &mpsc::Sender<TurnStreamEvent>,
        caller_span: tracing::Span,
    ) -> crate::error::Result<TurnResult> {
        let result = self
            .spawn_pipeline_task(
                session_key,
                session_id,
                content,
                Some(stream_tx.clone()),
                caller_span,
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
    async fn spawn_pipeline_task(
        &mut self,
        session_key: &str,
        db_session_id: Option<&str>,
        content: &str,
        stream_tx: Option<mpsc::Sender<TurnStreamEvent>>,
        caller_span: tracing::Span,
    ) -> Result<crate::error::Result<TurnResult>, tokio::task::JoinError> {
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

        let tool_ctx = ToolContext {
            nous_id,
            session_id: SessionId::parse(session.id.as_str()).unwrap_or_else(|_| SessionId::new()),
            workspace: self.services.oikos.nous_dir(&self.id),
            allowed_roots: vec![self.services.oikos.root().to_path_buf()],
            services: self.services.tool_services.clone(),
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        };

        let mut extra_bootstrap = self.extra_bootstrap.clone();
        extra_bootstrap.extend(self.resolve_skill_sections(content).await);

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
        tokio::spawn(
            async move {
                #[cfg(feature = "knowledge-store")]
                let text_search_ref: Option<&dyn crate::recall::TextSearch> =
                    text_search.as_deref();
                #[cfg(not(feature = "knowledge-store"))]
                let text_search_ref: Option<&dyn crate::recall::TextSearch> = None;

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
                        )
                        .await
                    }
                }
            }
            .instrument(caller_span),
        )
        .await
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
                self.record_panic();

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
                    panic_count = self.runtime.panic_count,
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

    /// Record a panic occurrence. Enters degraded mode if too many panics in the window.
    pub(super) fn record_panic(&mut self) {
        self.runtime.panic_count += 1;
        self.runtime
            .panic_timestamps
            .push(std::time::Instant::now());

        let cutoff = std::time::Instant::now()
            .checked_sub(DEGRADED_WINDOW)
            .unwrap_or(self.runtime.started_at);
        self.runtime.panic_timestamps.retain(|t| *t > cutoff);

        if self.runtime.panic_timestamps.len() >= DEGRADED_PANIC_THRESHOLD as usize {
            warn!(
                nous_id = %self.id,
                panic_count = self.runtime.panic_count,
                recent_panics = self.runtime.panic_timestamps.len(),
                "entering degraded mode — too many panics in window"
            );
            self.channel.status = NousLifecycle::Degraded;
        }
    }

    /// # Cancel safety
    ///
    /// Cancel-safe. Session creation via `entry().or_insert_with()` is
    /// idempotent. If cancelled during `run_pipeline`, the session exists
    /// but the turn is incomplete: no persistent state is corrupted.
    pub(super) async fn execute_turn(
        &mut self,
        session_key: &str,
        content: &str,
    ) -> crate::error::Result<TurnResult> {
        let session = self
            .sessions
            .entry(session_key.to_owned())
            .or_insert_with(|| {
                // WHY: cross-nous messages carry no database session ID: generate one so finalize can create the DB row
                let id = SessionId::new().to_string();
                debug!(session_key, session_id = %id, "creating new session");
                SessionState::new(id, session_key.to_owned(), &self.config)
            });

        session.next_turn();

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
        })?;

        let tool_ctx = ToolContext {
            nous_id,
            session_id: SessionId::parse(session.id.as_str()).unwrap_or_else(|_| SessionId::new()),
            workspace: self.services.oikos.nous_dir(&self.id),
            allowed_roots: vec![self.services.oikos.root().to_path_buf()],
            services: self.services.tool_services.clone(),
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        };

        let mut extra_bootstrap = self.extra_bootstrap.clone();
        extra_bootstrap.extend(self.resolve_skill_sections(content).await);

        #[cfg(feature = "knowledge-store")]
        let text_search_ref: Option<&dyn crate::recall::TextSearch> =
            self.stores.text_search.as_deref();
        #[cfg(not(feature = "knowledge-store"))]
        let text_search_ref: Option<&dyn crate::recall::TextSearch> = None;

        crate::pipeline::run_pipeline(
            input,
            &self.services.oikos,
            &self.config,
            &self.pipeline_config,
            &self.services.providers,
            &self.services.tools,
            &tool_ctx,
            self.services.embedding_provider.as_deref(),
            self.stores.vector_search.as_deref(),
            text_search_ref,
            self.stores.session_store.as_deref(),
            extra_bootstrap,
            None,
        )
        .await
    }
}
