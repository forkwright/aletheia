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

#[expect(dead_code, reason = "metric init called from server startup")]
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
    fn init_registers_all_metrics() {
        init();
        // Verify metrics are registered by accessing them
        let _ = AUTH_ATTEMPTS_TOTAL.with_label_values(&["test", "ok"]).get();
        let _ = TOKEN_REFRESHES_TOTAL.with_label_values(&["ok"]).get();
        let _ = CREDENTIAL_WRITE_FAILURES_TOTAL.get();
    }

    #[test]
    fn record_auth_attempt_increments_counter() {
        let method = "test-auth-method";
        let ok_before = AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "ok"]).get();
        let error_before = AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "error"]).get();

        record_auth_attempt(method, true);
        assert_eq!(
            AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "ok"]).get(),
            ok_before + 1,
            "ok counter should increment by 1"
        );
        assert_eq!(
            AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "error"]).get(),
            error_before,
            "error counter should not change for successful auth"
        );

        record_auth_attempt(method, false);
        assert_eq!(
            AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "ok"]).get(),
            ok_before + 1,
            "ok counter should be unchanged"
        );
        assert_eq!(
            AUTH_ATTEMPTS_TOTAL.with_label_values(&[method, "error"]).get(),
            error_before + 1,
            "error counter should increment by 1"
        );
    }

    #[test]
    fn record_token_refresh_increments_counter() {
        let ok_before = TOKEN_REFRESHES_TOTAL.with_label_values(&["ok"]).get();
        let error_before = TOKEN_REFRESHES_TOTAL.with_label_values(&["error"]).get();

        record_token_refresh(true);
        assert_eq!(
            TOKEN_REFRESHES_TOTAL.with_label_values(&["ok"]).get(),
            ok_before + 1,
            "ok counter should increment by 1"
        );
        assert_eq!(
            TOKEN_REFRESHES_TOTAL.with_label_values(&["error"]).get(),
            error_before,
            "error counter should not change for successful refresh"
        );

        record_token_refresh(false);
        assert_eq!(
            TOKEN_REFRESHES_TOTAL.with_label_values(&["ok"]).get(),
            ok_before + 1,
            "ok counter should be unchanged"
        );
        assert_eq!(
            TOKEN_REFRESHES_TOTAL.with_label_values(&["error"]).get(),
            error_before + 1,
            "error counter should increment by 1"
        );
    }

    #[test]
    fn record_credential_write_failure_increments_counter() {
        let before = CREDENTIAL_WRITE_FAILURES_TOTAL.get();
        record_credential_write_failure();
        assert_eq!(
            CREDENTIAL_WRITE_FAILURES_TOTAL.get(),
            before + 1,
            "credential write failure counter should increment by 1"
        );
    }
}
