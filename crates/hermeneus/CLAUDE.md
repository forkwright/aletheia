# hermeneus

Anthropic LLM client with streaming, retries, fallback, health tracking, and cost estimation. 7K lines.

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
| `StreamEvent` | `anthropic/mod.rs` | SSE events: TextDelta, ThinkingDelta, ToolUse, etc. |
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
| Add streaming event | `anthropic/mod.rs` (StreamEvent enum) + `anthropic/stream.rs` (accumulator) |
| Update model pricing | `src/models.rs` (PRICING constant) |
| Add metric | `src/metrics.rs` (LazyLock static, record function) |

## Dependencies

Uses: koina, taxis, reqwest, serde_json, tokio, snafu, tracing
Used by: nous, pylon, organon, melete
