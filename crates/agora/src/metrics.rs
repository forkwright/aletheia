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

// ---------------------------------------------------------------------------
// Label sets
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ChannelMessageLabels {
    channel_id: String,
    status: String,
}

// ---------------------------------------------------------------------------
// Metric families
// ---------------------------------------------------------------------------

static CHANNEL_MESSAGES_TOTAL: LazyLock<Family<ChannelMessageLabels, Counter>> =
    LazyLock::new(Family::default);

static ACTIVE_SUBSCRIPTIONS: LazyLock<Gauge> = LazyLock::new(Gauge::default);

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

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
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

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
        let r = fresh_registry();
        set_active_subscriptions(42);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_active_subscriptions 42"),
            "got: {out}"
        );
    }
}
