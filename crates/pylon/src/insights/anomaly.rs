//! Anomaly detection over rolling windows.

use crate::types::insights::{AnomalyAlert, TimeSeriesPoint};

/// Rolling window size for baseline computation (days).
const ROLLING_WINDOW_DAYS: usize = 14;

/// Z-score threshold for flagging an anomaly.
const Z_SCORE_THRESHOLD: f64 = 2.0;

/// Convert `usize` to `f64` losslessly for values that fit in `u32`.
fn usize_to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Detect anomalies in a single metric's time series.
///
/// Uses the last `ROLLING_WINDOW_DAYS` values to compute mean and standard
/// deviation. If the latest value exceeds `Z_SCORE_THRESHOLD` standard
/// deviations from the mean, an alert is generated.
#[must_use]
pub fn detect_anomalies(
    agent_id: &str,
    agent_name: &str,
    metric_name: &str,
    series: &[TimeSeriesPoint],
) -> Vec<AnomalyAlert> {
    let values: Vec<f64> = series.iter().map(|p| p.value).collect();
    let n = values.len();
    if n < 2 {
        return Vec::new();
    }

    let window_start = n.saturating_sub(ROLLING_WINDOW_DAYS);
    let window = values.get(window_start..n).unwrap_or(&[]);
    let window_len = window.len();
    if window_len < 2 {
        return Vec::new();
    }

    let mean = window.iter().sum::<f64>() / usize_to_f64(window_len);
    let variance =
        window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / usize_to_f64(window_len);
    let std_dev = variance.sqrt();

    if std_dev < f64::EPSILON {
        return Vec::new();
    }

    let latest = *values.last().unwrap_or(&0.0);
    let z_score = (latest - mean) / std_dev;

    if z_score.abs() < Z_SCORE_THRESHOLD {
        return Vec::new();
    }

    let deviation_pct = if mean.abs() > f64::EPSILON {
        ((latest - mean) / mean) * 100.0
    } else {
        0.0
    };

    let direction = if latest > mean {
        "up".to_owned()
    } else {
        "down".to_owned()
    };

    vec![AnomalyAlert {
        agent_id: agent_id.to_owned(),
        agent_name: agent_name.to_owned(),
        metric_name: metric_name.to_owned(),
        current_value: latest,
        baseline_mean: mean,
        deviation_pct,
        direction,
    }]
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn point(date: &str, value: f64) -> TimeSeriesPoint {
        TimeSeriesPoint {
            date: date.to_owned(),
            value,
        }
    }

    #[test]
    fn empty_series_returns_no_alerts() {
        let alerts = detect_anomalies("a", "A", "m", &[]);
        assert!(alerts.is_empty());
    }

    #[test]
    fn flat_series_returns_no_alerts() {
        let series = vec![
            point("2026-05-01", 10.0),
            point("2026-05-02", 10.0),
            point("2026-05-03", 10.0),
        ];
        let alerts = detect_anomalies("a", "A", "m", &series);
        assert!(alerts.is_empty());
    }

    #[test]
    fn spike_within_threshold_returns_no_alerts() {
        let series = vec![
            point("2026-05-01", 10.0),
            point("2026-05-02", 11.0),
            point("2026-05-03", 10.0),
        ];
        let alerts = detect_anomalies("a", "A", "m", &series);
        assert!(alerts.is_empty());
    }

    #[test]
    fn strong_spike_triggers_alert() {
        // WHY: need at least 5 baseline values for z-score > 2 with a single outlier.
        let series = vec![
            point("2026-05-01", 10.0),
            point("2026-05-02", 10.0),
            point("2026-05-03", 10.0),
            point("2026-05-04", 10.0),
            point("2026-05-05", 10.0),
            point("2026-05-06", 50.0),
        ];
        let alerts = detect_anomalies("a", "A", "m", &series);
        assert_eq!(alerts.len(), 1);
        let alert = alerts.first().expect("one alert");
        assert_eq!(alert.metric_name, "m");
        assert!((alert.current_value - 50.0).abs() < f64::EPSILON);
        assert!(alert.deviation_pct > 0.0);
        assert_eq!(alert.direction, "up");
    }

    #[test]
    fn strong_drop_triggers_alert() {
        let series = vec![
            point("2026-05-01", 10.0),
            point("2026-05-02", 10.0),
            point("2026-05-03", 10.0),
            point("2026-05-04", 10.0),
            point("2026-05-05", 10.0),
            point("2026-05-06", 1.0),
        ];
        let alerts = detect_anomalies("a", "A", "m", &series);
        assert_eq!(alerts.len(), 1);
        let alert = alerts.first().expect("one alert");
        assert_eq!(alert.direction, "down");
    }
}
