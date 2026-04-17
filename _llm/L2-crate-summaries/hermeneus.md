# hermeneus

**Purpose:** Anthropic LLM client with streaming, retries, cost tracking, provider health, and a `LlmProvider` trait for swappable backends.

## Key types

| Type | Purpose |
|------|---------|
| `LlmProvider` | Trait: `complete()`, `complete_streaming()`, `supported_models()` |
| `AnthropicProvider` | Concrete Anthropic Messages API implementation |
| `CompletionRequest` | LLM request: model, messages, tools, thinking, cache config |
| `CompletionResponse` | LLM response: content blocks, usage, stop reason |
| `ProviderRegistry` | Model → provider routing with health tracking |

## Public API surface

- `hermeneus::provider` — `LlmProvider` trait, `ProviderRegistry`, `ModelPricing`
- `hermeneus::types` — `CompletionRequest`, `CompletionResponse`, `Message`, `ContentBlock`
- `hermeneus::anthropic` — `AnthropicProvider`, `StreamEvent`, `FallbackConfig`

## When to look here

- When making LLM calls or adding a new provider implementation
- When configuring model fallback chains or health-aware routing
