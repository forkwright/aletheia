//! Prometheus metric definitions for the agent pipeline.
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
struct NousLabels {
    nous_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousStageLabels {
    nous_id: String,
    stage: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct PipelineErrorLabels {
    nous_id: String,
    stage: String,
    error_type: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousTaskTypeLabels {
    nous_id: String,
    task_type: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousToolLabels {
    nous_id: String,
    tool_name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousReasonLabels {
    nous_id: String,
    reason: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct StreamEventDroppedLabels {
    nous_id: String,
    event_type: String,
    reason: String,
}

// ── Metric families ──

static PIPELINE_TURNS_TOTAL: LazyLock<Family<NousLabels, Counter>> = LazyLock::new(Family::default);

fn pipeline_stage_histogram() -> Histogram {
    Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0])
}

type NousStageHistogramFamily = Family<NousStageLabels, Histogram, fn() -> Histogram>;

static PIPELINE_STAGE_DURATION_SECONDS: LazyLock<NousStageHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(pipeline_stage_histogram));

static PIPELINE_ERRORS_TOTAL: LazyLock<Family<PipelineErrorLabels, Counter>> =
    LazyLock::new(Family::default);

static CACHE_READ_TOKENS_TOTAL: LazyLock<Family<NousLabels, Counter>> =
    LazyLock::new(Family::default);

static BACKGROUND_TASK_FAILURES_TOTAL: LazyLock<Family<NousTaskTypeLabels, Counter>> =
    LazyLock::new(Family::default);

static TOOL_FAILURES_TOTAL: LazyLock<Family<NousToolLabels, Counter>> =
    LazyLock::new(Family::default);

static STREAM_EVENTS_DROPPED_TOTAL: LazyLock<Family<StreamEventDroppedLabels, Counter>> =
    LazyLock::new(Family::default);

static NOUS_INBOX_SATURATION_TOTAL: LazyLock<Family<NousReasonLabels, Counter>> =
    LazyLock::new(Family::default);

static CACHE_CREATION_TOKENS_TOTAL: LazyLock<Family<NousLabels, Counter>> =
    LazyLock::new(Family::default);

static DPO_PAIRS_CAPTURED_TOTAL: LazyLock<Family<NousLabels, Counter>> =
    LazyLock::new(Family::default);

static TRAINING_CAPTURE_REJECTED_TOTAL: LazyLock<Family<NousReasonLabels, Counter>> =
    LazyLock::new(Family::default);

static HEALTH_POLLER_RESTARTS_TOTAL: LazyLock<Counter> = LazyLock::new(Counter::default);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_pipeline_turns",
        "Total pipeline turns processed",
        PIPELINE_TURNS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_pipeline_stage_duration_seconds",
        "Pipeline stage duration in seconds",
        PIPELINE_STAGE_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_pipeline_errors",
        "Total pipeline errors",
        PIPELINE_ERRORS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_cache_read_tokens",
        "Total tokens read from prompt cache (cache hits)",
        CACHE_READ_TOKENS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_cache_creation_tokens",
        "Total tokens written to prompt cache (cache misses)",
        CACHE_CREATION_TOKENS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_nous_background_task_failures",
        "Total background task failures (extraction, distillation, skill analysis)",
        BACKGROUND_TASK_FAILURES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_tool_failures",
        "Total tool execution failures by tool name",
        TOOL_FAILURES_TOTAL.clone(),
    );
    registry.register(
        "aletheia_stream_events_dropped",
        "Total streaming events dropped due to full channel or disconnected receiver",
        STREAM_EVENTS_DROPPED_TOTAL.clone(),
    );
    registry.register(
        "aletheia_nous_inbox_saturation",
        "Total nous actor inbox saturation events by reason",
        NOUS_INBOX_SATURATION_TOTAL.clone(),
    );
    registry.register(
        "aletheia_dpo_pairs_captured",
        "Total DPO preference pairs captured from correction turns",
        DPO_PAIRS_CAPTURED_TOTAL.clone(),
    );
    registry.register(
        "aletheia_training_capture_rejected",
        "Total training records rejected by the authorship gate",
        TRAINING_CAPTURE_REJECTED_TOTAL.clone(),
    );
    registry.register(
        "aletheia_nous_health_poller_restarts",
        "Total health poller restarts by the supervisor",
        HEALTH_POLLER_RESTARTS_TOTAL.clone(),
    );
}

// ── Recording ──

/// Record a completed pipeline stage.
pub(crate) fn record_stage(nous_id: &str, stage: &str, duration_secs: f64) {
    PIPELINE_STAGE_DURATION_SECONDS
        .get_or_create(&NousStageLabels {
            nous_id: nous_id.to_owned(),
            stage: stage.to_owned(),
        })
        .observe(duration_secs);
}

/// Record a completed turn.
pub(crate) fn record_turn(nous_id: &str) {
    PIPELINE_TURNS_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc();
}

/// Record a pipeline error.
pub(crate) fn record_error(nous_id: &str, stage: &str, error_type: &str) {
    PIPELINE_ERRORS_TOTAL
        .get_or_create(&PipelineErrorLabels {
            nous_id: nous_id.to_owned(),
            stage: stage.to_owned(),
            error_type: error_type.to_owned(),
        })
        .inc();
}

/// Build the production pipeline event emitter.
pub(crate) fn pipeline_event_emitter() -> koina::event::EventEmitter {
    koina::event::EventEmitter::with_metric_sink(record_pipeline_event)
}

fn record_pipeline_event(name: &str, labels: &[(&str, String)], value: f64) {
    match name {
        "StageCompleted" => {
            if let (Some(nous_id), Some(stage)) = (label(labels, "nous_id"), label(labels, "stage"))
            {
                record_stage(nous_id, stage, value);
            }
        }
        "StageError" | "StageTimeout" => {
            if let (Some(nous_id), Some(stage), Some(error_type)) = (
                label(labels, "nous_id"),
                label(labels, "stage"),
                label(labels, "error_type"),
            ) {
                record_error(nous_id, stage, error_type);
            }
        }
        "TurnCompleted" => {
            if let Some(nous_id) = label(labels, "nous_id") {
                record_turn(nous_id);
            }
        }
        _ => {
            // NOTE: other event types have no associated metrics.
        }
    }
}

fn label<'a>(labels: &'a [(&str, String)], key: &str) -> Option<&'a str> {
    labels
        .iter()
        .find(|(label, _)| *label == key)
        .map(|(_, value)| value.as_str())
}

/// Record a tool execution failure.
///
/// WHY: Tool failures at warn level need a metrics counter so operators can
/// set alerts on systematic tool problems.
pub(crate) fn record_tool_failure(nous_id: &str, tool_name: &str) {
    tracing::warn!(nous_id, tool_name, "tool execution failed");
    TOOL_FAILURES_TOTAL
        .get_or_create(&NousToolLabels {
            nous_id: nous_id.to_owned(),
            tool_name: tool_name.to_owned(),
        })
        .inc();
}

/// Record a dropped streaming event.
///
/// WHY(#4893): Silently dropped streaming events are invisible contract
/// violations. This counter surfaces the drop rate per event type so operators
/// can detect LLM delta loss as readily as tool lifecycle drops.
pub(crate) fn record_stream_event_dropped(nous_id: &str, event_type: &str, reason: &str) {
    tracing::debug!(nous_id, event_type, reason, "streaming event dropped");
    STREAM_EVENTS_DROPPED_TOTAL
        .get_or_create(&StreamEventDroppedLabels {
            nous_id: nous_id.to_owned(),
            event_type: event_type.to_owned(),
            reason: reason.to_owned(),
        })
        .inc();
}

/// Record actor inbox saturation.
///
/// WHY(#4644): Bounded actor inboxes intentionally fail with service-busy
/// errors under sustained overload. Operators need a Prometheus signal before
/// client-facing failures become the only evidence of saturation.
pub(crate) fn record_inbox_saturation(nous_id: &str, reason: &str) {
    tracing::warn!(nous_id, reason, "nous actor inbox saturated");
    NOUS_INBOX_SATURATION_TOTAL
        .get_or_create(&NousReasonLabels {
            nous_id: nous_id.to_owned(),
            reason: reason.to_owned(),
        })
        .inc();
}

/// Record a captured DPO preference pair.
///
/// WHY: DPO pair capture is a training pipeline signal. Operators need
/// visibility into how much free preference data is being generated
/// from user corrections.
pub(crate) fn record_dpo_pair(nous_id: &str) {
    DPO_PAIRS_CAPTURED_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc();
}

/// Record a training record rejected by the authorship gate.
///
/// WHY: Operators need visibility into decontamination rates.
/// `reason` is the author class that triggered rejection
/// (e.g. "subagent", `system_scaffolding`).
pub(crate) fn record_training_capture_rejected(nous_id: &str, reason: &str) {
    TRAINING_CAPTURE_REJECTED_TOTAL
        .get_or_create(&NousReasonLabels {
            nous_id: nous_id.to_owned(),
            reason: reason.to_owned(),
        })
        .inc();
}

/// Record a background task failure.
///
/// WHY: Background extraction/distillation failures are silent data loss.
/// This counter surfaces the failure rate for alerting.
pub(crate) fn record_background_failure(nous_id: &str, task_type: &str) {
    BACKGROUND_TASK_FAILURES_TOTAL
        .get_or_create(&NousTaskTypeLabels {
            nous_id: nous_id.to_owned(),
            task_type: task_type.to_owned(),
        })
        .inc();
}

/// Record a health poller supervisor restart.
///
/// WHY: Repeated restarts indicate a persistent bug in `health_cycle` or
/// `restart_actor`. Operators should alert when this counter grows.
pub(crate) fn record_health_poller_restart() {
    HEALTH_POLLER_RESTARTS_TOTAL.inc();
}

/// Record prompt cache usage from an API response.
///
/// Tracks cache effectiveness for cost monitoring. `cache_read_tokens` maps to
/// the Anthropic response `cache_read_input_tokens` field (cache hits).
/// `cache_creation_tokens` maps to `cache_creation_input_tokens` (cache misses
/// that populate the cache for subsequent requests).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "forked agent cache coherence — wired from pipeline turn completion"
    )
)]
pub(crate) fn record_cache_usage(
    nous_id: &str,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) {
    CACHE_READ_TOKENS_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc_by(cache_read_tokens);
    CACHE_CREATION_TOKENS_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc_by(cache_creation_tokens);
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
    fn register_exposes_all_metrics() {
        let r = fresh_registry();
        record_stage("n1", "context", 0.001);
        record_turn("n1");
        record_error("n1", "execute", "timeout");
        record_cache_usage("n1", 5, 5);
        record_background_failure("n1", "extract");
        record_tool_failure("n1", "read");
        record_stream_event_dropped("n1", "text_delta", "full");
        record_inbox_saturation("n1", "send_timeout");
        record_training_capture_rejected("n1", "non_authored");
        record_dpo_pair("n1");
        record_health_poller_restart();

        let out = encode(&r);
        for metric in [
            "aletheia_pipeline_turns_total",
            "aletheia_pipeline_stage_duration_seconds_count",
            "aletheia_pipeline_errors_total",
            "aletheia_cache_read_tokens_total",
            "aletheia_cache_creation_tokens_total",
            "aletheia_nous_background_task_failures_total",
            "aletheia_tool_failures_total",
            "aletheia_stream_events_dropped_total",
            "aletheia_nous_inbox_saturation_total",
            "aletheia_training_capture_rejected_total",
            "aletheia_dpo_pairs_captured_total",
            "aletheia_nous_health_poller_restarts_total",
        ] {
            assert!(out.contains(metric), "missing `{metric}` in: {out}");
        }
    }

    #[test]
    fn record_stage_increments_histogram_count() {
        let r = fresh_registry();
        record_stage("agent-a", "context", 0.001);
        record_stage("agent-a", "context", 0.002);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_pipeline_stage_duration_seconds_count{nous_id=\"agent-a\",stage=\"context\"} 2"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn record_turn_increments_counter() {
        let r = fresh_registry();
        record_turn("agent-b");
        record_turn("agent-b");
        record_turn("agent-b");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_pipeline_turns_total{nous_id=\"agent-b\"} 3"),
            "got: {out}"
        );
    }

    #[test]
    fn turn_completed_event_counts_turn_exactly_once() {
        // WHY(#5400): the pipeline TurnCompleted event must be the single owner
        // of the turn counter. Audit hooks must not also call record_turn.
        let r = fresh_registry();
        let emitter = pipeline_event_emitter();
        emitter.emit(&crate::pipeline::events::TurnCompleted {
            nous_id: "agent-e".to_owned(),
            model: "test-model".to_owned(),
            duration_ms: 100,
            input_tokens: 10,
            output_tokens: 10,
            tool_calls: 0,
            stages_completed: 6,
        });
        let out = encode(&r);
        assert!(
            out.contains("aletheia_pipeline_turns_total{nous_id=\"agent-e\"} 1"),
            "TurnCompleted should increment the counter exactly once, got: {out}"
        );
    }

    #[test]
    fn record_error_increments_counter() {
        let r = fresh_registry();
        record_error("agent-c", "execute", "test_error");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_pipeline_errors_total{nous_id=\"agent-c\",stage=\"execute\",error_type=\"test_error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn record_cache_usage_accumulates() {
        let r = fresh_registry();
        record_cache_usage("agent-d", 1500, 500);
        record_cache_usage("agent-d", 0, 1000);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_cache_read_tokens_total{nous_id=\"agent-d\"} 1500"),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_cache_creation_tokens_total{nous_id=\"agent-d\"} 1500"),
            "got: {out}"
        );
    }
}
