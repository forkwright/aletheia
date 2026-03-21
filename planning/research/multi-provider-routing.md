# Multi-Provider LLM Routing

Research document for adding multi-provider LLM support with intelligent routing to Aletheia.

---

## Question

How should Aletheia support multiple LLM providers (Anthropic, OpenAI, local models) with routing based on cost, capability, and availability? What is the current coupling to Anthropic, and what needs to change?

---

## Findings

### 1. Current provider coupling audit

#### Architecture: anthropic-Native by design

Hermeneus is explicitly Anthropic-native. From the module documentation (`crates/hermeneus/src/lib.rs`):

> "Anthropic-native types and client for LLM interaction. Other providers implement adapters that map to/from the Anthropic type system."

This is a deliberate design decision, not accidental coupling. The type system models Anthropic's Messages API, and other providers are expected to adapt to it.

#### Existing abstraction layer

The `LlmProvider` trait (`crates/hermeneus/src/provider.rs:23-65`) defines the provider interface:

```rust
pub trait LlmProvider: Send + Sync {
    fn complete(&self, request: &CompletionRequest)
        -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send>>;
    fn complete_streaming(&self, request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send))
        -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send>>;
    fn supported_models(&self) -> &[&str];
    fn supports_model(&self, model: &str) -> bool;
    fn supports_streaming(&self) -> bool;
    fn name(&self) -> &str;
}
```

The trait accepts `CompletionRequest` and returns `CompletionResponse`, both Anthropic-modeled types. Other providers must translate to/from these types, which is the intended adapter pattern.

`ProviderRegistry` (`crates/hermeneus/src/provider.rs:160-239`) manages multiple providers with per-provider health tracking. It supports:

- Registering multiple providers
- Finding a provider by model name (linear search, first match)
- Per-provider health state machine (Up/Degraded/Down)
- Success/error recording for health transitions

#### Where Anthropic is hardcoded

| Location | What | Portability Impact |
|----------|------|--------------------|
| `anthropic/client.rs:25-31` | Hardcoded Claude model list in `SUPPORTED_MODELS` | Provider-specific, contained |
| `anthropic/client.rs:406-451` | `x-api-key` and `anthropic-version` headers | Provider-specific, contained |
| `provider.rs:95-153` | Default pricing for Claude models only | Needs extension for other providers |
| `types.rs:420-425` | `cache_system`, `cache_tools`, `cache_turns` fields | Anthropic-specific; other providers ignore them |
| `types.rs:126-154` | `ServerToolUse`, `WebSearchToolResult`, `CodeExecutionResult` | Anthropic server-side tools; no equivalent in other APIs |
| `types.rs:117-123` | `Thinking` content block with `signature` | Anthropic extended thinking; OpenAI has `reasoning` |
| `anthropic/wire/` | Wire format serialization/deserialization | Fully Anthropic-specific, correctly isolated |
| `anthropic/stream.rs` | SSE streaming parser | Anthropic-specific stream format, correctly isolated |

#### How providers flow through the system

1. **Init** (`crates/aletheia/src/server.rs`): `build_provider_registry()` creates a registry, initializes `AnthropicProvider`, and registers it. Only one provider is registered today.
2. **Routing** (`crates/nous/src/execute/mod.rs:33-56`): `resolve_provider_checked()` calls `registry.find_provider(model)` (first match), checks health, and returns the provider or an error.
3. **Execution**: `provider.complete()` or `provider.complete_streaming()` is called. The caller records success/error for health tracking.

#### Recent addition: model fallback chain

`crates/hermeneus/src/fallback.rs` (landed on main) implements intra-provider model fallback. `complete_with_fallback()` retries the primary model `retries_before_fallback` times, then tries each model in `FallbackConfig.fallback_models` in order. Non-retryable errors (auth, 4xx) propagate immediately; retryable errors (429, 5xx, timeout) trigger fallback via the `Error::is_retryable()` method (`crates/hermeneus/src/error.rs:72-87`).

This is model-level fallback within a single provider. It does not cross provider boundaries (the same `provider` instance handles all models in the chain). Multi-provider fallback would be the next layer: if Anthropic is down entirely, try OpenAI.

#### Assessment

The coupling is well-structured. Anthropic-specific code is isolated in the `anthropic/` module. The `LlmProvider` trait and `ProviderRegistry` already form a multi-provider foundation. The model-level fallback chain is already implemented. The main gap is that the types (`CompletionRequest`, `CompletionResponse`) carry Anthropic-specific fields that other providers must ignore or translate.

This is a strength, not a weakness. Making the types provider-neutral would lose Anthropic-specific features (prompt caching, extended thinking, server-side tools) that Aletheia uses heavily. The adapter pattern (other providers translate to/from these types) preserves full Anthropic capability while enabling other providers.

### 2. provider trait design

#### Current trait: what works

The existing `LlmProvider` trait handles the core use case well:

- `complete` and `complete_streaming` cover the two call patterns
- `supported_models` enables model-based routing
- `Send + Sync` allows shared ownership across async tasks
- `name` provides diagnostics

#### What is missing

**Model capability metadata.** The registry knows which provider supports which model name, but nothing about model capabilities. The caller must know that `claude-opus-4-6` supports thinking and `gpt-4o` does not. This information should be queryable.

**Embeddings.** Aletheia uses embeddings for memory recall (`crates/dianoia`). Today this is handled separately (local Candle inference or API call). A unified provider trait should optionally support embedding generation.

**Cost estimation before request.** `ProviderConfig` has pricing data, but it lives outside the trait. Callers cannot ask "what would this request cost on provider X?" without accessing the config directly.

**Health-aware routing.** `ProviderRegistry::find_provider()` returns the first match regardless of health. `resolve_provider_checked()` in nous checks health after finding the provider but does not try alternatives.

#### Proposed trait extension

Do not replace the existing trait. Extend the system with a capability registry:

```rust
/// Model capability metadata.
pub struct ModelCapabilities {
    /// Context window size in tokens.
    pub context_window: u32,
    /// Maximum output tokens.
    pub max_output_tokens: u32,
    /// Supports extended thinking / reasoning.
    pub supports_thinking: bool,
    /// Supports tool/function calling.
    pub supports_tools: bool,
    /// Supports streaming responses.
    pub supports_streaming: bool,
    /// Supports image input (vision).
    pub supports_vision: bool,
    /// Supports prompt caching.
    pub supports_caching: bool,
    /// Cost per million input tokens (USD).
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens (USD).
    pub output_cost_per_mtok: f64,
}

/// Extension trait for providers that can report capabilities.
pub trait ProviderCapabilities: LlmProvider {
    /// Return capabilities for a supported model, or None if unsupported.
    fn model_capabilities(&self, model: &str) -> Option<ModelCapabilities>;
}
```

This keeps `LlmProvider` backward-compatible. Providers that implement `ProviderCapabilities` enable richer routing. Providers that do not are still usable with manual model selection.

**Embedding trait** (separate, not merged into `LlmProvider`):

```rust
pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, texts: &[&str])
        -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send>>;
    fn embedding_dimensions(&self) -> u32;
    fn name(&self) -> &str;
}
```

Embeddings are a different operation with different providers (OpenAI `text-embedding-3-small`, local Candle models). Merging them into `LlmProvider` would violate interface segregation.

### 3. routing strategies

#### 3.1 static assignment (Task type -> provider)

Map task categories to specific models/providers in configuration:

```toml
[routing.static]
default = "claude-opus-4-6"
distillation = "claude-haiku-4-5"
embedding = "text-embedding-3-small"
qa_evaluation = "claude-sonnet-4-6"
```

**Pros:**
- Low implementation and reasoning overhead
- Predictable cost (operator controls which model handles what)
- Already partially implemented (Aletheia uses different models for different tasks via `NousConfig`)

**Cons:**
- No automatic adaptation to provider outages
- Requires manual configuration updates for new models
- Does not optimize cost dynamically

**Verdict: Implement first.** This is the natural extension of the current system. `NousConfig.model` already selects a model per agent. Extending this to support provider-qualified model names (e.g., `openai/gpt-4o`) requires minimal adaptation.

#### 3.2 fallback chain (Try providers in order)

**Existing work:** `crates/hermeneus/src/fallback.rs` implements model-level fallback within a single provider. This section covers cross-provider fallback: trying a different provider when the primary provider is entirely unavailable.

Define an ordered list of providers per model class. On failure, try the next provider:

```toml
[routing.fallback]
primary = "claude-opus-4-6"
chain = ["gpt-4o", "claude-sonnet-4-6"]
```

**Pros:**
- Automatic resilience against provider outages
- Transparent to callers (same request, different backend)
- Integrates with existing health tracking (skip Down providers)

**Cons:**
- Response format differences between providers can cause downstream issues (tool call format, thinking blocks)
- Latency increases on fallback (failed request + retry)
- Cost unpredictability (fallback provider may be more expensive)
- Need to define "equivalent" models across providers (capability matching)

**Implementation approach:**

Extend `ProviderRegistry::find_provider()` to return providers in priority order, filtered by health:

```rust
pub fn find_providers(&self, model: &str) -> Vec<&dyn LlmProvider> {
    // Return all providers that support this model, ordered by registration
    // and filtered by health (Up/Degraded first, Down last)
}
```

The caller (nous execute stage) wraps the call in a retry loop:

```rust
for provider in providers.find_providers(model) {
    match provider.complete(request).await {
        Ok(response) => {
            providers.record_success(provider.name());
            return Ok(response);
        }
        Err(e) if e.is_transient() => {
            providers.record_error(provider.name(), &e);
            continue; // Try next provider
        }
        Err(e) => return Err(e), // Non-transient: don't retry
    }
}
```

**Verdict: Implement second.** High value for reliability. The existing health system provides the foundation. The main risk is response format differences between providers causing downstream breakage.

#### 3.3 cost-Based routing (Cheapest capable provider)

Select the cheapest provider that meets the request's capability requirements:

```rust
fn select_cheapest(
    providers: &ProviderRegistry,
    requirements: &ModelRequirements,
) -> Option<&dyn LlmProvider> {
    providers.capable_providers(requirements)
        .filter(|p| p.health() != Down)
        .min_by(|a, b| a.estimated_cost(request).cmp(&b.estimated_cost(request)))
}
```

**Pros:**
- Automatic cost optimization
- Adapts to pricing changes without code changes
- Can reduce costs significantly for low-complexity tasks (use Haiku instead of Opus)

**Cons:**
- Requires accurate capability metadata for all models
- Cheapest is not always best (quality varies)
- Pricing data must be kept current
- Token count estimation before request is imprecise

**Verdict: Implement third, after capability metadata is in place.** This depends on `ModelCapabilities` being populated for all registered models. Without accurate capability data, the router cannot make correct decisions.

#### 3.4 load balancing (Distribute across providers)

Distribute requests across equivalent providers for throughput:

```rust
enum LoadBalanceStrategy {
    RoundRobin,
    LeastConnections,
    WeightedRandom { weights: Vec<f64> },
}
```

**Pros:**
- Higher aggregate throughput
- Reduces rate limiting from any single provider
- Useful when running multiple agents in parallel

**Cons:**
- Only useful when multiple providers support the same model class
- Adds complexity for marginal benefit in single-user scenarios
- Response consistency issues (different providers produce different outputs)

**Verdict: Defer.** Aletheia is a single-user system. Rate limiting is handled by the existing retry/backoff logic. Load balancing adds complexity without solving a current problem. Revisit if multi-tenant or high-throughput parallel agent use cases emerge.

### 4. API differences between providers

#### 4.1 message format

| Feature | Anthropic | OpenAI | Ollama (OpenAI-compatible) |
|---------|-----------|--------|---------------------------|
| System prompt | Separate `system` field | `system` role message | `system` role message |
| User message | `role: "user"`, `content: string or blocks` | `role: "user"`, `content: string or parts` | Same as OpenAI |
| Assistant message | `role: "assistant"` | `role: "assistant"` | Same as OpenAI |
| Image input | `type: "image"` content block with `source.type: "base64"` | `type: "image_url"` content part with `url` or `data:` URI | Varies by model |
| Thinking/reasoning | `type: "thinking"` content block with `signature` | `reasoning` field on response (not echoed back as content) | Not supported |

**Adapter complexity: Medium.** The core text message flow is similar across providers. The system prompt handling is the most common difference (separate field vs. message role). Image format differs but is a mechanical translation between base64 source blocks and data URIs. Thinking/reasoning is provider-specific and may need to be stripped for non-Anthropic providers.

#### 4.2 tool/Function calling

| Feature | Anthropic | OpenAI | Ollama |
|---------|-----------|--------|--------|
| Tool definition | `tools[]` with `input_schema` (JSON Schema) | `tools[]` with `function.parameters` (JSON Schema) | Same as OpenAI (subset) |
| Tool call in response | `tool_use` content block with `id`, `name`, `input` | `tool_calls[]` on message with `id`, `function.name`, `function.arguments` | Same as OpenAI |
| Tool result | `tool_result` content block with `tool_use_id` | `role: "tool"` message with `tool_call_id` | Same as OpenAI |
| Streaming tool calls | Incremental `input_json_delta` events | Incremental `function.arguments` chunks | Same as OpenAI |
| Force tool use | `tool_choice: { type: "tool", name: "X" }` | `tool_choice: { type: "function", function: { name: "X" } }` | `tool_choice: "auto"` only |
| Server-side tools | `web_search`, `code_execution` | No equivalent | No equivalent |
| Tool choice | `auto`, `any`, `tool` | `auto`, `required`, `none`, named | `auto` only |

**Adapter complexity: High.** Tool calling is the most divergent area. The definition format is similar (both use JSON Schema), but the call/result flow differs structurally: Anthropic uses content blocks within the message; OpenAI uses a separate `tool_calls` array on the message and a dedicated `tool` role for results. Streaming tool calls also differ in their delta format.

The adapter must:
1. Translate tool definitions (rename `input_schema` to `function.parameters`)
2. Extract tool calls from OpenAI's `tool_calls[]` and convert to `ContentBlock::ToolUse`
3. Convert `ContentBlock::ToolResult` messages to OpenAI's `role: "tool"` format
4. Handle streaming deltas differently
5. Drop server-side tools (no OpenAI equivalent) or warn

#### 4.3 streaming protocol

| Feature | Anthropic | OpenAI |
|---------|-----------|--------|
| Protocol | SSE (`text/event-stream`) | SSE (`text/event-stream`) |
| Event types | `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop` | `[DONE]` sentinel, `choices[].delta` |
| Content deltas | Per-block with block type and index | Per-choice with `delta.content` or `delta.tool_calls` |
| Usage | In `message_start` and `message_delta` events | In final chunk with `usage` field (requires `stream_options.include_usage: true`) |
| Error in stream | `error` event with structured JSON | Connection drop or error chunk |

**Adapter complexity: Medium.** Both use SSE, but the event schemas differ. Anthropic has richer event types (per-block start/stop, separate thinking deltas). The adapter must map OpenAI's simpler `delta.content` stream into the `StreamEvent` enum that Aletheia's streaming infrastructure expects.

The existing `StreamAccumulator` (`crates/hermeneus/src/anthropic/stream.rs`) expects Anthropic-specific events. An OpenAI streaming adapter would need its own accumulator that emits the same `StreamEvent` variants.

#### 4.4 token counting

| Feature | Anthropic | OpenAI | Local (Ollama) |
|---------|-----------|--------|----------------|
| Pre-request counting | No public tokenizer | `tiktoken` library available | Tokenizer-dependent |
| Post-request usage | `usage.input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` | `usage.prompt_tokens`, `completion_tokens`, `total_tokens` | Same as OpenAI |
| Counting API | Token counting endpoint (beta) | No dedicated endpoint | None |

**Adapter complexity: Low.** The response `Usage` struct maps directly. The field names differ (`input_tokens` vs `prompt_tokens`) but the semantics are identical. Cache tokens are Anthropic-specific and can default to 0 for other providers.

Pre-request token estimation is provider-specific. `tiktoken` covers OpenAI models. Anthropic has no public tokenizer. For cost estimation, character-based heuristics (4 chars per token) are sufficient. Precise counting is only needed for context window management, and the API returns actual usage after each call.

#### 4.5 authentication

| Provider | Method | Header |
|----------|--------|--------|
| Anthropic | API key or OAuth | `x-api-key: <key>` or `Authorization: Bearer <token>` |
| OpenAI | API key | `Authorization: Bearer <key>` |
| Ollama | None (local) | None |
| Azure OpenAI | API key or Azure AD | `api-key: <key>` or `Authorization: Bearer <token>` |

**Adapter complexity: Low.** Each provider sets its own headers. The existing `CredentialProvider` trait in koina (`crates/koina/src/credential.rs`) already abstracts credential resolution. Each provider implementation handles its own auth.

#### 4.6 error codes and rate limiting

| Feature | Anthropic | OpenAI |
|---------|-----------|--------|
| Rate limit | `429` with `retry-after` header | `429` with `retry-after` or `x-ratelimit-*` headers |
| Overloaded | `529` ("overloaded") | No equivalent |
| Auth error | `401` | `401` |
| Server error | `500` | `500` |
| Streaming error | `overloaded_error` / `rate_limit_error` event types | Connection drop |

**Adapter complexity: Low.** Both use standard HTTP status codes. The existing `Error` enum covers the necessary variants. Each provider's error mapper translates provider-specific codes to the common `Error` type. The health state machine operates on the common `Error` type and requires no changes.

### 5. observations

- **Debt**: `ProviderRegistry::find_provider()` returns the first matching provider with no fallback. If Anthropic is down, the system fails even if OpenAI is registered and healthy. (`crates/hermeneus/src/provider.rs:197-201`)
- **Debt**: `ProviderConfig::default()` hardcodes Anthropic pricing only. No mechanism for registering pricing from other providers at startup. (`crates/hermeneus/src/provider.rs:95-153`)
- **Idea**: Model aliases could map logical names to provider-specific model IDs: `"fast"` -> `"claude-haiku-4-5"` or `"gpt-4o-mini"`, `"strong"` -> `"claude-opus-4-6"` or `"gpt-4o"`. This decouples task configuration from provider selection.
- **Idea**: A health dashboard endpoint in pylon showing all providers, their health states, request counts, and error rates would aid debugging multi-provider setups.
- **Doc gap**: The `LlmProvider` trait has no documentation on what adapters must do with Anthropic-specific fields like `cache_system` or `thinking`. Adapters that silently ignore these could cause subtle behavior differences.

---

## Recommendations

### Phased Implementation

#### Phase 1: provider capabilities and configuration (Small)

Add model capability metadata and multi-provider configuration. No new providers yet.

**Scope:**
- Add `ModelCapabilities` struct to `crates/hermeneus/src/provider.rs`
- Add `ProviderCapabilities` trait (optional extension of `LlmProvider`)
- Implement `ProviderCapabilities` for `AnthropicProvider` with hardcoded Claude model capabilities
- Extend `ProviderConfig` to support multiple provider entries in taxis config
- Add model alias support: logical names that resolve to concrete model IDs

**Blast radius:** `crates/hermeneus/src/provider.rs`, `crates/hermeneus/src/anthropic/client.rs`, `crates/taxis/src/config.rs`

#### Phase 2: OpenAI provider adapter (Medium)

Implement an OpenAI-compatible provider adapter. This covers OpenAI, Azure OpenAI, and any OpenAI-compatible API (Groq, Together, etc.).

**Scope:**
- Add `openai/` module in hermeneus alongside `anthropic/`
- Implement `LlmProvider` for `OpenAiProvider`
- Translate `CompletionRequest` to OpenAI Chat Completions format
- Translate OpenAI response to `CompletionResponse`
- Handle tool call format differences (content blocks vs. `tool_calls[]` array)
- Implement streaming with OpenAI SSE format
- Map OpenAI errors to hermeneus `Error` variants
- Implement `ProviderCapabilities` with OpenAI model metadata

**Key translation points:**
- `system` field -> system role message
- `ContentBlock::ToolUse` -> `tool_calls[]` on assistant message
- `ContentBlock::ToolResult` -> `role: "tool"` message
- `cache_*` fields -> ignored (no OpenAI equivalent)
- `thinking` -> map to `reasoning_effort` parameter if available, or ignore
- `ServerToolDefinition` -> dropped with warning
- `StopReason` mapping: `end_turn` <-> `stop`, `tool_use` <-> `tool_calls`

**Blast radius:** new `crates/hermeneus/src/openai/` module, `crates/hermeneus/Cargo.toml`

#### Phase 3: cross-Provider fallback routing (Small)

Extend the existing model-level fallback (`crates/hermeneus/src/fallback.rs`) to cross provider boundaries.

**Scope:**
- Add `find_providers()` (plural) to `ProviderRegistry` that returns providers in priority order, filtered by health
- Extend `complete_with_fallback()` or add `complete_with_provider_fallback()` that tries alternative providers when the primary provider's entire model chain fails
- Update `resolve_provider_checked()` in nous to iterate through providers on transient failure
- Add routing configuration to taxis: fallback chain per model class
- Log provider transitions: `warn!(primary = %primary, fallback = %fallback, "falling back")`

**Blast radius:** `crates/hermeneus/src/provider.rs`, `crates/hermeneus/src/fallback.rs`, `crates/nous/src/execute/mod.rs`, `crates/taxis/src/config.rs`

#### Phase 4: Ollama / local model provider (Small)

Add support for local models via Ollama's OpenAI-compatible API.

**Scope:**
- Extend `OpenAiProvider` with Ollama-specific configuration (no auth, custom base URL)
- Or create a thin `OllamaProvider` that wraps `OpenAiProvider` with Ollama defaults
- Handle capability differences (most local models lack tool calling, vision, thinking)
- Add model discovery: query Ollama's `/api/tags` endpoint for available models

**Blast radius:** `crates/hermeneus/src/openai/` or new `crates/hermeneus/src/ollama/` module

#### Phase 5: cost-Based routing (Medium)

Route requests to the cheapest capable provider.

**Scope:**
- Add `ModelRequirements` struct: minimum context window, required capabilities (tools, vision, thinking)
- Implement `select_by_cost()` in `ProviderRegistry` that filters by requirements and selects cheapest
- Add cost estimation method to `ProviderCapabilities`: `estimate_cost(input_tokens, output_tokens) -> f64`
- Wire into routing configuration as an alternative strategy

**Blast radius:** `crates/hermeneus/src/provider.rs`, `crates/nous/src/execute/mod.rs`, `crates/taxis/src/config.rs`

### Effort estimates

| Phase | Files Changed | New Files | Complexity | Dependencies |
|-------|--------------|-----------|------------|-------------|
| Phase 1: Capabilities | 3 | 0 | Low | None |
| Phase 2: OpenAI adapter | 1 | 5-6 | High | Phase 1 |
| Phase 3: Fallback routing | 3 | 0 | Medium | Phase 1 |
| Phase 4: Ollama provider | 1-2 | 1-2 | Low | Phase 2 |
| Phase 5: Cost routing | 3 | 0 | Medium | Phase 1 |

Phases 1, 2, and 3 are the critical path. Phase 1 is a prerequisite for phases 2-5. Phases 2 and 3 can proceed in parallel after phase 1. Phase 4 builds on phase 2. Phase 5 can start after phase 1.

### What NOT to build

- **Provider-neutral type system.** Do not replace `CompletionRequest`/`CompletionResponse` with provider-agnostic types. The Anthropic-native types give full access to Anthropic features. Other providers adapt to what they support. A lowest-common-denominator type system would lose prompt caching, extended thinking, server-side tools, and citations.
- **Load balancer.** Not needed for single-user system. The fallback chain provides sufficient resilience.
- **Token counting library.** Pre-request token estimation is nice-to-have but not blocking. The API reports actual usage, and character-based heuristics work for budgeting.
- **Universal embedding interface.** Embeddings are a separate concern with different providers (Candle local, OpenAI API). Keep them separate from the LLM provider trait.

---

## Gotchas

1. **Tool call format divergence is the hardest translation.** Anthropic uses content blocks within messages. OpenAI uses a separate `tool_calls` array on the assistant message and a dedicated `tool` role for results. The entire tool loop in `crates/nous/src/execute/dispatch.rs` assumes Anthropic's format. The adapter must produce `ContentBlock::ToolUse` and consume `ContentBlock::ToolResult` regardless of the underlying wire format. If the translation is lossy (e.g., OpenAI does not support `disable_passthrough`), the behavior will differ silently.

2. **Streaming accumulator coupling.** `StreamAccumulator` in `crates/hermeneus/src/anthropic/stream.rs` is tightly coupled to Anthropic's SSE event types. An OpenAI streaming adapter needs its own accumulator. Both must produce the same `StreamEvent` variants so the TUI and pipeline code works unchanged. The `StreamEvent` enum itself is defined in the anthropic module and would need to move to the crate root.

3. **Thinking block round-tripping.** Anthropic's extended thinking produces `ContentBlock::Thinking` with a `signature` field. These blocks must be echoed back verbatim in subsequent requests (the signature is a cryptographic attestation). If a conversation starts on Anthropic with thinking enabled and falls back to OpenAI, the thinking blocks in history will be meaningless to OpenAI. The adapter must strip or pass them as text. Switching providers mid-conversation is inherently lossy.

4. **Prompt caching is provider-specific.** Anthropic's prompt caching (`cache_system`, `cache_tools`, `cache_turns`) reduces cost by 90% on cache hits. OpenAI has no equivalent. Switching from Anthropic to OpenAI for the same conversation loses caching benefits and increases cost. Cost-based routing must factor in caching discounts.

5. **Model version pinning.** Anthropic uses versioned model IDs (`claude-opus-4-20250514`). OpenAI uses aliases (`gpt-4o`) that point to rotating versions. The capability registry must handle both: pinned versions with known capabilities and aliases that may change behavior on provider updates.

6. **Rate limit headers differ.** Anthropic returns `retry-after` (seconds). OpenAI returns `retry-after` (seconds) plus `x-ratelimit-remaining-requests`, `x-ratelimit-remaining-tokens`, and `x-ratelimit-reset-requests`. The richer OpenAI headers could inform smarter routing (preemptive switch before hitting the limit) but require provider-specific parsing.

7. **Response quality variance.** Different providers produce different quality outputs for the same prompt. A prompt tuned for Claude may underperform on GPT-4o and vice versa. Automatic routing based on cost or availability may silently degrade output quality. Consider logging which provider handled each request so quality issues can be traced.

---

## References

- `crates/hermeneus/src/provider.rs`: `LlmProvider` trait, `ProviderRegistry`, `ProviderConfig`
- `crates/hermeneus/src/types.rs`: `CompletionRequest`, `CompletionResponse`, `ContentBlock`
- `crates/hermeneus/src/health.rs`: `ProviderHealth` state machine, `ProviderHealthTracker`
- `crates/hermeneus/src/error.rs`: `Error` enum with provider error variants, `is_retryable()`
- `crates/hermeneus/src/fallback.rs`: `FallbackConfig`, `complete_with_fallback()` (intra-provider model fallback)
- `crates/hermeneus/src/anthropic/client.rs`: `AnthropicProvider` implementation
- `crates/hermeneus/src/anthropic/stream.rs`: `StreamAccumulator`, `StreamEvent`
- `crates/hermeneus/src/anthropic/wire/`: Anthropic wire format types
- `crates/nous/src/execute/mod.rs:33-56`: `resolve_provider_checked()` routing logic
- `crates/nous/src/manager.rs`: `NousManager` holds `Arc<ProviderRegistry>`
- `crates/koina/src/credential.rs`: `CredentialProvider` trait
- OpenAI Chat Completions API: https://platform.openai.com/docs/api-reference/chat
- Ollama API: https://github.com/ollama/ollama/blob/main/docs/openai.md
