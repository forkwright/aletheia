# hermeneus

**Purpose:** LLM provider abstraction and Anthropic streaming client.

## Key types

| Type | Purpose |
|------|---------|
| `LlmProvider` | Current public type or boundary; see L3/source for exact fields |
| `ProviderRegistry` | Current public type or boundary; see L3/source for exact fields |
| `FallbackConfig` | Current public type or boundary; see L3/source for exact fields |
| `LoopGuard` | Current public type or boundary; see L3/source for exact fields |
| `CompletionRequest` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `hermeneus::anthropic/batch` - public items from `src/anthropic/batch.rs`
- `hermeneus::anthropic/cc_profile` - public items from `src/anthropic/cc_profile.rs`
- `hermeneus::anthropic/client` - public items from `src/anthropic/client.rs`
- `hermeneus::anthropic/stream` - public items from `src/anthropic/stream/mod.rs`
- `hermeneus::cc/provider` - public items from `src/cc/provider.rs`

## When to look here

- When work touches `crates/hermeneus` or downstream imports from `hermeneus`.
- For exact signatures, load `_llm/L3-api-index/hermeneus.md` if present, then source.

## Recent changes

Fallback chains, token redaction, max_tokens forwarding, and composite loop detection are part of the provider surface.
