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

#[expect(dead_code, reason = "metric init called from server startup")]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
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
    fn init_registers_all_metrics() {
        init();
        // Verify metrics are registered by accessing them
        let _ = CHANNEL_MESSAGES_TOTAL.with_label_values(&["test", "ok"]).get();
        let _ = ACTIVE_SUBSCRIPTIONS.get();
    }

    #[test]
    fn record_channel_message_increments_counter() {
        let channel_id = "test-channel";
        let ok_before = CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "ok"]).get();
        let error_before = CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "error"]).get();

        record_channel_message(channel_id, true);
        assert_eq!(
            CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "ok"]).get(),
            ok_before + 1,
            "ok counter should increment for success=true"
        );
        assert_eq!(
            CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "error"]).get(),
            error_before,
            "error counter should not change for success=true"
        );

        record_channel_message(channel_id, false);
        assert_eq!(
            CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "ok"]).get(),
            ok_before + 1,
            "ok counter should be unchanged after error"
        );
        assert_eq!(
            CHANNEL_MESSAGES_TOTAL.with_label_values(&[channel_id, "error"]).get(),
            error_before + 1,
            "error counter should increment for success=false"
        );
    }

    #[test]
    fn set_active_subscriptions_updates_gauge() {
        set_active_subscriptions(3);
        assert_eq!(
            ACTIVE_SUBSCRIPTIONS.get(),
            3,
            "gauge should be set to 3"
        );

        set_active_subscriptions(10);
        assert_eq!(
            ACTIVE_SUBSCRIPTIONS.get(),
            10,
            "gauge should be updated to 10"
        );

        set_active_subscriptions(0);
        assert_eq!(
            ACTIVE_SUBSCRIPTIONS.get(),
            0,
            "gauge should be resettable to 0"
        );
    }
}
