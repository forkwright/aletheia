//! Prometheus metric definitions for the channel registry.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ChannelMessageLabels {
    channel_id: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProviderFailureLabels {
    channel_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct HandlerFailureLabels {
    channel_id: String,
}

// ── Metric families ──

static CHANNEL_MESSAGES_TOTAL: LazyLock<Family<ChannelMessageLabels, Counter>> =
    LazyLock::new(Family::default);

static ACTIVE_SUBSCRIPTIONS: LazyLock<Gauge> = LazyLock::new(Gauge::default);

static PROVIDER_FAILURES_TOTAL: LazyLock<Family<ProviderFailureLabels, Counter>> =
    LazyLock::new(Family::default);

static HANDLER_FAILURES_TOTAL: LazyLock<Family<HandlerFailureLabels, Counter>> =
    LazyLock::new(Family::default);

// ── Registration ──

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_channel_messages",
        "Total channel messages sent",
        CHANNEL_MESSAGES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_active_subscriptions",
        "Number of active channel subscriptions",
        ACTIVE_SUBSCRIPTIONS.clone(),
    );
    registry.register(
        "aletheia_provider_failures",
        "Total provider polling task failures",
        PROVIDER_FAILURES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_handler_failures",
        "Total inbound-message handler task failures",
        HANDLER_FAILURES_TOTAL.clone(),
    );
}

// ── Recording ──

/// Record a channel message send.
pub(crate) fn record_channel_message(channel_id: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    CHANNEL_MESSAGES_TOTAL
        .get_or_create(&ChannelMessageLabels {
            channel_id: channel_id.to_owned(),
            status: status.to_owned(),
        })
        .inc();
}

/// Set the number of active subscriptions.
pub(crate) fn set_active_subscriptions(count: i64) {
    ACTIVE_SUBSCRIPTIONS.set(count);
}

/// Record a provider polling task failure.
pub(crate) fn record_provider_failure(channel_id: &str) {
    PROVIDER_FAILURES_TOTAL
        .get_or_create(&ProviderFailureLabels {
            channel_id: channel_id.to_owned(),
        })
        .inc();
}

/// Record an inbound-message handler task failure.
pub(crate) fn record_handler_failure(channel_id: &str) {
    HANDLER_FAILURES_TOTAL
        .get_or_create(&HandlerFailureLabels {
            channel_id: channel_id.to_owned(),
        })
        .inc();
}

/// Serializes tests that read or write `ACTIVE_SUBSCRIPTIONS` to prevent
/// cross-test gauge interference when the full test suite runs in parallel.
#[cfg(test)]
pub(crate) static GAUGE_TEST_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

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
    fn register_and_record_channel_message_success() {
        let r = fresh_registry();
        record_channel_message("_test_channel_ok", true);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_channel_messages_total{channel_id=\"_test_channel_ok\",status=\"ok\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_channel_message_failure() {
        let r = fresh_registry();
        record_channel_message("_test_channel_err", false);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_channel_messages_total{channel_id=\"_test_channel_err\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_set_active_subscriptions() {
        let _guard = super::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let r = fresh_registry();
        set_active_subscriptions(42);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_active_subscriptions 42"),
            "got: {out}"
        );
        set_active_subscriptions(0);
    }

    #[test]
    fn register_and_record_provider_failure() {
        let r = fresh_registry();
        record_provider_failure("_test_provider");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_provider_failures_total{channel_id=\"_test_provider\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_handler_failure() {
        let r = fresh_registry();
        record_handler_failure("_test_handler");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_handler_failures_total{channel_id=\"_test_handler\"} 1"),
            "got: {out}"
        );
    }
}
