# Spec 38: Provider Adapter Interface

**Status:** Draft
**Origin:** Issue #298
**Module:** `hermeneus`

---

## Problem

`hermeneus/router.ts` currently has Anthropic hard-wired. Adding a second provider (OpenAI, Gemini, Mistral, local Ollama) requires modifying the router directly — there is no adapter interface. This creates coupling between routing logic and provider implementation, and makes it impossible to test routing decisions without making real API calls.

## Prerequisites

- Provider fallback (use OpenAI if Anthropic is rate-limited)
- Per-agent model overrides (agent A uses Claude Opus, agent B uses local Llama)
- Cost optimization routing (use cheaper model for simple tasks)
- Offline/local operation (Ollama for air-gapped environments)

## Proposed Interface

### `LLMProvider` Adapter

```typescript
export interface LLMMessage {
  role: "user" | "assistant" | "tool_result";
  content: string | ContentBlock[];
}

export interface LLMRequest {
  model: string;
  messages: LLMMessage[];
  systemPrompt?: string;
  tools?: ToolDefinition[];
  maxTokens?: number;
  temperature?: number;
  thinkingBudget?: number;
  stream?: boolean;
}

export interface LLMUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
}

export interface LLMProvider {
  name: string;
  supportsStreaming: boolean;
  supportsThinking: boolean;
  supportedModels: string[];

  complete(request: LLMRequest): Promise<LLMResponse>;
  stream(request: LLMRequest): AsyncIterable<LLMStreamEvent>;
}
```

### Provider Registry

Convention-based discovery: `hermeneus/providers/{name}.ts` exports a factory function. Config maps model IDs to providers.

## Phases

1. Define `LLMProvider` interface + types
2. Extract current Anthropic logic into `AnthropicProvider`
3. Router refactor: provider resolution from config, not hardcoded
4. OpenAI adapter (GPT-4o, o1)
5. Ollama adapter (local models)
6. Provider fallback chain (retry on 429/5xx with next provider)

## Open Questions

- Thinking budget normalization across providers (only Anthropic supports it natively)
- Tool schema translation (Anthropic vs OpenAI function calling formats differ)
- Streaming event normalization
