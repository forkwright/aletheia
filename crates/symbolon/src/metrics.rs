//! Prometheus metric definitions for authentication and authorization.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{IntCounterVec, Opts, register_int_counter_vec};

static AUTH_ATTEMPTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_auth_attempts_total",
            "Total authentication attempts"
        ),
        &["method", "status"]
    )
    .expect("metric registration")
});

static TOKEN_REFRESHES_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_token_refreshes_total",
            "Total token refresh operations"
        ),
        &["status"]
    )
    .expect("metric registration")
});

static CREDENTIAL_WRITE_FAILURES_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
    prometheus::register_int_counter!(
        "aletheia_credential_write_failures_total",
        "Total credential file write failures"
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&AUTH_ATTEMPTS_TOTAL);
    LazyLock::force(&TOKEN_REFRESHES_TOTAL);
    LazyLock::force(&CREDENTIAL_WRITE_FAILURES_TOTAL);
}

/// Record an authentication attempt.
pub(crate) fn record_auth_attempt(method: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    AUTH_ATTEMPTS_TOTAL
        .with_label_values(&[method, status])
        .inc();
}

/// Record a token refresh operation.
pub(crate) fn record_token_refresh(success: bool) {
    let status = if success { "ok" } else { "error" };
    TOKEN_REFRESHES_TOTAL.with_label_values(&[status]).inc();
}

/// Record a credential file write failure.
pub(crate) fn record_credential_write_failure() {
    CREDENTIAL_WRITE_FAILURES_TOTAL.inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_auth_attempt_does_not_panic() {
        record_auth_attempt("password", true);
        record_auth_attempt("api_key", false);
    }

    #[test]
    fn record_token_refresh_does_not_panic() {
        record_token_refresh(true);
        record_token_refresh(false);
    }

    #[test]
    fn record_credential_write_failure_does_not_panic() {
        record_credential_write_failure();
    }
}
