//! Pattern-based stuck detection for tool execution loops.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

const DEFAULT_HISTORY_WINDOW: usize = 20;
const DEFAULT_REPEATED_ERROR_THRESHOLD: u32 = 3;
const DEFAULT_SAME_ARGS_THRESHOLD: u32 = 3;
const DEFAULT_ALTERNATING_THRESHOLD: u32 = 3;
const DEFAULT_ESCALATING_RETRY_THRESHOLD: u32 = 3;

/// Configuration for stuck detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StuckConfig {
    /// How many identical consecutive errors trigger detection.
    pub repeated_error_threshold: u32,
    /// How many identical consecutive tool+args calls trigger detection.
    pub same_args_threshold: u32,
    /// How many alternating failure cycles trigger detection.
    pub alternating_threshold: u32,
    /// How many scattered retries with the same error trigger detection.
    pub escalating_retry_threshold: u32,
    /// Maximum number of invocations retained in the sliding window.
    pub history_window: usize,
}

impl Default for StuckConfig {
    fn default() -> Self {
        Self {
            repeated_error_threshold: DEFAULT_REPEATED_ERROR_THRESHOLD,
            same_args_threshold: DEFAULT_SAME_ARGS_THRESHOLD,
            alternating_threshold: DEFAULT_ALTERNATING_THRESHOLD,
            escalating_retry_threshold: DEFAULT_ESCALATING_RETRY_THRESHOLD,
            history_window: DEFAULT_HISTORY_WINDOW,
        }
    }
}

/// The outcome of a tool invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InvocationOutcome {
    /// Tool executed successfully.
    Success,
    /// Tool execution failed with an error message.
    Error {
        /// The error message from the failed invocation.
        message: String,
    },
}

/// A recorded tool invocation for pattern analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// Name of the tool that was invoked.
    pub tool_name: String,
    /// Serialized arguments for identity comparison.
    pub arguments: String,
    /// Whether the invocation succeeded or failed.
    pub outcome: InvocationOutcome,
    /// When this invocation was recorded.
    pub recorded_at: jiff::Timestamp,
}

/// The type of stuck pattern detected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StuckPattern {
    /// The same error message appeared in the last N consecutive invocations.
    RepeatedError {
        /// The repeated error message.
        message: String,
        /// How many consecutive times the error appeared.
        count: u32,
    },
    /// The same tool was called with identical arguments N consecutive times.
    SameToolSameArgs {
        /// The tool that was called repeatedly.
        tool_name: String,
        /// How many consecutive times it was called with the same args.
        count: u32,
    },
    /// Two tools alternated back and forth, both failing.
    AlternatingFailure {
        /// First tool in the alternating pattern.
        tool_a: String,
        /// Second tool in the alternating pattern.
        tool_b: String,
        /// How many complete alternation cycles were detected.
        cycles: u32,
    },
    /// The same tool+args+error combination appeared scattered across the history window.
    EscalatingRetry {
        /// The tool being retried.
        tool_name: String,
        /// How many times the same failure was observed in the window.
        count: u32,
    },
}

/// A signal that the agent appears to be stuck, with a suggestion for intervention.
#[derive(Debug, Clone)]
pub struct StuckSignal {
    /// The pattern that triggered stuck detection.
    pub pattern: StuckPattern,
    /// A human-readable suggestion for how to intervene.
    pub suggestion: String,
}

/// Detects stuck patterns in a sliding window of tool invocations.
///
/// Records each tool call and checks for repeated errors, same-argument loops,
/// alternating failures, and escalating retry patterns.
#[derive(Debug)]
pub struct StuckDetector {
    config: StuckConfig,
    history: VecDeque<ToolInvocation>,
}

impl StuckDetector {
    /// Create a new detector with the given configuration.
    #[must_use]
    pub fn new(config: StuckConfig) -> Self {
        let capacity = config.history_window;
        Self {
            config,
            history: VecDeque::with_capacity(capacity),
        }
    }

    /// Record a tool invocation and check for stuck patterns.
    ///
    /// Returns `Some(StuckSignal)` if a stuck pattern is detected.
    pub fn record(&mut self, invocation: ToolInvocation) -> Option<StuckSignal> {
        self.history.push_back(invocation);
        while self.history.len() > self.config.history_window {
            self.history.pop_front();
        }
        self.check()
    }

    /// Check current history for stuck patterns without recording a new invocation.
    #[must_use]
    pub fn check(&self) -> Option<StuckSignal> {
        self.check_same_tool_same_args()
            .or_else(|| self.check_repeated_error())
            .or_else(|| self.check_alternating_failure())
            .or_else(|| self.check_escalating_retry())
    }

    /// Reset the detector, clearing all recorded history.
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Number of invocations currently in the history window.
    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// The active configuration.
    #[must_use]
    pub fn config(&self) -> &StuckConfig {
        &self.config
    }

    fn check_repeated_error(&self) -> Option<StuckSignal> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold values are small, no truncation possible"
        )]
        let threshold = self.config.repeated_error_threshold as usize;
        if self.history.len() < threshold {
            return None;
        }

        let tail: Vec<_> = self.history.iter().rev().take(threshold).collect();
        let first_message = match &tail.first()?.outcome {
            InvocationOutcome::Error { message } => message,
            InvocationOutcome::Success => return None,
        };

        let all_same = tail.iter().all(|inv| {
            matches!(
                &inv.outcome,
                InvocationOutcome::Error { message } if message == first_message
            )
        });

        if all_same {
            Some(StuckSignal {
                pattern: StuckPattern::RepeatedError {
                    message: first_message.clone(),
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_possible_truncation,
                        reason = "usize→u32: threshold originated as u32, roundtrip is lossless"
                    )]
                    count: threshold as u32,
                },
                suggestion: format!(
                    "The same error has occurred {threshold} times consecutively. \
                     Consider a different approach or ask for help."
                ),
            })
        } else {
            None
        }
    }

    fn check_same_tool_same_args(&self) -> Option<StuckSignal> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold values are small, no truncation possible"
        )]
        let threshold = self.config.same_args_threshold as usize;
        if self.history.len() < threshold {
            return None;
        }

        let tail: Vec<_> = self.history.iter().rev().take(threshold).collect();
        let first = tail.first()?;

        let all_same = tail
            .iter()
            .all(|inv| inv.tool_name == first.tool_name && inv.arguments == first.arguments);

        if all_same {
            Some(StuckSignal {
                pattern: StuckPattern::SameToolSameArgs {
                    tool_name: first.tool_name.clone(),
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_possible_truncation,
                        reason = "usize→u32: threshold originated as u32, roundtrip is lossless"
                    )]
                    count: threshold as u32,
                },
                suggestion: format!(
                    "Tool '{}' has been called with identical arguments {threshold} times. \
                     The approach is not working.",
                    first.tool_name
                ),
            })
        } else {
            None
        }
    }

    fn check_alternating_failure(&self) -> Option<StuckSignal> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold values are small, no truncation possible"
        )]
        let threshold = self.config.alternating_threshold as usize;
        let required = threshold.checked_mul(2)?;
        if self.history.len() < required {
            return None;
        }

        let tail: Vec<_> = self.history.iter().rev().take(required).collect();

        // WHY: all invocations must be errors for an alternating failure pattern
        if !tail
            .iter()
            .all(|inv| matches!(inv.outcome, InvocationOutcome::Error { .. }))
        {
            return None;
        }

        let a_inv = tail.first()?;
        let b_inv = tail.get(1)?;

        // The two pairs must be different tool+args combinations
        if a_inv.tool_name == b_inv.tool_name && a_inv.arguments == b_inv.arguments {
            return None;
        }

        let alternates = tail.iter().enumerate().all(|(i, inv)| {
            let expected = if i % 2 == 0 { a_inv } else { b_inv };
            inv.tool_name == expected.tool_name && inv.arguments == expected.arguments
        });

        if alternates {
            Some(StuckSignal {
                pattern: StuckPattern::AlternatingFailure {
                    tool_a: a_inv.tool_name.clone(),
                    tool_b: b_inv.tool_name.clone(),
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_possible_truncation,
                        reason = "usize→u32: threshold originated as u32, roundtrip is lossless"
                    )]
                    cycles: threshold as u32,
                },
                suggestion: format!(
                    "Alternating between '{}' and '{}' without progress for {threshold} cycles. \
                     Both approaches are failing.",
                    a_inv.tool_name, b_inv.tool_name
                ),
            })
        } else {
            None
        }
    }

    fn check_escalating_retry(&self) -> Option<StuckSignal> {
        #[expect(
            clippy::as_conversions,
            reason = "u32→usize: threshold values are small, no truncation possible"
        )]
        let threshold = self.config.escalating_retry_threshold as usize;
        if self.history.len() < threshold {
            return None;
        }

        // WHY: count occurrences of each (tool, args, error) triple across the full window
        // to find scattered retries that aren't necessarily consecutive
        let mut counts: Vec<(&str, &str, &str, usize)> = Vec::new();

        for inv in &self.history {
            if let InvocationOutcome::Error { message } = &inv.outcome {
                let found = counts.iter_mut().find(|(t, a, m, _)| {
                    *t == inv.tool_name && *a == inv.arguments && *m == message.as_str()
                });
                if let Some(entry) = found {
                    entry.3 += 1;
                } else {
                    counts.push((&inv.tool_name, &inv.arguments, message, 1));
                }
            }
        }

        let max_entry = counts.iter().max_by_key(|(_, _, _, count)| count)?;
        if max_entry.3 < threshold {
            return None;
        }

        // WHY: only trigger when retries are scattered (not consecutive), because consecutive
        // patterns are already caught by check_repeated_error / check_same_tool_same_args
        let matching_indices: Vec<usize> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, inv)| {
                inv.tool_name == max_entry.0
                    && inv.arguments == max_entry.1
                    && matches!(
                        &inv.outcome,
                        InvocationOutcome::Error { message } if message == max_entry.2
                    )
            })
            .map(|(i, _)| i)
            .collect();

        let first = matching_indices.first()?;
        let last = matching_indices.last()?;
        let span = last - first;
        let has_gap = span > matching_indices.len() - 1;

        if has_gap {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "usize→u32: match count bounded by history_window which fits in u32"
            )]
            Some(StuckSignal {
                pattern: StuckPattern::EscalatingRetry {
                    tool_name: max_entry.0.to_string(),
                    count: max_entry.3 as u32,
                },
                suggestion: format!(
                    "Tool '{}' has been retried {} times across the history window with the same error. \
                     Consider changing strategy.",
                    max_entry.0, max_entry.3
                ),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_invocation(tool: &str, args: &str, outcome: InvocationOutcome) -> ToolInvocation {
        ToolInvocation {
            tool_name: tool.into(),
            arguments: args.into(),
            outcome,
            recorded_at: jiff::Timestamp::now(),
        }
    }

    fn error_outcome(msg: &str) -> InvocationOutcome {
        InvocationOutcome::Error {
            message: msg.into(),
        }
    }

    #[test]
    fn default_config_values() {
        let config = StuckConfig::default();
        assert_eq!(config.repeated_error_threshold, 3);
        assert_eq!(config.same_args_threshold, 3);
        assert_eq!(config.alternating_threshold, 3);
        assert_eq!(config.escalating_retry_threshold, 3);
        assert_eq!(config.history_window, 20);
    }

    #[test]
    fn no_stuck_on_empty_history() {
        let detector = StuckDetector::new(StuckConfig::default());
        assert!(detector.check().is_none());
    }

    #[test]
    fn no_stuck_on_varied_success_sequence() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for i in 0..5 {
            let result = detector.record(make_invocation(
                &format!("tool_{i}"),
                &format!("/path/{i}"),
                InvocationOutcome::Success,
            ));
            assert!(result.is_none());
        }
    }

    #[test]
    fn detects_repeated_error() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for i in 0..3 {
            let result = detector.record(make_invocation(
                &format!("tool_{i}"),
                &format!("args_{i}"),
                error_outcome("connection refused"),
            ));
            if i < 2 {
                assert!(result.is_none());
            } else {
                let signal = result.unwrap();
                assert_eq!(
                    signal.pattern,
                    StuckPattern::RepeatedError {
                        message: "connection refused".into(),
                        count: 3,
                    }
                );
            }
        }
    }

    #[test]
    fn detects_same_tool_same_args() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for i in 0..3 {
            let result = detector.record(make_invocation(
                "read_file",
                "/etc/hosts",
                InvocationOutcome::Success,
            ));
            if i < 2 {
                assert!(result.is_none());
            } else {
                let signal = result.unwrap();
                assert_eq!(
                    signal.pattern,
                    StuckPattern::SameToolSameArgs {
                        tool_name: "read_file".into(),
                        count: 3,
                    }
                );
            }
        }
    }

    #[test]
    fn same_tool_different_args_not_stuck() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for i in 0..5 {
            let result = detector.record(make_invocation(
                "read_file",
                &format!("/path/{i}"),
                InvocationOutcome::Success,
            ));
            assert!(result.is_none());
        }
    }

    #[test]
    fn detects_alternating_failure() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        let mut last_result = None;
        // WHY: each tool gets a distinct error message so repeated_error doesn't fire first
        for i in 0..6 {
            let tool = if i % 2 == 0 { "compile" } else { "lint" };
            let args = if i % 2 == 0 {
                "src/main.rs"
            } else {
                "src/lib.rs"
            };
            let err = if i % 2 == 0 {
                "syntax error"
            } else {
                "lint violation"
            };
            last_result = detector.record(make_invocation(tool, args, error_outcome(err)));
        }
        let signal = last_result.unwrap();
        assert!(matches!(
            signal.pattern,
            StuckPattern::AlternatingFailure { .. }
        ));
    }

    #[test]
    fn alternating_with_success_not_stuck() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for i in 0..6 {
            let tool = if i % 2 == 0 { "compile" } else { "lint" };
            let outcome = if i == 3 {
                InvocationOutcome::Success
            } else {
                error_outcome("failed")
            };
            let result = detector.record(make_invocation(tool, "args", outcome));
            // WHY: a success breaks the alternating failure pattern
            if i >= 3 {
                assert!(
                    !matches!(
                        result.as_ref().map(|s| &s.pattern),
                        Some(StuckPattern::AlternatingFailure { .. })
                    ),
                    "should not detect alternating failure when success is present"
                );
            }
        }
    }

    #[test]
    fn detects_escalating_retry() {
        let config = StuckConfig {
            escalating_retry_threshold: 3,
            // WHY: raise other thresholds so only escalating retry triggers
            same_args_threshold: 100,
            repeated_error_threshold: 100,
            alternating_threshold: 100,
            ..StuckConfig::default()
        };
        let mut detector = StuckDetector::new(config);

        // Scattered retries with different tools in between
        detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));
        detector.record(make_invocation(
            "check_status",
            "prod",
            InvocationOutcome::Success,
        ));
        detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));
        detector.record(make_invocation(
            "read_logs",
            "prod",
            InvocationOutcome::Success,
        ));
        let result = detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));

        let signal = result.unwrap();
        assert!(matches!(
            signal.pattern,
            StuckPattern::EscalatingRetry { .. }
        ));
    }

    #[test]
    fn consecutive_errors_not_escalating() {
        let config = StuckConfig {
            escalating_retry_threshold: 3,
            // WHY: raise same_args threshold so it doesn't fire first
            same_args_threshold: 100,
            repeated_error_threshold: 100,
            ..StuckConfig::default()
        };
        let mut detector = StuckDetector::new(config);

        // Three consecutive identical errors — no gap, so escalating retry should not trigger
        detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));
        detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));
        let result = detector.record(make_invocation("deploy", "prod", error_outcome("timeout")));

        assert!(result.is_none());
    }

    #[test]
    fn custom_thresholds() {
        let config = StuckConfig {
            repeated_error_threshold: 5,
            same_args_threshold: 5,
            ..StuckConfig::default()
        };
        let mut detector = StuckDetector::new(config);

        // 3 identical calls should not trigger with threshold of 5
        for _ in 0..3 {
            let result = detector.record(make_invocation(
                "read_file",
                "/same",
                error_outcome("not found"),
            ));
            assert!(result.is_none());
        }
    }

    #[test]
    fn reset_clears_history() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        detector.record(make_invocation("tool", "args", error_outcome("error")));
        detector.record(make_invocation("tool", "args", error_outcome("error")));
        assert_eq!(detector.history_len(), 2);

        detector.reset();
        assert_eq!(detector.history_len(), 0);
        assert!(detector.check().is_none());
    }

    #[test]
    fn history_window_eviction() {
        let config = StuckConfig {
            history_window: 3,
            ..StuckConfig::default()
        };
        let mut detector = StuckDetector::new(config);

        for i in 0..5 {
            detector.record(make_invocation(
                &format!("tool_{i}"),
                "args",
                InvocationOutcome::Success,
            ));
        }
        assert_eq!(detector.history_len(), 3);
    }

    #[test]
    fn same_tool_same_args_has_priority_over_repeated_error() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        // WHY: same tool + same args + same error should trigger SameToolSameArgs first
        // because it's checked before RepeatedError in the priority chain
        for _ in 0..3 {
            detector.record(make_invocation(
                "compile",
                "src/main.rs",
                error_outcome("syntax error"),
            ));
        }
        let signal = detector.check().unwrap();
        assert!(matches!(
            signal.pattern,
            StuckPattern::SameToolSameArgs { .. }
        ));
    }

    #[test]
    fn below_threshold_no_signal() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        detector.record(make_invocation("tool", "args", error_outcome("error")));
        detector.record(make_invocation("tool", "args", error_outcome("error")));
        // Only 2 of threshold 3
        assert!(detector.check().is_none());
    }

    #[test]
    fn mixed_errors_not_repeated() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        detector.record(make_invocation("tool", "args", error_outcome("error A")));
        detector.record(make_invocation("tool", "args", error_outcome("error B")));
        detector.record(make_invocation("tool", "args", error_outcome("error A")));
        // Different error messages break the repeated error pattern
        // But same tool+args triggers SameToolSameArgs
        let signal = detector.check().unwrap();
        assert!(matches!(
            signal.pattern,
            StuckPattern::SameToolSameArgs { .. }
        ));
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = StuckConfig {
            repeated_error_threshold: 5,
            same_args_threshold: 4,
            alternating_threshold: 6,
            escalating_retry_threshold: 7,
            history_window: 50,
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: StuckConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repeated_error_threshold, 5);
        assert_eq!(back.same_args_threshold, 4);
        assert_eq!(back.alternating_threshold, 6);
        assert_eq!(back.escalating_retry_threshold, 7);
        assert_eq!(back.history_window, 50);
    }

    #[test]
    fn invocation_serde_roundtrip() {
        let inv = make_invocation("read_file", "/path/to/file", error_outcome("not found"));
        let json = serde_json::to_string(&inv).unwrap();
        let back: ToolInvocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_name, "read_file");
        assert_eq!(back.arguments, "/path/to/file");
        assert_eq!(
            back.outcome,
            InvocationOutcome::Error {
                message: "not found".into()
            }
        );
    }

    #[test]
    fn signal_suggestion_is_nonempty() {
        let mut detector = StuckDetector::new(StuckConfig::default());
        for _ in 0..3 {
            detector.record(make_invocation("tool", "args", error_outcome("error")));
        }
        let signal = detector.check().unwrap();
        assert!(!signal.suggestion.is_empty());
    }
}
