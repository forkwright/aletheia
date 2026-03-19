//! Internal event system coupling metrics and structured logs.
//!
//! Inspired by Vector's internal event topology: pipeline stages emit typed
//! events via a single [`emit()`] call that simultaneously produces a metric
//! increment and a structured log line. No double-instrumentation at call sites.
//!
//! # Usage
//!
//! ```
//! use aletheia_koina::event::{InternalEvent, EventEmitter, LogLevel};
//!
//! struct StageCompleted {
//!     stage: &'static str,
//!     duration_ms: u64,
//! }
//!
//! impl InternalEvent for StageCompleted {
//!     fn event_name(&self) -> &'static str { "StageCompleted" }
//!     fn log_level(&self) -> LogLevel { LogLevel::Info }
//!     fn log_message(&self) -> String {
//!         format!("stage {} completed in {}ms", self.stage, self.duration_ms)
//!     }
//!     fn metric_labels(&self) -> Vec<(&'static str, String)> {
//!         vec![("stage", self.stage.to_owned())]
//!     }
//! }
//!
//! let emitter = EventEmitter::new();
//! emitter.emit(&StageCompleted { stage: "context", duration_ms: 42 });
//! assert_eq!(emitter.event_count(), 1);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tracing::{debug, error, info, trace, warn};

/// Log level for an internal event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Trace-level detail.
    Trace,
    /// Debug-level detail.
    Debug,
    /// Informational.
    Info,
    /// Warning.
    Warn,
    /// Error.
    Error,
}

/// A typed internal event that produces both a log line and metric labels.
///
/// Implementors define what to log and what metric labels to attach.
/// The [`EventEmitter`] handles dispatching to both sinks.
pub trait InternalEvent: Send + Sync {
    /// Machine-readable event name (e.g. `"StageCompleted"`).
    fn event_name(&self) -> &'static str;

    /// Log level for the structured log line.
    fn log_level(&self) -> LogLevel;

    /// Human-readable log message.
    fn log_message(&self) -> String;

    /// Key-value pairs for metric labels.
    ///
    /// Empty vec means no metric is recorded, only a log line.
    fn metric_labels(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Optional numeric value for the metric (default: 1 for counter-style events).
    fn metric_value(&self) -> f64 {
        1.0
    }
}

/// Callback type for metric sinks.
///
/// Receives: event name, labels, and numeric value.
type MetricSink = dyn Fn(&str, &[(&str, String)], f64) + Send + Sync;

/// Listener callback type.
type Listener = dyn Fn(&dyn InternalEvent) + Send + Sync;

/// Event emitter that dispatches typed events to log and metric sinks.
///
/// Thread-safe via `Arc` internals. Clone is cheap (shared state).
#[derive(Clone)]
pub struct EventEmitter {
    /// Total events emitted (monotonic counter).
    counter: Arc<AtomicU64>,
    /// Optional metric sink for forwarding to Prometheus or other collectors.
    metric_sink: Arc<Option<Box<MetricSink>>>,
    /// Optional event listeners for testing and composition.
    listeners: Arc<Mutex<Vec<Box<Listener>>>>,
}

impl std::fmt::Debug for EventEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventEmitter")
            .field("event_count", &self.counter.load(Ordering::Relaxed))
            .field("has_metric_sink", &self.metric_sink.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter {
    /// Create an emitter with no metric sink (log-only).
    #[must_use]
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
            metric_sink: Arc::new(None),
            listeners: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create an emitter with a metric sink callback.
    #[must_use]
    pub fn with_metric_sink(
        sink: impl Fn(&str, &[(&str, String)], f64) + Send + Sync + 'static,
    ) -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
            metric_sink: Arc::new(Some(Box::new(sink))),
            listeners: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a listener that receives all emitted events.
    ///
    /// Useful for testing and event composition.
    pub fn add_listener(&self, listener: impl Fn(&dyn InternalEvent) + Send + Sync + 'static) {
        // WHY: lock held only during Vec::push, no await
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push(Box::new(listener));
        }
    }

    /// Emit a typed event: logs it and forwards to the metric sink.
    ///
    /// A single call at the stage level produces both a metric increment and
    /// a structured log line. No double-instrumentation needed.
    pub fn emit(&self, event: &impl InternalEvent) {
        self.counter.fetch_add(1, Ordering::Relaxed);

        // Log dispatch.
        let msg = event.log_message();
        let name = event.event_name();
        match event.log_level() {
            LogLevel::Trace => trace!(event = name, "{msg}"),
            LogLevel::Debug => debug!(event = name, "{msg}"),
            LogLevel::Info => info!(event = name, "{msg}"),
            LogLevel::Warn => warn!(event = name, "{msg}"),
            LogLevel::Error => error!(event = name, "{msg}"),
        }

        // Metric dispatch.
        let labels = event.metric_labels();
        if !labels.is_empty()
            && let Some(ref sink) = *self.metric_sink
        {
            sink(name, &labels, event.metric_value());
        }

        // Listener dispatch.
        if let Ok(listeners) = self.listeners.lock() {
            for listener in listeners.iter() {
                listener(event);
            }
        }
    }

    /// Total number of events emitted since creation.
    #[must_use]
    pub fn event_count(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Reset the event counter to zero.
    pub fn reset_count(&self) {
        self.counter.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::AtomicU64;

    struct TestEvent {
        stage: &'static str,
        duration_ms: u64,
    }

    impl InternalEvent for TestEvent {
        fn event_name(&self) -> &'static str {
            "TestEvent"
        }
        fn log_level(&self) -> LogLevel {
            LogLevel::Info
        }
        fn log_message(&self) -> String {
            format!("stage {} completed in {}ms", self.stage, self.duration_ms)
        }
        fn metric_labels(&self) -> Vec<(&'static str, String)> {
            vec![("stage", self.stage.to_owned())]
        }
    }

    struct ErrorEvent {
        reason: String,
    }

    impl InternalEvent for ErrorEvent {
        fn event_name(&self) -> &'static str {
            "ErrorEvent"
        }
        fn log_level(&self) -> LogLevel {
            LogLevel::Error
        }
        fn log_message(&self) -> String {
            format!("error: {}", self.reason)
        }
    }

    #[test]
    fn emit_increments_counter() {
        let emitter = EventEmitter::new();
        assert_eq!(emitter.event_count(), 0, "starts at zero");
        emitter.emit(&TestEvent {
            stage: "context",
            duration_ms: 10,
        });
        assert_eq!(emitter.event_count(), 1, "incremented after emit");
        emitter.emit(&TestEvent {
            stage: "execute",
            duration_ms: 100,
        });
        assert_eq!(emitter.event_count(), 2, "incremented again");
    }

    #[test]
    fn emit_calls_metric_sink() {
        let sink_count = Arc::new(AtomicU64::new(0));
        let count_clone = Arc::clone(&sink_count);
        let emitter = EventEmitter::with_metric_sink(move |_name, _labels, _val| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        });
        emitter.emit(&TestEvent {
            stage: "recall",
            duration_ms: 5,
        });
        assert_eq!(
            sink_count.load(Ordering::Relaxed),
            1,
            "metric sink called for event with labels"
        );
    }

    #[test]
    fn emit_skips_metric_sink_when_no_labels() {
        let sink_count = Arc::new(AtomicU64::new(0));
        let count_clone = Arc::clone(&sink_count);
        let emitter = EventEmitter::with_metric_sink(move |_name, _labels, _val| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        });
        emitter.emit(&ErrorEvent {
            reason: "test".to_owned(),
        });
        assert_eq!(
            sink_count.load(Ordering::Relaxed),
            0,
            "no metric for label-less event"
        );
    }

    #[test]
    fn clone_shares_state() {
        let emitter = EventEmitter::new();
        let clone = emitter.clone();
        emitter.emit(&TestEvent {
            stage: "a",
            duration_ms: 1,
        });
        assert_eq!(clone.event_count(), 1, "clone sees shared counter");
    }

    #[test]
    fn reset_count_zeroes() {
        let emitter = EventEmitter::new();
        emitter.emit(&TestEvent {
            stage: "a",
            duration_ms: 1,
        });
        emitter.reset_count();
        assert_eq!(emitter.event_count(), 0, "counter reset to zero");
    }

    #[test]
    fn listener_receives_events() {
        let received = Arc::new(AtomicU64::new(0));
        let recv_clone = Arc::clone(&received);
        let emitter = EventEmitter::new();
        emitter.add_listener(move |_event| {
            recv_clone.fetch_add(1, Ordering::Relaxed);
        });
        emitter.emit(&TestEvent {
            stage: "context",
            duration_ms: 10,
        });
        emitter.emit(&TestEvent {
            stage: "execute",
            duration_ms: 100,
        });
        assert_eq!(
            received.load(Ordering::Relaxed),
            2,
            "listener called for each event"
        );
    }

    #[test]
    fn default_metric_value_is_one() {
        let event = TestEvent {
            stage: "test",
            duration_ms: 0,
        };
        assert!(
            (event.metric_value() - 1.0).abs() < f64::EPSILON,
            "default metric value should be 1.0"
        );
    }

    #[test]
    fn single_emit_produces_log_and_metric() {
        let metric_calls = Arc::new(AtomicU64::new(0));
        let metric_clone = Arc::clone(&metric_calls);
        let log_calls = Arc::new(AtomicU64::new(0));
        let log_clone = Arc::clone(&log_calls);

        let emitter = EventEmitter::with_metric_sink(move |_name, _labels, _val| {
            metric_clone.fetch_add(1, Ordering::Relaxed);
        });
        emitter.add_listener(move |_event| {
            log_clone.fetch_add(1, Ordering::Relaxed);
        });

        // Single emit call at the stage level.
        emitter.emit(&TestEvent {
            stage: "context",
            duration_ms: 42,
        });

        assert_eq!(
            metric_calls.load(Ordering::Relaxed),
            1,
            "one metric increment from single emit"
        );
        assert_eq!(
            log_calls.load(Ordering::Relaxed),
            1,
            "one listener call from single emit"
        );
        assert_eq!(emitter.event_count(), 1, "one event total");
    }
}
