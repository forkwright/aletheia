# hermeneus

Anthropic LLM client with streaming, retries, fallback, health tracking, and cost estimation. 10.5K lines.

## Read first

1. `src/types.rs`: CompletionRequest, CompletionResponse, Message, ContentBlock (canonical type system)
2. `src/provider.rs`: LlmProvider trait, ProviderRegistry, ModelPricing
3. `src/anthropic/client.rs`: AnthropicProvider (HTTP client, retries, streaming)
4. `src/error.rs`: Error variants and `is_retryable()` classification
5. `src/health.rs`: ProviderHealth state machine (Up/Degraded/Down)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `LlmProvider` | `provider.rs` | Trait: `complete()`, `complete_streaming()`, `supported_models()` |
| `ProviderRegistry` | `provider.rs` | Model -> provider routing with health tracking |
| `AnthropicProvider` | `anthropic/client.rs` | Concrete Anthropic Messages API implementation |
| `CompletionRequest` | `types.rs` | LLM request (model, messages, tools, thinking, cache config) |
| `CompletionResponse` | `types.rs` | LLM response (content blocks, usage, stop reason) |
| `StreamEvent` | `anthropic/stream/mod.rs` | SSE events: TextDelta, ThinkingDelta, ToolUse, etc. |
| `FallbackConfig` | `fallback.rs` | Model fallback chain on transient failures |

## Patterns

- **Retry + backoff**: exponential (1s base, 2x factor, 30s max), 3 retries default. Jitter ±25%.
- **Health circuit breaker**: 5 consecutive errors -> Down. 60s cooldown. AuthFailure never auto-recovers.
- **Streaming**: SSE parser emits StreamEvent. Retry-safe only before first event (mid-stream propagates).
- **Prompt caching**: `cache_system`, `cache_tools`, `cache_turns` flags inject `cache_control: ephemeral`.
- **OAuth auth**: `Bearer` + `anthropic-beta: oauth-2025-04-20`. API key auth: `x-api-key` header.
- **Wire format**: `anthropic/wire/` handles Anthropic-specific JSON serialization.

## Common tasks

| Task | Where |
|------|-------|
| Add LLM provider | New module (e.g., `src/openai/`), implement `LlmProvider` trait |
| Modify retry logic | `src/models.rs` (constants) + `src/anthropic/client.rs` (retry loop) |
| Add streaming event | `anthropic/stream/mod.rs` (StreamEvent enum) + `anthropic/stream/accumulator.rs` (accumulator) |
| Update model pricing | `src/models.rs` (PRICING constant) |
| Add metric | `src/metrics.rs` (LazyLock static, record function) |

## Dependencies

Uses: koina, taxis, reqwest, serde_json, tokio, snafu, tracing
Used by: nous, pylon, organon, melete

## Observability

### Metrics (Prometheus)

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_llm_tokens_total` | Counter | `provider`, `direction` | Total tokens consumed (input/output) |
| `aletheia_llm_cost_total` | Counter | `provider` | Total cost in USD |
| `aletheia_llm_requests_total` | Counter | `provider`, `status` | Total API requests (ok/error) |
| `aletheia_llm_cache_tokens_total` | Counter | `provider`, `direction` | Cache tokens read/written |
| `aletheia_llm_request_duration_seconds` | Histogram | `model`, `status` | Request latency (buckets: 0.1s to 120s) |
| `aletheia_llm_ttft_seconds` | Histogram | `model`, `status` | Time to first token for streaming (buckets: 0.05s to 10s) |
| `aletheia_llm_circuit_breaker_transitions_total` | Counter | `provider`, `from`, `to` | Circuit breaker state changes |
| `aletheia_llm_concurrency_limit` | Gauge | `provider` | Current adaptive concurrency limit |
| `aletheia_llm_concurrency_latency_ewma_seconds` | Gauge | `provider` | EWMA latency for adaptive limiter |
| `aletheia_llm_concurrency_in_flight` | Gauge | `provider` | In-flight request count |

### Spans

_No `#[instrument]` spans in this crate. Span creation is delegated to callers (typically via `tracing::info_span!` in parent crates)._

### Log Events

| Level | Event | When |
|-------|-------|------|
| `warn` | `circuit-breaker open; streaming request rejected` | Request blocked by circuit breaker |
| `warn` | `streaming retry abandoned after content started` | Non-retryable error mid-stream |
| `warn` | `SSE stream returned retryable error before content` | Pre-content retryable failure |
| `warn` | `no pricing configured for model; cost reported as 0` | Missing pricing config for model |
| `warn` | `fallback chain exhausted` | All fallback models failed |
| `error` | `SSE error after content started streaming — cannot retry` | Unrecoverable mid-stream error |
| `info` | `complexity limiter recommendation` | Token complexity analysis results |
