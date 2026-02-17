// Anthropic provider — direct @anthropic-ai/sdk integration
import Anthropic from "@anthropic-ai/sdk";
import { createLogger } from "../koina/logger.js";
import { ProviderError } from "../koina/errors.js";

const log = createLogger("hermeneus");

export interface ToolDefinition {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
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

export type ContentBlock = TextBlock | ToolUseBlock;
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

export interface CompletionRequest {
  model: string;
  system: string | Array<{ type: "text"; text: string; cache_control?: { type: "ephemeral" } }>;
  messages: MessageParam[];
  tools?: ToolDefinition[];
  maxTokens?: number;
  temperature?: number;
}

export type StreamingEvent =
  | { type: "text_delta"; text: string }
  | { type: "tool_use_start"; index: number; id: string; name: string }
  | { type: "tool_use_end"; index: number }
  | { type: "message_complete"; result: TurnResult };

export class AnthropicProvider {
  private client: Anthropic;

  constructor(opts?: { apiKey?: string; authToken?: string }) {
    // Support both API key (x-api-key) and OAuth token (Bearer auth)
    // OAuth is used for Max/Pro plan routing
    const authToken = opts?.authToken ?? process.env["ANTHROPIC_AUTH_TOKEN"];
    const apiKey = opts?.apiKey ?? process.env["ANTHROPIC_API_KEY"];

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

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const { model, system, messages, tools, maxTokens, temperature } = request;

    try {
      const response = await this.client.messages.create({
        model,
        max_tokens: maxTokens ?? 8192,
        system: typeof system === "string"
          ? system
          : system as Anthropic.Messages.TextBlockParam[],
        messages: messages as Anthropic.Messages.MessageParam[],
        ...(tools ? { tools: tools as Anthropic.Messages.Tool[] } : {}),
        ...(temperature !== undefined ? { temperature } : {}),
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
    const { model, system, messages, tools, maxTokens, temperature } = request;

    const stream = await this.client.messages.create({
      model,
      max_tokens: maxTokens ?? 8192,
      stream: true,
      system: typeof system === "string"
        ? system
        : system as Anthropic.Messages.TextBlockParam[],
      messages: messages as Anthropic.Messages.MessageParam[],
      ...(tools ? { tools: tools as Anthropic.Messages.Tool[] } : {}),
      ...(temperature !== undefined ? { temperature } : {}),
    });

    const contentBlocks: ContentBlock[] = [];
    let stopReason = "end_turn";
    const usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 };
    let responseModel = model;

    // Track in-progress content blocks by index
    const blockState = new Map<number, { type: string; id?: string; name?: string; text?: string; jsonParts?: string[] }>();

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
          }
          break;
        }

        case "content_block_delta": {
          const delta = event.delta;
          if (delta.type === "text_delta") {
            const state = blockState.get(event.index);
            if (state?.type === "text") state.text = (state.text ?? "") + delta.text;
            yield { type: "text_delta", text: delta.text };
          } else if (delta.type === "input_json_delta") {
            const state = blockState.get(event.index);
            if (state?.jsonParts) state.jsonParts.push(delta.partial_json);
          }
          break;
        }

        case "content_block_stop": {
          const state = blockState.get(event.index);
          if (state?.type === "text") {
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
