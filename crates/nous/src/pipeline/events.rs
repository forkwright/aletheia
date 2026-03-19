//! Typed pipeline events for the internal event system.
//!
//! Each event implements [`InternalEvent`] so a single `emit()` call at the
//! stage level produces both a metric increment and a structured log line.

use aletheia_koina::event::{InternalEvent, LogLevel};

/// A pipeline stage completed successfully.
pub(crate) struct StageCompleted {
    /// Agent identifier.
    pub(crate) nous_id: String,
    /// Stage name (context, recall, history, guard, execute, finalize).
    pub(crate) stage: &'static str,
    /// Duration in seconds.
    pub(crate) duration_secs: f64,
}

impl InternalEvent for StageCompleted {
    fn event_name(&self) -> &'static str {
        "StageCompleted"
    }

    fn log_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn log_message(&self) -> String {
        format!(
            "stage {} completed in {:.3}s for {}",
            self.stage, self.duration_secs, self.nous_id
        )
    }

    fn metric_labels(&self) -> Vec<(&'static str, String)> {
        vec![
            ("nous_id", self.nous_id.clone()),
            ("stage", self.stage.to_owned()),
        ]
    }

    fn metric_value(&self) -> f64 {
        self.duration_secs
    }
}

/// A pipeline stage encountered an error.
pub(crate) struct StageError {
    /// Agent identifier.
    pub(crate) nous_id: String,
    /// Stage name.
    pub(crate) stage: &'static str,
    /// Error classification.
    pub(crate) error_type: String,
}

impl InternalEvent for StageError {
    fn event_name(&self) -> &'static str {
        "StageError"
    }

    fn log_level(&self) -> LogLevel {
        LogLevel::Error
    }

    fn log_message(&self) -> String {
        format!(
            "stage {} error ({}) for {}",
            self.stage, self.error_type, self.nous_id
        )
    }

    fn metric_labels(&self) -> Vec<(&'static str, String)> {
        vec![
            ("nous_id", self.nous_id.clone()),
            ("stage", self.stage.to_owned()),
            ("error_type", self.error_type.clone()),
        ]
    }
}

/// A pipeline turn completed.
pub(crate) struct TurnCompleted {
    /// Agent identifier.
    pub(crate) nous_id: String,
    /// Model name.
    pub(crate) model: String,
    /// Total duration in milliseconds.
    pub(crate) duration_ms: u64,
    /// Input tokens consumed.
    pub(crate) input_tokens: u64,
    /// Output tokens produced.
    pub(crate) output_tokens: u64,
    /// Number of tool calls.
    pub(crate) tool_calls: u64,
    /// Number of stages completed.
    pub(crate) stages_completed: u32,
}

impl InternalEvent for TurnCompleted {
    fn event_name(&self) -> &'static str {
        "TurnCompleted"
    }

    fn log_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn log_message(&self) -> String {
        format!(
            "turn completed for {} (model={}, {}ms, in={}, out={}, tools={}, stages={})",
            self.nous_id,
            self.model,
            self.duration_ms,
            self.input_tokens,
            self.output_tokens,
            self.tool_calls,
            self.stages_completed,
        )
    }

    fn metric_labels(&self) -> Vec<(&'static str, String)> {
        vec![("nous_id", self.nous_id.clone())]
    }
}

/// A pipeline stage was skipped (e.g. recall without embedding provider).
pub(crate) struct StageSkipped {
    /// Agent identifier.
    pub(crate) nous_id: String,
    /// Stage name.
    pub(crate) stage: &'static str,
    /// Reason the stage was skipped.
    pub(crate) reason: String,
}

impl InternalEvent for StageSkipped {
    fn event_name(&self) -> &'static str {
        "StageSkipped"
    }

    fn log_level(&self) -> LogLevel {
        LogLevel::Debug
    }

    fn log_message(&self) -> String {
        format!(
            "stage {} skipped for {}: {}",
            self.stage, self.nous_id, self.reason
        )
    }
}

/// A pipeline stage timed out.
pub(crate) struct StageTimeout {
    /// Agent identifier.
    pub(crate) nous_id: String,
    /// Stage name.
    pub(crate) stage: &'static str,
    /// Timeout duration in seconds.
    pub(crate) timeout_secs: u32,
}

impl InternalEvent for StageTimeout {
    fn event_name(&self) -> &'static str {
        "StageTimeout"
    }

    fn log_level(&self) -> LogLevel {
        LogLevel::Warn
    }

    fn log_message(&self) -> String {
        format!(
            "stage {} timed out after {}s for {}",
            self.stage, self.timeout_secs, self.nous_id
        )
    }

    fn metric_labels(&self) -> Vec<(&'static str, String)> {
        vec![
            ("nous_id", self.nous_id.clone()),
            ("stage", self.stage.to_owned()),
            ("error_type", "timeout".to_owned()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_completed_event_fields() {
        let event = StageCompleted {
            nous_id: "test-agent".to_owned(),
            stage: "context",
            duration_secs: 0.042,
        };
        assert_eq!(event.event_name(), "StageCompleted");
        assert_eq!(event.log_level(), LogLevel::Info);
        assert!(
            event.log_message().contains("context"),
            "message mentions stage"
        );
        assert!(
            event.log_message().contains("test-agent"),
            "message mentions nous_id"
        );
        let labels = event.metric_labels();
        assert_eq!(labels.len(), 2, "nous_id and stage labels");
    }

    #[test]
    fn stage_error_event_fields() {
        let event = StageError {
            nous_id: "test-agent".to_owned(),
            stage: "execute",
            error_type: "timeout".to_owned(),
        };
        assert_eq!(event.event_name(), "StageError");
        assert_eq!(event.log_level(), LogLevel::Error);
        let labels = event.metric_labels();
        assert_eq!(labels.len(), 3, "nous_id, stage, error_type labels");
    }

    #[test]
    fn turn_completed_event_fields() {
        let event = TurnCompleted {
            nous_id: "test-agent".to_owned(),
            model: "test-model".to_owned(),
            duration_ms: 1234,
            input_tokens: 1000,
            output_tokens: 500,
            tool_calls: 3,
            stages_completed: 6,
        };
        assert_eq!(event.event_name(), "TurnCompleted");
        let msg = event.log_message();
        assert!(msg.contains("1234"), "message includes duration");
        assert!(msg.contains("test-model"), "message includes model");
    }

    #[test]
    fn stage_skipped_has_no_metric_labels() {
        let event = StageSkipped {
            nous_id: "test-agent".to_owned(),
            stage: "recall",
            reason: "no embedding provider".to_owned(),
        };
        assert!(
            event.metric_labels().is_empty(),
            "skipped events produce no metric"
        );
    }

    #[test]
    fn stage_timeout_event_fields() {
        let event = StageTimeout {
            nous_id: "test-agent".to_owned(),
            stage: "recall",
            timeout_secs: 30,
        };
        assert_eq!(event.log_level(), LogLevel::Warn);
        let labels = event.metric_labels();
        assert_eq!(labels.len(), 3);
    }
}
