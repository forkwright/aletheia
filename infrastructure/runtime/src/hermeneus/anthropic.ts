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

export type ContentBlock = TextBlock | ToolUseBlock;
export type UserContentBlock = TextBlock | ToolResultBlock;

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
}

export class AnthropicProvider {
  private client: Anthropic;

  constructor(apiKey?: string) {
    this.client = new Anthropic({
      apiKey: apiKey ?? process.env.ANTHROPIC_API_KEY,
    });
    log.info("Anthropic provider initialized");
  }

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const { model, system, messages, tools, maxTokens } = request;

    try {
      const response = await this.client.messages.create({
        model,
        max_tokens: maxTokens ?? 8192,
        system: typeof system === "string"
          ? system
          : system as Anthropic.Messages.TextBlockParam[],
        messages: messages as Anthropic.Messages.MessageParam[],
        tools: tools as Anthropic.Messages.Tool[] | undefined,
      });

      const usage = response.usage;
      return {
        content: response.content as ContentBlock[],
        stopReason: response.stop_reason ?? "end_turn",
        usage: {
          inputTokens: usage.input_tokens,
          outputTokens: usage.output_tokens,
          cacheReadTokens:
            (usage as unknown as Record<string, number>).cache_read_input_tokens ?? 0,
          cacheWriteTokens:
            (usage as unknown as Record<string, number>).cache_creation_input_tokens ?? 0,
        },
        model: response.model,
      };
    } catch (error) {
      if (error instanceof Anthropic.APIError) {
        throw new ProviderError(
          `Anthropic API error: ${error.status} ${error.message}`,
          error,
        );
      }
      throw new ProviderError("Anthropic request failed", error);
    }
  }
}
