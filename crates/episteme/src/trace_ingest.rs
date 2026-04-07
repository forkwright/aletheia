//! Structured tracing subscriber layer for operational Datalog facts.
//!
//! Captures key tracing events — turn completed, tool executed, error occurred —
//! and converts them into krites-compatible tuples stored in `ops.*` relations.
//! Events are buffered in memory and written in batches, not per-event, to avoid
//! contention on the knowledge store.
//!
//! # Schema
//!
//! ```text
//! ops_turns { session_id: String, nous_id: String =>
//!     model: String, tokens: Int, duration_ms: Int, timestamp: String }
//!
//! ops_tools { session_id: String, tool_name: String, timestamp: String =>
//!     success: Bool, duration_ms: Int }
//!
//! ops_errors { session_id: String, error_class: String, timestamp: String =>
//!     message: String }
//! ```
//!
//! # Feature gate
//!
//! The [`TraceIngestLayer`] type is always available so it can be composed into
//! the subscriber stack without a feature check at the call site.  The
//! [`TraceIngestLayer::flush`] method (which writes to the Datalog engine) is
//! gated on the `mneme-engine` feature; without it the buffer still fills but
//! flushes are no-ops.

use std::sync::Mutex;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Structured event variant captured by the ingest layer.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TraceEvent {
    /// A conversation turn completed.
    TurnCompleted {
        /// Owning session identifier.
        session_id: String,
        /// Agent identifier.
        nous_id: String,
        /// Model name used for this turn.
        model: String,
        /// Total tokens consumed (prompt + completion).
        tokens: i64,
        /// Wall-clock duration of the turn in milliseconds.
        duration_ms: i64,
        /// ISO-8601 timestamp when the turn completed.
        timestamp: String,
    },
    /// A tool call completed.
    ToolExecuted {
        /// Owning session identifier.
        session_id: String,
        /// Name of the tool that was called.
        tool_name: String,
        /// Whether the tool call returned successfully.
        success: bool,
        /// Wall-clock duration of the tool call in milliseconds.
        duration_ms: i64,
        /// ISO-8601 timestamp when the call completed.
        timestamp: String,
    },
    /// An error was recorded.
    ErrorOccurred {
        /// Owning session identifier.
        session_id: String,
        /// Classifier / category for the error (e.g. `"rate_limit"`).
        error_class: String,
        /// Human-readable error message.
        message: String,
        /// ISO-8601 timestamp when the error occurred.
        timestamp: String,
    },
}

/// DDL that creates the three `ops.*` relations in a Datalog database.
///
/// Apply this before the first [`TraceIngestLayer::flush`] call.  In production
/// the knowledge-store init path runs all DDL; this constant is exposed so
/// tests and the init migration can reference the canonical schema.
pub const OPS_DDL: &[&str] = &[
    r":create ops_turns {
        session_id: String, nous_id: String =>
        model: String,
        tokens: Int,
        duration_ms: Int,
        timestamp: String
    }",
    r":create ops_tools {
        session_id: String, tool_name: String, timestamp: String =>
        success: Bool,
        duration_ms: Int
    }",
    r":create ops_errors {
        session_id: String, error_class: String, timestamp: String =>
        message: String
    }",
];

// ── Visitor ───────────────────────────────────────────────────────────────────

/// Field visitor that extracts structured key-value fields from a tracing event.
struct EventVisitor {
    fields: std::collections::BTreeMap<String, FieldValue>,
}

/// A single field value captured by the visitor.
#[derive(Debug, Clone)]
enum FieldValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl EventVisitor {
    fn new() -> Self {
        Self {
            fields: std::collections::BTreeMap::new(),
        }
    }

    fn get_str(&self, key: &str) -> Option<&str> {
        match self.fields.get(key) {
            Some(FieldValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    fn get_int(&self, key: &str) -> Option<i64> {
        match self.fields.get(key) {
            Some(FieldValue::Int(n)) => Some(*n),
            _ => None,
        }
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        match self.fields.get(key) {
            Some(FieldValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_owned(),
            FieldValue::Str(format!("{value:?}")),
        );
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::Str(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::Int(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        // Saturating cast: u64 > i64::MAX is astronomically unlikely for
        // operational fields (tokens, duration_ms).  Use i64::try_from with
        // saturation so the value is always representable.
        let v = i64::try_from(value).unwrap_or(i64::MAX);
        self.fields
            .insert(field.name().to_owned(), FieldValue::Int(v));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::Bool(value));
    }
}

// ── Layer ─────────────────────────────────────────────────────────────────────

/// Tracing subscriber layer that captures structured operational events.
///
/// Install via [`tracing_subscriber::registry().with(TraceIngestLayer::new())`].
/// Call [`TraceIngestLayer::flush`] periodically (e.g., every 30 s) to drain the
/// buffer into the Datalog engine.
///
/// Event recognition is driven by the `message` field.  Emit matching events with
/// [`tracing::info!`] and the field names documented on each [`TraceEvent`] variant:
///
/// ```text
/// tracing::info!(
///     message = "turn_completed",
///     session_id = %session_id,
///     nous_id    = %nous_id,
///     model      = %model,
///     tokens     = tokens_total,
///     duration_ms = elapsed.as_millis() as i64,
/// );
/// ```
pub struct TraceIngestLayer {
    buffer: Mutex<Vec<TraceEvent>>,
}

impl TraceIngestLayer {
    /// Create a new ingest layer with an empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Drain the buffer and return the captured events.
    ///
    /// This is the infallible variant — it returns events regardless of whether
    /// a knowledge store is available.  Use [`TraceIngestLayer::flush`] to write
    /// them to Datalog.
    pub fn drain(&self) -> Vec<TraceEvent> {
        let mut buf = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *buf)
    }

    /// How many events are waiting in the buffer.
    pub fn pending(&self) -> usize {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Flush buffered events into the Datalog knowledge store.
    ///
    /// Drains the buffer and executes one `:put` statement per event.  Errors
    /// from individual puts are logged at `WARN` level and do not abort the
    /// remaining writes — operational telemetry is best-effort.
    ///
    /// Requires the `mneme-engine` feature.  Without it this is a no-op that
    /// still drains the buffer (preventing unbounded growth).
    #[cfg(feature = "mneme-engine")]
    pub fn flush(&self, store: &crate::knowledge_store::KnowledgeStore) {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};

        let events = self.drain();
        if events.is_empty() {
            return;
        }

        tracing::debug!(count = events.len(), "trace_ingest: flushing buffered events");

        for event in events {
            let (script, params) = match event {
                TraceEvent::TurnCompleted {
                    session_id,
                    nous_id,
                    model,
                    tokens,
                    duration_ms,
                    timestamp,
                } => {
                    let mut p = BTreeMap::new();
                    p.insert("session_id".to_owned(), DataValue::Str(session_id.into()));
                    p.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
                    p.insert("model".to_owned(), DataValue::Str(model.into()));
                    p.insert("tokens".to_owned(), DataValue::from(tokens));
                    p.insert("duration_ms".to_owned(), DataValue::from(duration_ms));
                    p.insert("timestamp".to_owned(), DataValue::Str(timestamp.into()));
                    let script = concat!(
                        "?[session_id, nous_id, model, tokens, duration_ms, timestamp]",
                        " <- [[$session_id, $nous_id, $model, $tokens, $duration_ms, $timestamp]]",
                        " :put ops_turns { session_id, nous_id => model, tokens, duration_ms, timestamp }"
                    );
                    (script, p)
                }
                TraceEvent::ToolExecuted {
                    session_id,
                    tool_name,
                    success,
                    duration_ms,
                    timestamp,
                } => {
                    let mut p = BTreeMap::new();
                    p.insert("session_id".to_owned(), DataValue::Str(session_id.into()));
                    p.insert("tool_name".to_owned(), DataValue::Str(tool_name.into()));
                    p.insert("success".to_owned(), DataValue::Bool(success));
                    p.insert("duration_ms".to_owned(), DataValue::from(duration_ms));
                    p.insert("timestamp".to_owned(), DataValue::Str(timestamp.into()));
                    let script = concat!(
                        "?[session_id, tool_name, timestamp, success, duration_ms]",
                        " <- [[$session_id, $tool_name, $timestamp, $success, $duration_ms]]",
                        " :put ops_tools { session_id, tool_name, timestamp => success, duration_ms }"
                    );
                    (script, p)
                }
                TraceEvent::ErrorOccurred {
                    session_id,
                    error_class,
                    message,
                    timestamp,
                } => {
                    let mut p = BTreeMap::new();
                    p.insert("session_id".to_owned(), DataValue::Str(session_id.into()));
                    p.insert(
                        "error_class".to_owned(),
                        DataValue::Str(error_class.into()),
                    );
                    p.insert("message".to_owned(), DataValue::Str(message.into()));
                    p.insert("timestamp".to_owned(), DataValue::Str(timestamp.into()));
                    let script = concat!(
                        "?[session_id, error_class, timestamp, message]",
                        " <- [[$session_id, $error_class, $timestamp, $message]]",
                        " :put ops_errors { session_id, error_class, timestamp => message }"
                    );
                    (script, p)
                }
            };

            if let Err(e) = store.run_mut_query(script, params) {
                tracing::warn!(error = %e, "trace_ingest: failed to write event to ops relation");
            }
        }
    }

    /// No-op drain when `mneme-engine` is not enabled.
    ///
    /// Prevents unbounded buffer growth when no knowledge store is wired in.
    #[cfg(not(feature = "mneme-engine"))]
    pub fn flush_noop(&self) {
        self.drain();
    }
}

impl Default for TraceIngestLayer {
    fn default() -> Self {
        Self::new()
    }
}

fn now_iso8601() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

impl<S> Layer<S> for TraceIngestLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = EventVisitor::new();
        event.record(&mut visitor);

        let Some(message) = visitor.get_str("message") else {
            return;
        };

        let captured = match message {
            "turn_completed" => {
                let session_id = visitor.get_str("session_id").unwrap_or("").to_owned();
                let nous_id = visitor.get_str("nous_id").unwrap_or("").to_owned();
                let model = visitor.get_str("model").unwrap_or("").to_owned();
                let tokens = visitor.get_int("tokens").unwrap_or(0);
                let duration_ms = visitor.get_int("duration_ms").unwrap_or(0);
                let timestamp = visitor
                    .get_str("timestamp")
                    .map_or_else(now_iso8601, ToOwned::to_owned);
                TraceEvent::TurnCompleted {
                    session_id,
                    nous_id,
                    model,
                    tokens,
                    duration_ms,
                    timestamp,
                }
            }
            "tool_executed" => {
                let session_id = visitor.get_str("session_id").unwrap_or("").to_owned();
                let tool_name = visitor.get_str("tool_name").unwrap_or("").to_owned();
                let success = visitor.get_bool("success").unwrap_or(false);
                let duration_ms = visitor.get_int("duration_ms").unwrap_or(0);
                let timestamp = visitor
                    .get_str("timestamp")
                    .map_or_else(now_iso8601, ToOwned::to_owned);
                TraceEvent::ToolExecuted {
                    session_id,
                    tool_name,
                    success,
                    duration_ms,
                    timestamp,
                }
            }
            "error_occurred" => {
                let session_id = visitor.get_str("session_id").unwrap_or("").to_owned();
                let error_class = visitor.get_str("error_class").unwrap_or("").to_owned();
                let message = visitor.get_str("error_message").unwrap_or("").to_owned();
                let timestamp = visitor
                    .get_str("timestamp")
                    .map_or_else(now_iso8601, ToOwned::to_owned);
                TraceEvent::ErrorOccurred {
                    session_id,
                    error_class,
                    message,
                    timestamp,
                }
            }
            // Not a recognised operational event — pass through silently.
            _ => return,
        };

        let mut buf = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buf.push(captured);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    use super::*;

    fn push_event(layer: &TraceIngestLayer, event: TraceEvent) {
        let mut buf = layer
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buf.push(event);
    }

    #[test]
    fn captures_turn_completed_event() {
        let layer = TraceIngestLayer::new();

        push_event(
            &layer,
            TraceEvent::TurnCompleted {
                session_id: "sess-1".to_owned(),
                nous_id: "syn".to_owned(),
                model: "claude-3".to_owned(),
                tokens: 1024,
                duration_ms: 500,
                timestamp: now_iso8601(),
            },
        );

        let drained = layer.drain();
        assert_eq!(drained.len(), 1);
        match &drained[0] {
            TraceEvent::TurnCompleted {
                session_id,
                nous_id,
                model,
                tokens,
                duration_ms,
                ..
            } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(nous_id, "syn");
                assert_eq!(model, "claude-3");
                assert_eq!(*tokens, 1024);
                assert_eq!(*duration_ms, 500);
            }
            other => panic!("unexpected event variant: {other:?}"),
        }

        // Buffer should be empty after drain.
        assert_eq!(layer.pending(), 0);
    }

    #[test]
    fn captures_tool_executed_event() {
        let layer = TraceIngestLayer::new();

        push_event(
            &layer,
            TraceEvent::ToolExecuted {
                session_id: "sess-2".to_owned(),
                tool_name: "web_search".to_owned(),
                success: true,
                duration_ms: 120,
                timestamp: now_iso8601(),
            },
        );

        let drained = layer.drain();
        assert_eq!(drained.len(), 1);
        match &drained[0] {
            TraceEvent::ToolExecuted {
                session_id,
                tool_name,
                success,
                ..
            } => {
                assert_eq!(session_id, "sess-2");
                assert_eq!(tool_name, "web_search");
                assert!(*success);
            }
            other => panic!("unexpected event variant: {other:?}"),
        }
    }

    #[test]
    fn captures_error_occurred_event() {
        let layer = TraceIngestLayer::new();

        push_event(
            &layer,
            TraceEvent::ErrorOccurred {
                session_id: "sess-3".to_owned(),
                error_class: "rate_limit".to_owned(),
                message: "429 Too Many Requests".to_owned(),
                timestamp: now_iso8601(),
            },
        );

        let drained = layer.drain();
        assert_eq!(drained.len(), 1);
        match &drained[0] {
            TraceEvent::ErrorOccurred {
                session_id,
                error_class,
                message,
                ..
            } => {
                assert_eq!(session_id, "sess-3");
                assert_eq!(error_class, "rate_limit");
                assert!(message.contains("429"));
            }
            other => panic!("unexpected event variant: {other:?}"),
        }
    }

    #[test]
    fn ignores_unknown_message_types() {
        let layer = TraceIngestLayer::new();

        // Nothing pushed; drain must return empty.
        assert_eq!(layer.pending(), 0);
        assert!(layer.drain().is_empty());
    }

    #[test]
    fn pending_returns_buffer_length() {
        let layer = TraceIngestLayer::new();

        for i in 0..3_u32 {
            push_event(
                &layer,
                TraceEvent::ErrorOccurred {
                    session_id: format!("sess-{i}"),
                    error_class: "test".to_owned(),
                    message: "test".to_owned(),
                    timestamp: now_iso8601(),
                },
            );
        }

        assert_eq!(layer.pending(), 3);
        layer.drain();
        assert_eq!(layer.pending(), 0);
    }

    #[test]
    fn layer_on_event_via_tracing_subscriber() {
        let layer = std::sync::Arc::new(TraceIngestLayer::new());

        // Clone an Arc handle so we can inspect the buffer after recording.
        let captured = std::sync::Arc::clone(&layer);

        // Set a per-test default subscriber scoped to this thread.
        let _guard = tracing_subscriber::registry()
            .with(std::sync::Arc::clone(&layer))
            .set_default();

        tracing::info!(
            message = "tool_executed",
            session_id = "sess-live",
            tool_name = "file_read",
            success = true,
            duration_ms = 42_i64,
        );

        let events = captured.drain();
        assert_eq!(
            events.len(),
            1,
            "layer should have captured the tool_executed event"
        );
        match &events[0] {
            TraceEvent::ToolExecuted {
                session_id,
                tool_name,
                success,
                duration_ms,
                ..
            } => {
                assert_eq!(session_id, "sess-live");
                assert_eq!(tool_name, "file_read");
                assert!(*success);
                assert_eq!(*duration_ms, 42);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[cfg(feature = "mneme-engine")]
    mod engine_tests {
        use super::*;
        use crate::knowledge_store::{KnowledgeConfig, KnowledgeStore};

        fn open_store_with_ops_schema() -> std::sync::Arc<KnowledgeStore> {
            let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig::default())
                .expect("knowledge store init failed");
            for ddl in OPS_DDL {
                store
                    .run_mut_query(ddl, std::collections::BTreeMap::new())
                    .expect("ops DDL failed");
            }
            store
        }

        #[test]
        fn flush_writes_turn_to_ops_turns() {
            let store = open_store_with_ops_schema();
            let layer = TraceIngestLayer::new();

            super::push_event(
                &layer,
                TraceEvent::TurnCompleted {
                    session_id: "sess-a".to_owned(),
                    nous_id: "syn".to_owned(),
                    model: "claude-3".to_owned(),
                    tokens: 2048,
                    duration_ms: 800,
                    timestamp: "2026-04-06T12:00:00Z".to_owned(),
                },
            );

            layer.flush(&store);

            let result = store
                .run_query(
                    "?[session_id, nous_id, tokens] := *ops_turns{session_id, nous_id, tokens}",
                    std::collections::BTreeMap::new(),
                )
                .expect("query failed");
            assert_eq!(result.rows.len(), 1, "expected one ops_turns row");
        }

        #[test]
        fn flush_writes_tool_to_ops_tools() {
            let store = open_store_with_ops_schema();
            let layer = TraceIngestLayer::new();

            super::push_event(
                &layer,
                TraceEvent::ToolExecuted {
                    session_id: "sess-b".to_owned(),
                    tool_name: "web_search".to_owned(),
                    success: false,
                    duration_ms: 300,
                    timestamp: "2026-04-06T12:01:00Z".to_owned(),
                },
            );

            layer.flush(&store);

            let result = store
                .run_query(
                    "?[session_id, tool_name, success] := *ops_tools{session_id, tool_name, success: success}",
                    std::collections::BTreeMap::new(),
                )
                .expect("query failed");
            assert_eq!(result.rows.len(), 1, "expected one ops_tools row");
        }

        #[test]
        fn flush_writes_error_to_ops_errors() {
            let store = open_store_with_ops_schema();
            let layer = TraceIngestLayer::new();

            super::push_event(
                &layer,
                TraceEvent::ErrorOccurred {
                    session_id: "sess-c".to_owned(),
                    error_class: "timeout".to_owned(),
                    message: "connection timed out".to_owned(),
                    timestamp: "2026-04-06T12:02:00Z".to_owned(),
                },
            );

            layer.flush(&store);

            let result = store
                .run_query(
                    "?[session_id, error_class] := *ops_errors{session_id, error_class}",
                    std::collections::BTreeMap::new(),
                )
                .expect("query failed");
            assert_eq!(result.rows.len(), 1, "expected one ops_errors row");
        }
    }
}
