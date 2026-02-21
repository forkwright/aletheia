// Anthropic provider — direct @anthropic-ai/sdk integration
import Anthropic from "@anthropic-ai/sdk";
import { createLogger } from "../koina/logger.js";
import { ProviderError } from "../koina/errors.js";

const log = createLogger("hermeneus");

export interface ToolDefinition {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  cache_control?: { type: "ephemeral" };
}

export interface ToolUseBlock {
  type: "tool_use";
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface TextBlock {
  type: "text";
  text: string;
}

export interface ToolResultBlock {
  type: "tool_result";
  tool_use_id: string;
  content: string;
  is_error?: boolean;
}

export interface ImageBlock {
  type: "image";
  source: {
    type: "base64";
    media_type: string;
    data: string;
  };
}

export interface ThinkingBlock {
  type: "thinking";
  thinking: string;
  signature?: string;
}

export type ContentBlock = TextBlock | ToolUseBlock | ThinkingBlock;
export type UserContentBlock = TextBlock | ToolResultBlock | ImageBlock;

export interface TurnResult {
  content: ContentBlock[];
  stopReason: string;
  usage: {
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheWriteTokens: number;
  };
  model: string;
}

export interface MessageParam {
  role: "user" | "assistant";
  content: string | ContentBlock[] | UserContentBlock[];
}

export interface ThinkingConfig {
  type: "enabled";
  budget_tokens: number;
}

export interface ContextManagementEdit {
  type: "clear_thinking_20251015" | "clear_tool_uses_20250919";
  trigger?: { type: "input_tokens"; value: number };
  keep?: { type: "tool_uses" | "thinking_turns"; value: number } | "all";
  clear_at_least?: { type: "input_tokens"; value: number };
  exclude_tools?: string[];
  clear_tool_inputs?: boolean;
}

export interface ContextManagement {
  edits: ContextManagementEdit[];
}

export interface CompletionRequest {
  model: string;
  system: string | Array<{ type: "text"; text: string; cache_control?: { type: "ephemeral" } }>;
  messages: MessageParam[];
  tools?: ToolDefinition[];
  maxTokens?: number;
  temperature?: number;
  signal?: AbortSignal;
  thinking?: ThinkingConfig;
  contextManagement?: ContextManagement;
}

export type StreamingEvent =
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_use_start"; index: number; id: string; name: string }
  | { type: "tool_use_end"; index: number }
  | { type: "message_complete"; result: TurnResult };

export class AnthropicProvider {
  private client: Anthropic;
  private isOAuth: boolean;

  constructor(opts?: { apiKey?: string; authToken?: string }) {
    // Support both API key (x-api-key) and OAuth token (Bearer auth)
    // OAuth is used for Max/Pro plan routing
    const authToken = opts?.authToken ?? process.env["ANTHROPIC_AUTH_TOKEN"];
    const apiKey = opts?.apiKey ?? process.env["ANTHROPIC_API_KEY"];

    this.isOAuth = !!authToken;
    if (authToken) {
      this.client = new Anthropic({
        apiKey: null,
        authToken,
        defaultHeaders: {
          "anthropic-beta": "oauth-2025-04-20",
        },
      });
      log.info("Anthropic provider initialized (OAuth)");
    } else {
      this.client = new Anthropic({ apiKey: apiKey ?? null });
      log.info("Anthropic provider initialized (API key)");
    }
  }

  // Build anthropic-beta header — merges OAuth beta with any feature betas.
  // Per-request headers override defaultHeaders, so we must include oauth
  // beta explicitly whenever we set per-request headers.
  private buildBetaHeader(contextManagement?: unknown): string | undefined {
    const betas: string[] = [];
    if (this.isOAuth) betas.push("oauth-2025-04-20");
    if (contextManagement) betas.push("context-management-2025-06-27");
    return betas.length > 0 ? betas.join(",") : undefined;
  }

  // Inject cache_control breakpoints on tools and conversation history
  // to maximize Anthropic prefix caching. Uses 2 of the 4 allowed breakpoints
  // (the other 2 are on the system prompt, set in bootstrap.ts).
  private injectCacheBreakpoints(request: CompletionRequest): {
    system: CompletionRequest["system"];
    messages: Anthropic.Messages.MessageParam[];
    tools?: Anthropic.Messages.Tool[];
  } {
    const cacheHint = { type: "ephemeral" as const };

    // Cache tool definitions: mark last tool
    let tools: Anthropic.Messages.Tool[] | undefined;
    if (request.tools && request.tools.length > 0) {
      tools = request.tools.map((t, i) => ({
        name: t.name,
        description: t.description,
        input_schema: t.input_schema as Anthropic.Messages.Tool.InputSchema,
        ...(i === request.tools!.length - 1 ? { cache_control: cacheHint } : {}),
      }));
    }

    // Cache conversation history: mark last user message's last content block
    const messages = (request.messages as Anthropic.Messages.MessageParam[]).map((m) => ({ ...m }));
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i]!;
      if (msg.role !== "user") continue;

      if (typeof msg.content === "string") {
        messages[i] = {
          role: "user",
          content: [{ type: "text" as const, text: msg.content, cache_control: cacheHint }],
        };
      } else if (Array.isArray(msg.content) && msg.content.length > 0) {
        const blocks = msg.content.map((b) => ({ ...b }));
        (blocks[blocks.length - 1] as Record<string, unknown>)["cache_control"] = cacheHint;
        messages[i] = { role: "user", content: blocks as Anthropic.Messages.ContentBlockParam[] };
      }
      break;
    }

    return { system: request.system, messages, ...(tools ? { tools } : {}) };
  }

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const { model, maxTokens, temperature, signal, thinking, contextManagement } = request;
    const cached = this.injectCacheBreakpoints(request);

    // Build params — thinking requires dropping temperature (API constraint)
    const params: Anthropic.Messages.MessageCreateParamsNonStreaming & Record<string, unknown> = {
      model,
      max_tokens: maxTokens ?? 8192,
      system: typeof cached.system === "string"
        ? cached.system
        : cached.system as Anthropic.Messages.TextBlockParam[],
      messages: cached.messages,
      ...(cached.tools ? { tools: cached.tools } : {}),
      ...(thinking ? {} : temperature !== undefined ? { temperature } : {}),
    };
    if (thinking) params["thinking"] = thinking;
    if (contextManagement) params["context_management"] = contextManagement;

    const betaHeader = this.buildBetaHeader(contextManagement);

    try {
      const response = await this.client.messages.create(params, {
        ...(signal ? { signal } : {}),
        ...(betaHeader ? { headers: { "anthropic-beta": betaHeader } } : {}),
      });

      const usage = response.usage;
      return {
        content: response.content as ContentBlock[],
        stopReason: response.stop_reason ?? "end_turn",
        usage: {
          inputTokens: usage.input_tokens,
          outputTokens: usage.output_tokens,
          cacheReadTokens:
            (usage as unknown as Record<string, number>)["cache_read_input_tokens"] ?? 0,
          cacheWriteTokens:
            (usage as unknown as Record<string, number>)["cache_creation_input_tokens"] ?? 0,
        },
        model: response.model,
      };
    } catch (error) {
      if (error instanceof Anthropic.APIError) {
        const status = error.status;
        log.error(`Anthropic API ${status}: ${error.message}`);

        const code = status === 429 ? "PROVIDER_RATE_LIMITED" as const
          : status === 529 ? "PROVIDER_OVERLOADED" as const
          : (status === 401 || status === 403) ? "PROVIDER_AUTH_FAILED" as const
          : "PROVIDER_INVALID_RESPONSE" as const;

        const recoverable = status === 429 || status === 529 || status >= 500;
        throw new ProviderError(
          `Anthropic API error: ${status} ${error.message}`,
          {
            cause: error,
            code,
            recoverable,
            ...(status === 429 ? { retryAfterMs: 60_000 } : status === 529 ? { retryAfterMs: 30_000 } : {}),
            context: { status, model },
          },
        );
      }
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Anthropic request failed: ${msg}`);
      throw new ProviderError(`Anthropic request failed: ${msg}`, {
        cause: error, code: "PROVIDER_TIMEOUT", context: { model },
      });
    }
  }

  async *completeStreaming(request: CompletionRequest): AsyncGenerator<StreamingEvent> {
    const { model, maxTokens, temperature, signal, thinking, contextManagement } = request;
    const cached = this.injectCacheBreakpoints(request);

    const streamParams: Anthropic.Messages.MessageCreateParamsStreaming & Record<string, unknown> = {
      model,
      max_tokens: maxTokens ?? 8192,
      stream: true,
      system: typeof cached.system === "string"
        ? cached.system
        : cached.system as Anthropic.Messages.TextBlockParam[],
      messages: cached.messages,
      ...(cached.tools ? { tools: cached.tools } : {}),
      ...(thinking ? {} : temperature !== undefined ? { temperature } : {}),
    };
    if (thinking) streamParams["thinking"] = thinking;
    if (contextManagement) streamParams["context_management"] = contextManagement;

    const betaHeader = this.buildBetaHeader(contextManagement);

    const stream = await this.client.messages.create(streamParams, {
      ...(signal ? { signal } : {}),
      ...(betaHeader ? { headers: { "anthropic-beta": betaHeader } } : {}),
    });

    const contentBlocks: ContentBlock[] = [];
    let stopReason = "end_turn";
    const usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 };
    let responseModel = model;

    // Track in-progress content blocks by index
    const blockState = new Map<number, { type: string; id?: string; name?: string; text?: string; jsonParts?: string[]; signature?: string }>();

    for await (const event of stream) {
      switch (event.type) {
        case "message_start": {
          const msg = event.message;
          responseModel = msg.model;
          const u = msg.usage;
          usage.inputTokens = u.input_tokens;
          usage.outputTokens = u.output_tokens;
          usage.cacheReadTokens = (u as unknown as Record<string, number>)["cache_read_input_tokens"] ?? 0;
          usage.cacheWriteTokens = (u as unknown as Record<string, number>)["cache_creation_input_tokens"] ?? 0;
          break;
        }

        case "content_block_start": {
          const block = event.content_block;
          if (block.type === "text") {
            blockState.set(event.index, { type: "text", text: "" });
          } else if (block.type === "tool_use") {
            blockState.set(event.index, { type: "tool_use", id: block.id, name: block.name, jsonParts: [] });
            yield { type: "tool_use_start", index: event.index, id: block.id, name: block.name };
          } else if (block.type === "thinking") {
            blockState.set(event.index, { type: "thinking", text: "" });
          }
          break;
        }

        case "content_block_delta": {
          const delta = event.delta;
          if (delta.type === "text_delta") {
            const state = blockState.get(event.index);
            if (state?.type === "text") state.text = (state.text ?? "") + delta.text;
            yield { type: "text_delta", text: delta.text };
          } else if (delta.type === "thinking_delta") {
            const state = blockState.get(event.index);
            if (state?.type === "thinking") state.text = (state.text ?? "") + (delta as unknown as { thinking: string }).thinking;
            yield { type: "thinking_delta", text: (delta as unknown as { thinking: string }).thinking };
          } else if (delta.type === "signature_delta") {
            const state = blockState.get(event.index);
            if (state?.type === "thinking") {
              state.signature = (state.signature ?? "") + (delta as unknown as { signature: string }).signature;
            }
          } else if (delta.type === "input_json_delta") {
            const state = blockState.get(event.index);
            if (state?.jsonParts) state.jsonParts.push(delta.partial_json);
          }
          break;
        }

        case "content_block_stop": {
          const state = blockState.get(event.index);
          if (state?.type === "thinking") {
            contentBlocks.push({
              type: "thinking",
              thinking: state.text ?? "",
              ...(state.signature ? { signature: state.signature } : {}),
            });
          } else if (state?.type === "text") {
            contentBlocks.push({ type: "text", text: state.text ?? "" });
          } else if (state?.type === "tool_use") {
            let input: Record<string, unknown> = {};
            try {
              input = JSON.parse(state.jsonParts?.join("") ?? "{}");
            } catch {
              log.warn("Failed to parse tool_use input JSON from stream");
            }
            contentBlocks.push({
              type: "tool_use",
              id: state.id!,
              name: state.name!,
              input,
            });
            yield { type: "tool_use_end", index: event.index };
          }
          break;
        }

        case "message_delta": {
          stopReason = event.delta.stop_reason ?? "end_turn";
          const deltaUsage = event.usage as unknown as Record<string, number> | undefined;
          if (deltaUsage?.["output_tokens"]) {
            usage.outputTokens = deltaUsage["output_tokens"];
          }
          break;
        }
      }
    }

    yield {
      type: "message_complete",
      result: {
        content: contentBlocks,
        stopReason,
        usage,
        model: responseModel,
      },
    };
  }
}
