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
}
