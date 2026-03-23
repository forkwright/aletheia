//! Prometheus metric definitions for the channel registry.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{IntCounterVec, IntGauge, Opts, register_int_counter_vec, register_int_gauge};

static CHANNEL_MESSAGES_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_channel_messages_total",
            "Total channel messages sent"
        ),
        &["channel_id", "status"]
    )
    .expect("metric registration")
});

static ACTIVE_SUBSCRIPTIONS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "aletheia_active_subscriptions",
        "Number of active channel subscriptions"
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&CHANNEL_MESSAGES_TOTAL);
    LazyLock::force(&ACTIVE_SUBSCRIPTIONS);
}

/// Record a channel message send.
pub(crate) fn record_channel_message(channel_id: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    CHANNEL_MESSAGES_TOTAL
        .with_label_values(&[channel_id, status])
        .inc();
}

/// Set the number of active subscriptions.
pub(crate) fn set_active_subscriptions(count: i64) {
    ACTIVE_SUBSCRIPTIONS.set(count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_channel_message_does_not_panic() {
        record_channel_message("signal-main", true);
        record_channel_message("signal-main", false);
    }

    #[test]
    fn set_active_subscriptions_does_not_panic() {
        set_active_subscriptions(3);
        set_active_subscriptions(0);
    }
}
