//! Prometheus metric definitions for the HTTP gateway.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    Gauge, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, register_gauge,
    register_histogram_vec, register_int_counter_vec, register_int_gauge,
};

static HTTP_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_http_requests_total", "Total HTTP requests"),
        &["method", "path", "status"]
    )
    .expect("metric registration")
});

static HTTP_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_http_request_duration_seconds",
            "HTTP request duration in seconds"
        )
        .buckets(vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
        ]),
        &["method", "path"]
    )
    .expect("metric registration")
});

static ACTIVE_SESSIONS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!("aletheia_active_sessions", "Number of active sessions")
        .expect("metric registration")
});

static UPTIME_SECONDS: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!("aletheia_uptime_seconds", "Server uptime in seconds")
        .expect("metric registration")
});

/// Force-initialize all lazy metric statics so they register with the default prometheus registry.
pub fn init() {
    LazyLock::force(&HTTP_REQUESTS_TOTAL);
    LazyLock::force(&HTTP_REQUEST_DURATION_SECONDS);
    LazyLock::force(&ACTIVE_SESSIONS);
    LazyLock::force(&UPTIME_SECONDS);
}

/// Record an HTTP request metric.
pub fn record_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    let status_str = status.to_string();
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[method, path, &status_str])
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[method, path])
        .observe(duration_secs);
}

/// Update system gauge metrics.
pub fn update_system_gauges(uptime_secs: f64, active_sessions: i64) {
    UPTIME_SECONDS.set(uptime_secs);
    ACTIVE_SESSIONS.set(active_sessions);
}

/// Normalize a URL path by replacing dynamic segments with `{id}`.
///
/// Prevents label explosion from unique IDs in prometheus metrics.
pub fn normalize_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let normalized: Vec<&str> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i > 0 && looks_like_id(part) {
                "{id}"
            } else {
                part
            }
        })
        .collect();
    normalized.join("/")
}

fn looks_like_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // NOTE: ULIDs are 26 alphanumeric chars; UUIDs are 36 chars with hyphens.
    let len = s.len();
    (len >= 20 && s.chars().all(|c| c.is_ascii_alphanumeric()))
        || (len == 36 && s.contains('-') && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_static_paths() {
        assert_eq!(normalize_path("/api/health"), "/api/health");
        assert_eq!(normalize_path("/api/nous"), "/api/nous");
        assert_eq!(normalize_path("/metrics"), "/metrics");
    }

    #[test]
    fn normalize_dynamic_paths() {
        assert_eq!(
            normalize_path("/api/sessions/01JTEST1234567890ABCDEFGH"),
            "/api/sessions/{id}"
        );
        assert_eq!(
            normalize_path("/api/nous/01JTEST1234567890ABCDEFGH/tools"),
            "/api/nous/{id}/tools"
        );
    }

    #[test]
    fn normalize_uuid_paths() {
        assert_eq!(
            normalize_path("/api/sessions/550e8400-e29b-41d4-a716-446655440000"),
            "/api/sessions/{id}"
        );
    }

    #[test]
    fn normalize_short_names_preserved() {
        assert_eq!(normalize_path("/api/nous/syn"), "/api/nous/syn");
        assert_eq!(normalize_path("/api/nous/syn/tools"), "/api/nous/syn/tools");
    }
}
