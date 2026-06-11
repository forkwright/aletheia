//! Prometheus metric definitions for the tool system.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolInvocationLabels {
    tool_name: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolLabels {
    tool_name: String,
}

// ── Metric families ──

static TOOL_INVOCATIONS_TOTAL: LazyLock<Family<ToolInvocationLabels, Counter>> =
    LazyLock::new(Family::default);

fn tool_duration_histogram() -> Histogram {
    Histogram::new([0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0])
}

type ToolHistogramFamily = Family<ToolLabels, Histogram, fn() -> Histogram>;

static TOOL_DURATION_SECONDS: LazyLock<ToolHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(tool_duration_histogram));

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_tool_invocations",
        "Total tool invocations",
        TOOL_INVOCATIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_tool_duration_seconds",
        "Tool execution duration in seconds",
        TOOL_DURATION_SECONDS.clone(),
    );
}

// ── Recording ──

/// Outcome bucket used for tool invocation metrics.
#[derive(Clone, Copy)]
pub(crate) enum InvocationStatus {
    Ok,
    Partial,
    Error,
}

impl InvocationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Error => "error",
        }
    }
}

/// Record a tool invocation.
pub(crate) fn record_invocation(tool_name: &str, duration_secs: f64, status: InvocationStatus) {
    TOOL_INVOCATIONS_TOTAL
        .get_or_create(&ToolInvocationLabels {
            tool_name: tool_name.to_owned(),
            status: status.as_str().to_owned(),
        })
        .inc();
    TOOL_DURATION_SECONDS
        .get_or_create(&ToolLabels {
            tool_name: tool_name.to_owned(),
        })
        .observe(duration_secs);
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
    fn register_and_record_invocation_success() {
        let r = fresh_registry();
        record_invocation("_test_tool_ok", 0.05, InvocationStatus::Ok);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_ok\",status=\"ok\"} 1"
            ),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_tool_duration_seconds_count{tool_name=\"_test_tool_ok\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_invocation_failure() {
        let r = fresh_registry();
        record_invocation("_test_tool_err", 0.01, InvocationStatus::Error);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_err\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_invocation_partial() {
        let r = fresh_registry();
        record_invocation("_test_tool_partial", 0.02, InvocationStatus::Partial);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_partial\",status=\"partial\"} 1"
            ),
            "got: {out}"
        );
    }
}
