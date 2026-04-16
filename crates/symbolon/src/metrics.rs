//! Prometheus metric definitions for authentication and authorization.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

// ---------------------------------------------------------------------------
// Label sets
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct AuthAttemptLabels {
    method: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct TokenRefreshLabels {
    status: String,
}

// ---------------------------------------------------------------------------
// Metric families
// ---------------------------------------------------------------------------

static AUTH_ATTEMPTS_TOTAL: LazyLock<Family<AuthAttemptLabels, Counter>> =
    LazyLock::new(Family::default);

static TOKEN_REFRESHES_TOTAL: LazyLock<Family<TokenRefreshLabels, Counter>> =
    LazyLock::new(Family::default);

static CREDENTIAL_WRITE_FAILURES_TOTAL: LazyLock<Counter> = LazyLock::new(Counter::default);

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_auth_attempts",
        "Total authentication attempts",
        AUTH_ATTEMPTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_token_refreshes",
        "Total token refresh operations",
        TOKEN_REFRESHES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_credential_write_failures",
        "Total credential file write failures",
        CREDENTIAL_WRITE_FAILURES_TOTAL.clone(),
    );
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Record an authentication attempt.
pub(crate) fn record_auth_attempt(method: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    AUTH_ATTEMPTS_TOTAL
        .get_or_create(&AuthAttemptLabels {
            method: method.to_owned(),
            status: status.to_owned(),
        })
        .inc();
}

/// Record a token refresh operation.
pub(crate) fn record_token_refresh(success: bool) {
    let status = if success { "ok" } else { "error" };
    TOKEN_REFRESHES_TOTAL
        .get_or_create(&TokenRefreshLabels {
            status: status.to_owned(),
        })
        .inc();
}

/// Record a credential file write failure.
pub(crate) fn record_credential_write_failure() {
    CREDENTIAL_WRITE_FAILURES_TOTAL.inc();
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    #[test]
    fn register_and_record_auth_attempt() {
        let r = fresh_registry();
        record_auth_attempt("_test_method", true);
        record_auth_attempt("_test_method", false);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_auth_attempts_total{method=\"_test_method\",status=\"ok\"} 1"),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_auth_attempts_total{method=\"_test_method\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_token_refresh() {
        let r = fresh_registry();
        record_token_refresh(true);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_token_refreshes_total{status=\"ok\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_credential_write_failure() {
        let r = fresh_registry();
        record_credential_write_failure();
        let out = encode(&r);
        assert!(
            out.contains("aletheia_credential_write_failures_total 1"),
            "got: {out}"
        );
    }
}
