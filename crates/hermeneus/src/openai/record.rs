//! Telemetry helpers for OpenAI provider: span recording and metrics emission.

use std::time::Instant;

use tracing::info;

use crate::types::{CompletionRequest, CompletionResponse};

pub(super) fn elapsed_millis_u64(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn record_nonstream_success(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    api_family: &str,
    request: &CompletionRequest,
    response: &mut CompletionResponse,
    cost_usd: f64,
) {
    response.duration_ms = Some(elapsed_millis_u64(start));
    tracing::Span::current().record("llm.tokens_in", response.usage.input_tokens);
    tracing::Span::current().record("llm.tokens_out", response.usage.output_tokens);
    tracing::Span::current().record("llm.status", "ok");
    tracing::Span::current().record("llm.retries", attempt);
    info!(
        provider = %provider_name,
        api_family,
        model = %request.model,
        tokens_in = response.usage.input_tokens,
        tokens_out = response.usage.output_tokens,
        "OpenAI call complete"
    );
    crate::metrics::record_completion(
        provider_name,
        response.usage.input_tokens,
        response.usage.output_tokens,
        cost_usd,
        true,
    );
    // WHY(#4658): OpenAI wire parsers populate cache_read_tokens; emit them so
    // cost/efficiency dashboards see prompt-cache activity.
    crate::metrics::record_cache_tokens(
        provider_name,
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );
    crate::metrics::record_latency(&request.model, "ok", start.elapsed().as_secs_f64());
}

pub(super) fn record_stream_failure(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
) {
    tracing::Span::current().record("llm.duration_ms", elapsed_millis_u64(start));
    tracing::Span::current().record("llm.retries", attempt);
    tracing::Span::current().record("llm.status", "error");
    crate::metrics::record_completion(provider_name, 0, 0, 0.0, false);
    crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
}

pub(super) fn record_stream_http_failure(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
    status: u16,
) {
    tracing::Span::current().record("llm.duration_ms", elapsed_millis_u64(start));
    tracing::Span::current().record("llm.retries", attempt);
    tracing::Span::current().record(
        "llm.status",
        if status == 401 {
            "auth_failed"
        } else {
            "error"
        },
    );
    crate::metrics::record_completion(provider_name, 0, 0, 0.0, false);
    crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
}

pub(super) fn record_stream_success(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
    response: &mut CompletionResponse,
    cost_usd: f64,
) {
    let duration_ms = elapsed_millis_u64(start);
    response.duration_ms = Some(duration_ms);
    tracing::Span::current().record("llm.duration_ms", duration_ms);
    tracing::Span::current().record("llm.tokens_in", response.usage.input_tokens);
    tracing::Span::current().record("llm.tokens_out", response.usage.output_tokens);
    tracing::Span::current().record("llm.status", "ok");
    tracing::Span::current().record("llm.retries", attempt);
    crate::metrics::record_completion(
        provider_name,
        response.usage.input_tokens,
        response.usage.output_tokens,
        cost_usd,
        true,
    );
    // WHY(#4658): Streaming completions accumulate cached prompt tokens in
    // usage.cache_read_tokens; record them for observability parity with
    // non-streaming completions.
    crate::metrics::record_cache_tokens(
        provider_name,
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );
    crate::metrics::record_latency(&request.model, "ok", start.elapsed().as_secs_f64());
}
