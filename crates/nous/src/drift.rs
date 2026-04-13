//! Quality drift detection: tracks per-turn metrics and detects degradation.
//!
//! The [`DriftDetector`] maintains a rolling window of [`TurnMetrics`] and
//! compares recent averages against historical baselines. When the deviation
//! exceeds a configurable threshold, a [`DriftEvent`] is emitted via
//! `tracing::warn` and returned to the caller for session metadata storage.

use std::collections::VecDeque;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Quality metrics collected after each turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnMetrics {
    /// Output tokens produced by the model.
    pub response_tokens: u64,
    /// Fraction of tool calls that returned errors (0.0--1.0).
    pub tool_error_rate: f64,
    /// Whether the user's next message was classified as a correction.
    pub user_correction: bool,
    /// Total tool calls in this turn.
    pub tool_call_count: u32,
    /// Timestamp when the turn completed.
    pub timestamp: Timestamp,
}

/// Which metric triggered the drift detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DriftMetric {
    /// Response length (tokens) dropped significantly.
    ResponseLength,
    /// Tool error rate increased significantly.
    ToolErrorRate,
    /// User correction frequency increased significantly.
    UserCorrections,
}

impl std::fmt::Display for DriftMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResponseLength => f.write_str("response_length"),
            Self::ToolErrorRate => f.write_str("tool_error_rate"),
            Self::UserCorrections => f.write_str("user_corrections"),
        }
    }
}

/// A detected quality drift event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEvent {
    /// Which metric drifted.
    pub metric: DriftMetric,
    /// Current value (recent window average).
    pub current: f64,
    /// Historical baseline (full window average).
    pub baseline: f64,
    /// Standard deviation of the historical window.
    pub std_dev: f64,
    /// Z-score: how many standard deviations from the baseline.
    pub z_score: f64,
    /// When the drift was detected.
    pub detected_at: Timestamp,
}

/// Configuration for drift detection thresholds.
///
/// All defaults match the constants they replace so behaviour is identical
/// when the detector is constructed with `DriftConfig::default()`.
#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Number of turns in the rolling window. Default: 20.
    pub window_size: usize,
    /// Number of recent turns to compare against the window. Default: 5.
    pub recent_size: usize,
    /// Z-score threshold for triggering a drift event. Default: 2.0.
    pub deviation_threshold: f64,
    /// Minimum turns in the window before drift detection activates. Default: 8.
    ///
    /// WHY: with fewer samples the standard deviation is unreliable and would
    /// produce false positives.
    pub min_samples: usize,
}

impl Default for DriftConfig {
    fn default() -> Self {
        let b = taxis::config::AgentBehaviorDefaults::default();
        Self {
            window_size: b.drift_window_size,
            recent_size: b.drift_recent_size,
            deviation_threshold: b.drift_deviation_threshold,
            min_samples: b.drift_min_samples,
        }
    }
}

impl DriftConfig {
    /// Build from a resolved agent behavior config.
    #[must_use]
    pub fn from_behavior(behavior: &taxis::config::AgentBehaviorDefaults) -> Self {
        Self {
            window_size: behavior.drift_window_size,
            recent_size: behavior.drift_recent_size,
            deviation_threshold: behavior.drift_deviation_threshold,
            min_samples: behavior.drift_min_samples,
        }
    }
}

/// Rolling-window quality drift detector.
///
/// Accumulates [`TurnMetrics`] and compares the most recent `recent_size`
/// turns against the full window. When a metric's z-score exceeds the
/// configured threshold, a [`DriftEvent`] is produced and logged at warn
/// level.
pub struct DriftDetector {
    window: VecDeque<TurnMetrics>,
    config: DriftConfig,
}

impl DriftDetector {
    /// Create a new drift detector with the given configuration.
    #[must_use]
    pub fn new(config: DriftConfig) -> Self {
        Self {
            window: VecDeque::with_capacity(config.window_size),
            config,
        }
    }

    /// Record a turn's metrics and check for drift.
    ///
    /// Returns any drift events detected after incorporating this turn.
    pub fn record(&mut self, metrics: TurnMetrics) -> Vec<DriftEvent> {
        self.window.push_back(metrics);

        // WHY: cap the window to avoid unbounded memory growth
        while self.window.len() > self.config.window_size {
            self.window.pop_front();
        }

        tracing::debug!(
            window_len = self.window.len(),
            min_samples = self.config.min_samples,
            "drift record: checking threshold"
        );

        if self.window.len() < self.config.min_samples {
            return Vec::new();
        }

        let mut events = Vec::new();

        if let Some(event) = self.check_response_length() {
            warn!(
                metric = %event.metric,
                current = event.current,
                baseline = event.baseline,
                z_score = format!("{:.2}", event.z_score),
                "quality drift detected: response length degradation"
            );
            events.push(event);
        }

        if let Some(event) = self.check_tool_error_rate() {
            warn!(
                metric = %event.metric,
                current = event.current,
                baseline = event.baseline,
                z_score = format!("{:.2}", event.z_score),
                "quality drift detected: tool error rate increase"
            );
            events.push(event);
        }

        if let Some(event) = self.check_user_corrections() {
            warn!(
                metric = %event.metric,
                current = event.current,
                baseline = event.baseline,
                z_score = format!("{:.2}", event.z_score),
                "quality drift detected: user correction frequency increase"
            );
            events.push(event);
        }

        events
    }

    /// Number of turns currently in the window.
    #[must_use]
    pub fn turn_count(&self) -> usize {
        self.window.len()
    }

    /// Clear all recorded metrics.
    pub fn reset(&mut self) {
        self.window.clear();
    }

    /// Compare recent values against the older baseline portion of the window.
    ///
    /// Splits the window at `window.len() - recent_size` so the baseline
    /// statistics exclude the recent entries being tested. This prevents
    /// the recent values from pulling the mean toward themselves and
    /// masking real drift.
    ///
    /// `higher_is_worse`: when `true` (e.g. error rates), a positive
    /// deviation triggers drift. When `false` (e.g. response length), a
    /// negative deviation (drop) triggers drift.
    fn check_metric(
        &self,
        values: &[f64],
        metric: DriftMetric,
        higher_is_worse: bool,
    ) -> Option<DriftEvent> {
        let n = values.len();
        let recent_size = self.config.recent_size.min(n);
        let baseline_end = n.saturating_sub(recent_size);

        // WHY: need enough baseline samples for meaningful statistics
        if baseline_end < self.config.min_samples.saturating_sub(self.config.recent_size) {
            return None;
        }
        if baseline_end == 0 {
            return None;
        }

        // SAFETY: baseline_end > 0 checked above, and baseline_end <= n
        #[expect(clippy::indexing_slicing, reason = "baseline_end > 0 and <= values.len()")]
        let baseline_slice = &values[..baseline_end];
        #[expect(clippy::indexing_slicing, reason = "baseline_end <= values.len()")]
        let recent_slice = &values[baseline_end..];

        let (baseline, std_dev) = mean_and_stddev(baseline_slice);
        let recent = slice_mean(recent_slice);

        // WHY: when baseline variance is zero (perfectly stable metrics) but
        // recent values differ, that is definitive drift. Use a synthetic
        // z-score of infinity to guarantee detection.
        let z_score = if std_dev < f64::EPSILON {
            let diff = if higher_is_worse {
                recent - baseline
            } else {
                baseline - recent
            };
            if diff > f64::EPSILON {
                f64::INFINITY
            } else {
                return None;
            }
        } else if higher_is_worse {
            // Rising metric (errors, corrections): recent > baseline is bad
            (recent - baseline) / std_dev
        } else {
            // Falling metric (response length): baseline > recent is bad
            (baseline - recent) / std_dev
        };

        if z_score > self.config.deviation_threshold {
            Some(DriftEvent {
                metric,
                current: recent,
                baseline,
                std_dev,
                z_score,
                detected_at: Timestamp::now(),
            })
        } else {
            None
        }
    }

    /// Check for response length degradation.
    ///
    /// A significant drop in output tokens indicates the model is producing
    /// shorter, potentially lower-quality responses.
    fn check_response_length(&self) -> Option<DriftEvent> {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "u64→f64: per-turn response token counts are far below f64 mantissa precision"
        )]
        let values: Vec<f64> = self
            .window
            .iter()
            .map(|m| m.response_tokens as f64) // kanon:ignore RUST/as-cast
            .collect();

        self.check_metric(&values, DriftMetric::ResponseLength, false)
    }

    /// Check for rising tool error rate.
    fn check_tool_error_rate(&self) -> Option<DriftEvent> {
        let values: Vec<f64> = self.window.iter().map(|m| m.tool_error_rate).collect();
        self.check_metric(&values, DriftMetric::ToolErrorRate, true)
    }

    /// Check for increasing user correction frequency.
    fn check_user_corrections(&self) -> Option<DriftEvent> {
        let values: Vec<f64> = self
            .window
            .iter()
            .map(|m| if m.user_correction { 1.0 } else { 0.0 })
            .collect();
        self.check_metric(&values, DriftMetric::UserCorrections, true)
    }
}

impl Default for DriftDetector {
    fn default() -> Self {
        Self::new(DriftConfig::default())
    }
}

/// Compute mean and population standard deviation of a slice.
///
/// Returns `(0.0, 0.0)` for empty slices.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize→f64: drift window sizes are bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn mean_and_stddev(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }

    let n = values.len() as f64; // kanon:ignore RUST/as-cast
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

/// Compute mean of a slice.
///
/// Returns `0.0` if the slice is empty.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize→f64: drift window sizes are bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn slice_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let len = values.len() as f64; // kanon:ignore RUST/as-cast
    values.iter().sum::<f64>() / len
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_metrics(response_tokens: u64, tool_error_rate: f64, user_correction: bool) -> TurnMetrics {
        TurnMetrics {
            response_tokens,
            tool_error_rate,
            user_correction,
            tool_call_count: 3,
            timestamp: Timestamp::now(),
        }
    }

    #[test]
    fn no_drift_with_insufficient_samples() {
        let mut detector = DriftDetector::default();
        for _ in 0..5 {
            let events = detector.record(make_metrics(500, 0.1, false));
            assert!(events.is_empty(), "should not detect drift with few samples");
        }
    }

    #[test]
    fn no_drift_with_stable_metrics() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        // Record 20 turns of stable metrics
        for _ in 0..20 {
            let events = detector.record(make_metrics(500, 0.1, false));
            assert!(events.is_empty(), "stable metrics should not trigger drift");
        }
    }

    #[test]
    fn detects_response_length_drop() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        // Build a baseline of normal responses
        for _ in 0..15 {
            detector.record(make_metrics(500, 0.1, false));
        }

        // Sudden drop in response length
        let mut drift_detected = false;
        for _ in 0..5 {
            let events = detector.record(make_metrics(50, 0.1, false));
            if events.iter().any(|e| e.metric == DriftMetric::ResponseLength) {
                drift_detected = true;
            }
        }
        assert!(drift_detected, "should detect response length degradation");
    }

    #[test]
    fn detects_tool_error_rate_increase() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        // Build a baseline of low error rate
        for _ in 0..15 {
            detector.record(make_metrics(500, 0.05, false));
        }

        // Sudden spike in tool errors
        let mut drift_detected = false;
        for _ in 0..5 {
            let events = detector.record(make_metrics(500, 0.8, false));
            if events.iter().any(|e| e.metric == DriftMetric::ToolErrorRate) {
                drift_detected = true;
            }
        }
        assert!(drift_detected, "should detect tool error rate increase");
    }

    #[test]
    fn detects_user_correction_increase() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        // Build a baseline with no corrections
        for _ in 0..15 {
            detector.record(make_metrics(500, 0.1, false));
        }

        // Sudden correction spike
        let mut drift_detected = false;
        for _ in 0..5 {
            let events = detector.record(make_metrics(500, 0.1, true));
            if events.iter().any(|e| e.metric == DriftMetric::UserCorrections) {
                drift_detected = true;
            }
        }
        assert!(drift_detected, "should detect user correction increase");
    }

    #[test]
    fn window_caps_at_configured_size() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 10,
            recent_size: 3,
            deviation_threshold: 2.0,
            min_samples: 3,
        });

        for _ in 0..25 {
            detector.record(make_metrics(500, 0.1, false));
        }

        assert_eq!(detector.turn_count(), 10, "window should cap at window_size");
    }

    #[test]
    fn reset_clears_window() {
        let mut detector = DriftDetector::default();
        for _ in 0..10 {
            detector.record(make_metrics(500, 0.1, false));
        }
        assert!(detector.turn_count() > 0);
        detector.reset();
        assert_eq!(detector.turn_count(), 0, "reset should clear all metrics");
    }

    #[test]
    fn drift_event_contains_correct_metric() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        for _ in 0..15 {
            detector.record(make_metrics(500, 0.05, false));
        }

        // Spike error rate to trigger drift
        let mut events_seen = Vec::new();
        for _ in 0..5 {
            let events = detector.record(make_metrics(500, 0.9, false));
            events_seen.extend(events);
        }

        let tool_drift = events_seen
            .iter()
            .find(|e| e.metric == DriftMetric::ToolErrorRate);
        assert!(tool_drift.is_some(), "should have a tool error rate drift event");
        let event = tool_drift.unwrap(); // kanon:ignore RUST/unwrap
        assert!(event.current > event.baseline, "current should exceed baseline for error rate");
        assert!(event.z_score > 0.0, "z-score should be positive for increases");
    }

    #[test]
    fn no_drift_on_zero_variance() {
        let mut detector = DriftDetector::new(DriftConfig {
            window_size: 20,
            recent_size: 5,
            deviation_threshold: 2.0,
            ..DriftConfig::default()
        });

        // All identical metrics: zero variance, cannot compute z-score
        for _ in 0..20 {
            let events = detector.record(make_metrics(500, 0.1, false));
            assert!(events.is_empty(), "zero variance should not trigger drift");
        }
    }

    #[test]
    fn drift_metric_display() {
        assert_eq!(DriftMetric::ResponseLength.to_string(), "response_length");
        assert_eq!(DriftMetric::ToolErrorRate.to_string(), "tool_error_rate");
        assert_eq!(DriftMetric::UserCorrections.to_string(), "user_corrections");
    }

    #[test]
    fn drift_event_serialization_roundtrip() {
        let event = DriftEvent {
            metric: DriftMetric::ResponseLength,
            current: 100.0,
            baseline: 500.0,
            std_dev: 50.0,
            z_score: 8.0,
            detected_at: Timestamp::now(),
        };
        let json = serde_json::to_string(&event).expect("serialize drift event"); // kanon:ignore RUST/expect
        let back: DriftEvent = serde_json::from_str(&json).expect("deserialize drift event"); // kanon:ignore RUST/expect
        assert_eq!(back.metric, DriftMetric::ResponseLength);
        assert!((back.current - 100.0).abs() < f64::EPSILON);
        assert!((back.baseline - 500.0).abs() < f64::EPSILON);
    }

    // --- mean_and_stddev ---

    #[test]
    fn mean_and_stddev_empty() {
        let (mean, stddev) = mean_and_stddev(&[]);
        assert!((mean).abs() < f64::EPSILON);
        assert!((stddev).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_and_stddev_single_value() {
        let (mean, stddev) = mean_and_stddev(&[42.0]);
        assert!((mean - 42.0).abs() < f64::EPSILON);
        assert!((stddev).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_and_stddev_known_values() {
        // [2, 4, 4, 4, 5, 5, 7, 9] -> mean=5, stddev=2
        let values = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let (mean, stddev) = mean_and_stddev(&values);
        assert!((mean - 5.0).abs() < f64::EPSILON, "mean should be 5.0, got {mean}");
        assert!((stddev - 2.0).abs() < f64::EPSILON, "stddev should be 2.0, got {stddev}");
    }

    // --- slice_mean ---

    #[test]
    fn slice_mean_full_slice() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        let result = slice_mean(&values);
        assert!((result - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn slice_mean_single_value() {
        let result = slice_mean(&[42.0]);
        assert!((result - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn slice_mean_empty() {
        let result = slice_mean(&[]);
        assert!((result).abs() < f64::EPSILON);
    }

    #[test]
    fn slice_mean_two_values() {
        let values = [10.0, 20.0];
        let result = slice_mean(&values);
        assert!((result - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn default_detector_config() {
        let c = DriftConfig::default();
        let detector = DriftDetector::default();
        assert_eq!(detector.turn_count(), 0);
        assert_eq!(detector.config.window_size, c.window_size);
        assert_eq!(detector.config.recent_size, c.recent_size);
        assert!((detector.config.deviation_threshold - c.deviation_threshold).abs() < f64::EPSILON);
        assert_eq!(detector.config.min_samples, c.min_samples);
    }
}
