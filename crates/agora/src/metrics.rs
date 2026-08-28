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

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CommandDeniedLabels {
    channel_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct IngressDuplicateLabels {
    channel_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CursorCheckpointLabels {
    channel_id: String,
}

// ── Metric families ──

static CHANNEL_MESSAGES_TOTAL: LazyLock<Family<ChannelMessageLabels, Counter>> =
    LazyLock::new(Family::default);

static ACTIVE_SUBSCRIPTIONS: LazyLock<Gauge> = LazyLock::new(Gauge::default);

/// Owns one contribution to the process-wide active-subscription gauge.
///
/// Provider tasks construct this guard when they begin polling. Completion,
/// cancellation, and panic all drop it with the task future, so the gauge
/// describes live provider-account work rather than listener handle ownership.
#[must_use = "dropping the guard immediately releases the subscription metric"]
pub(crate) struct ActiveSubscriptionGuard {
    gauge: Gauge,
}

impl ActiveSubscriptionGuard {
    pub(crate) fn new() -> Self {
        Self::for_gauge(ACTIVE_SUBSCRIPTIONS.clone())
    }

    fn for_gauge(gauge: Gauge) -> Self {
        gauge.inc();
        Self { gauge }
    }
}

impl Drop for ActiveSubscriptionGuard {
    fn drop(&mut self) {
        self.gauge.dec();
    }
}

static PROVIDER_FAILURES_TOTAL: LazyLock<Family<ProviderFailureLabels, Counter>> =
    LazyLock::new(Family::default);

static HANDLER_FAILURES_TOTAL: LazyLock<Family<HandlerFailureLabels, Counter>> =
    LazyLock::new(Family::default);

static COMMAND_DENIED_TOTAL: LazyLock<Family<CommandDeniedLabels, Counter>> =
    LazyLock::new(Family::default);

static INGRESS_DUPLICATES_TOTAL: LazyLock<Family<IngressDuplicateLabels, Counter>> =
    LazyLock::new(Family::default);

static CURSOR_CHECKPOINTS_TOTAL: LazyLock<Family<CursorCheckpointLabels, Counter>> =
    LazyLock::new(Family::default);

static INBOUND_HANDLERS_IN_FLIGHT: LazyLock<Gauge> = LazyLock::new(Gauge::default);

static INBOUND_HANDLER_SATURATION_TOTAL: LazyLock<Counter> = LazyLock::new(Counter::default);

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
    registry.register(
        "aletheia_command_denied",
        "Total inbound commands denied by the inbound command policy",
        COMMAND_DENIED_TOTAL.clone(),
    );
    registry.register(
        "aletheia_ingress_duplicates",
        "Total inbound messages dropped as duplicate deliveries",
        INGRESS_DUPLICATES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_cursor_checkpoints",
        "Total provider sync cursor checkpoints persisted",
        CURSOR_CHECKPOINTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_inbound_handlers_in_flight",
        "Inbound-message handler tasks currently running",
        INBOUND_HANDLERS_IN_FLIGHT.clone(),
    );
    registry.register(
        "aletheia_inbound_handler_saturation",
        "Total times inbound dispatch had to wait for a free handler slot",
        INBOUND_HANDLER_SATURATION_TOTAL.clone(),
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
#[cfg(test)]
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

/// Record an inbound `!`-command denied by the command policy.
///
/// `pub` because the enforcement point lives in the binary's dispatch layer,
/// outside this crate.
pub fn record_command_denied(channel_id: &str) {
    COMMAND_DENIED_TOTAL
        .get_or_create(&CommandDeniedLabels {
            channel_id: channel_id.to_owned(),
        })
        .inc();
}

/// Record an inbound message dropped as a duplicate delivery.
pub(crate) fn record_ingress_duplicate(channel_id: &str) {
    INGRESS_DUPLICATES_TOTAL
        .get_or_create(&IngressDuplicateLabels {
            channel_id: channel_id.to_owned(),
        })
        .inc();
}

/// Record a provider sync cursor checkpoint persisted to the cursor store.
pub(crate) fn record_cursor_checkpoint(channel_id: &str) {
    CURSOR_CHECKPOINTS_TOTAL
        .get_or_create(&CursorCheckpointLabels {
            channel_id: channel_id.to_owned(),
        })
        .inc();
}

/// Add one handler's delta to the number of inbound-message handler tasks.
///
/// Listeners contribute independently; setting an absolute value from a local
/// task set would let concurrent listeners overwrite one another.
fn add_inbound_handlers_in_flight(delta: i64) {
    if delta >= 0 {
        INBOUND_HANDLERS_IN_FLIGHT.inc_by(delta);
    } else {
        INBOUND_HANDLERS_IN_FLIGHT.dec_by(delta.saturating_neg());
    }
}

/// Owns one contribution to the process-wide in-flight handler gauge.
///
/// The guard travels with a spawned handler future, so completion, panic, and
/// task abortion all remove exactly the contribution they added. Deltas keep
/// concurrent listeners from overwriting one another's counts.
pub(crate) struct InboundHandlerGuard;

impl InboundHandlerGuard {
    pub(crate) fn new() -> Self {
        add_inbound_handlers_in_flight(1);
        Self
    }
}

impl Drop for InboundHandlerGuard {
    fn drop(&mut self) {
        add_inbound_handlers_in_flight(-1);
    }
}

/// Record that inbound dispatch had to wait for a free handler slot
/// (the concurrency cap was reached).
pub(crate) fn record_inbound_handler_saturation() {
    INBOUND_HANDLER_SATURATION_TOTAL.inc();
}

/// Serializes tests that read or write process-global gauges to prevent
/// cross-test interference when the full test suite runs in parallel.
#[cfg(test)]
pub(crate) static GAUGE_TEST_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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

    fn scalar_value(encoded: &str, metric: &str) -> Option<i64> {
        let needle = format!("{metric} ");
        encoded.lines().find_map(|line| {
            line.strip_prefix(&needle)
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<i64>().ok())
        })
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

    #[tokio::test]
    async fn subscription_guard_tracks_live_tasks_across_every_exit_path() {
        use tokio::sync::oneshot;

        let gauge = Gauge::default();

        let (started_tx, started_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = oneshot::channel();
        let task_gauge = gauge.clone();
        let normal_exit = tokio::spawn(async move {
            let _subscription = ActiveSubscriptionGuard::for_gauge(task_gauge);
            let _ = started_tx.send(());
            let _ = finish_rx.await;
        });
        started_rx.await.expect("normal task must start");
        assert_eq!(gauge.get(), 1, "a live task must contribute exactly once");
        let _ = finish_tx.send(());
        normal_exit.await.expect("normal task must join");
        assert_eq!(gauge.get(), 0, "normal exit must release its contribution");

        let (started_tx, started_rx) = oneshot::channel();
        let task_gauge = gauge.clone();
        let cancelled = tokio::spawn(async move {
            let _subscription = ActiveSubscriptionGuard::for_gauge(task_gauge);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.expect("cancelled task must start");
        assert_eq!(gauge.get(), 1, "a cancellable live task must be counted");
        cancelled.abort();
        let cancellation = cancelled.await.expect_err("task must be cancelled");
        assert!(cancellation.is_cancelled());
        assert_eq!(gauge.get(), 0, "cancellation must release its contribution");

        let (started_tx, started_rx) = oneshot::channel();
        let (fail_tx, fail_rx) = oneshot::channel();
        let task_gauge = gauge.clone();
        let failed = tokio::spawn(async move {
            let _subscription = ActiveSubscriptionGuard::for_gauge(task_gauge);
            let _ = started_tx.send(());
            let _ = fail_rx.await;
            panic!("synthetic provider failure");
        });
        started_rx.await.expect("failing task must start");
        assert_eq!(gauge.get(), 1, "a live task before failure must be counted");
        let _ = fail_tx.send(());
        let failure = failed.await.expect_err("task must fail");
        assert!(failure.is_panic());
        assert_eq!(gauge.get(), 0, "task failure must release its contribution");
    }

    #[test]
    fn subscription_guard_owns_exact_task_lifetime() {
        let _lock = super::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let r = fresh_registry();
        set_active_subscriptions(0);
        {
            let _guard = ActiveSubscriptionGuard::new();
            assert!(
                encode(&r).contains("aletheia_active_subscriptions 1"),
                "live polling task must own one subscription"
            );
        }
        assert!(
            encode(&r).contains("aletheia_active_subscriptions 0"),
            "task exit must release its subscription"
        );
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

    #[test]
    fn register_and_record_command_denied() {
        let r = fresh_registry();
        record_command_denied("_test_channel");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_command_denied_total{channel_id=\"_test_channel\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_ingress_duplicate() {
        let r = fresh_registry();
        record_ingress_duplicate("_test_channel");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_ingress_duplicates_total{channel_id=\"_test_channel\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_cursor_checkpoint() {
        let r = fresh_registry();
        record_cursor_checkpoint("_test_channel");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_cursor_checkpoints_total{channel_id=\"_test_channel\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_handler_saturation_and_in_flight() {
        let _guard = super::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let r = fresh_registry();
        let before = encode(&r);
        let saturation_before =
            scalar_value(&before, "aletheia_inbound_handler_saturation_total").unwrap_or(0);
        let in_flight_before =
            scalar_value(&before, "aletheia_inbound_handlers_in_flight").unwrap_or(0);

        record_inbound_handler_saturation();
        add_inbound_handlers_in_flight(3);
        let out = encode(&r);
        assert_eq!(
            scalar_value(&out, "aletheia_inbound_handler_saturation_total"),
            Some(saturation_before + 1),
            "got: {out}"
        );
        assert_eq!(
            scalar_value(&out, "aletheia_inbound_handlers_in_flight"),
            Some(in_flight_before + 3),
            "got: {out}"
        );
        add_inbound_handlers_in_flight(-3);
        assert_eq!(
            scalar_value(&encode(&r), "aletheia_inbound_handlers_in_flight"),
            Some(in_flight_before)
        );
    }
}
