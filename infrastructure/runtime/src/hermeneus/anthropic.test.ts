// Anthropic provider tests
import { describe, expect, it, vi } from "vitest";
import { AnthropicProvider } from "./anthropic.js";
import { ProviderError } from "../koina/errors.js";

// Mock the Anthropic SDK
vi.mock("@anthropic-ai/sdk", () => {
  class MockAnthropic {
    messages = {
      create: vi.fn().mockResolvedValue({
        content: [{ type: "text", text: "response" }],
        stop_reason: "end_turn",
        usage: {
          input_tokens: 100,
          output_tokens: 50,
          cache_read_input_tokens: 10,
          cache_creation_input_tokens: 5,
        },
        model: "claude-sonnet-4-6",
      }),
    };
    
  }

  class APIError extends Error {
    status: number | undefined;
    error?: { type?: string };
    constructor(status: number | undefined, message: string, errorBody?: { type?: string }) {
      super(message);
      this.status = status;
      this.error = errorBody;
      this.name = "APIError";
    }
  }

  MockAnthropic.APIError = APIError;
  return { default: MockAnthropic, APIError };
});

describe("AnthropicProvider", () => {
  it("initializes with API key", () => {
    const provider = new AnthropicProvider({ apiKey: "sk-test" });
    expect(provider).toBeDefined();
  });

  it("initializes with auth token", () => {
    const provider = new AnthropicProvider({ authToken: "sk-ant-oat01-test" });
    expect(provider).toBeDefined();
  });

  it("completes a request", async () => {
    const provider = new AnthropicProvider({ apiKey: "sk-test" });
    const result = await provider.complete({
      model: "claude-sonnet-4-6",
      system: "You are helpful",
      messages: [{ role: "user", content: "hello" }],
    });

    expect(result.content[0]).toEqual({ type: "text", text: "response" });
    expect(result.stopReason).toBe("end_turn");
    expect(result.usage.inputTokens).toBe(100);
    expect(result.usage.outputTokens).toBe(50);
    expect(result.usage.cacheReadTokens).toBe(10);
    expect(result.usage.cacheWriteTokens).toBe(5);
  });

  it("passes maxTokens parameter", async () => {
    const provider = new AnthropicProvider({ apiKey: "sk-test" });
    const result = await provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
      maxTokens: 2048,
    });
    expect(result).toBeDefined();
  });

  it("passes tools parameter", async () => {
    const provider = new AnthropicProvider({ apiKey: "sk-test" });
    const result = await provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "read", description: "Read file", input_schema: { type: "object", properties: {} } }],
    });
    expect(result).toBeDefined();
  });

  it("handles API error with rate limit", async () => {
    const Anthropic = (await import("@anthropic-ai/sdk")).default;
    const provider = new AnthropicProvider({ apiKey: "sk-test" });

    const apiError = new (Anthropic as unknown as { APIError: new (s: number, m: string) => Error }).APIError(429, "Rate limited");
    (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } }).client.messages.create = vi.fn().mockRejectedValue(apiError);

    await expect(provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
    })).rejects.toThrow("Anthropic API error");
  });

  it("handles non-API error", async () => {
    const provider = new AnthropicProvider({ apiKey: "sk-test" });

    (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } }).client.messages.create = vi.fn().mockRejectedValue(new Error("Network failure"));

    await expect(provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
    })).rejects.toThrow("Network failure");
  });

  it("maps expired OAuth 401 to PROVIDER_TOKEN_EXPIRED (recoverable: true)", async () => {
    const Anthropic = (await import("@anthropic-ai/sdk")).default;
    const provider = new AnthropicProvider({ authToken: "sk-ant-oat01-test" });

    const expiredErr = new (Anthropic as unknown as { APIError: new (s: number, m: string) => Error }).APIError(
      401,
      "OAuth token has expired. Please obtain a new token or refresh your existing token.",
    );
    (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } })
      .client.messages.create = vi.fn().mockRejectedValue(expiredErr);

    const err = await provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
    }).catch((error) => error);

    expect(err.code).toBe("PROVIDER_TOKEN_EXPIRED");
    expect(err.recoverable).toBe(true);
  });

  it("maps invalid-key 401 to PROVIDER_AUTH_FAILED (recoverable: false)", async () => {
    const Anthropic = (await import("@anthropic-ai/sdk")).default;
    const provider = new AnthropicProvider({ apiKey: "sk-bad-key" });

    const authErr = new (Anthropic as unknown as { APIError: new (s: number, m: string) => Error }).APIError(
      401,
      "Invalid API key",
    );
    (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } })
      .client.messages.create = vi.fn().mockRejectedValue(authErr);

    const err = await provider.complete({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
    }).catch((error) => error);

    expect(err.code).toBe("PROVIDER_AUTH_FAILED");
    expect(err.recoverable).toBe(false);
  });

  async function collectStreamError(provider: AnthropicProvider): Promise<unknown> {
    const gen = provider.completeStreaming({
      model: "claude-sonnet-4-6",
      system: "test",
      messages: [{ role: "user", content: "hi" }],
    });
    try {
      for await (const _event of gen) { /* consume */ }
      return null;
    } catch (error) {
      return error;
    }
  }

  describe("mid-stream error handling", () => {
    it("converts mid-stream 529 APIError to recoverable ProviderError", async () => {
      const provider = new AnthropicProvider({ apiKey: "sk-test" });
      const mockStream = {
        async *[Symbol.asyncIterator]() {
          yield {
            type: "message_start",
            message: {
              model: "claude-sonnet-4-6",
              usage: { input_tokens: 10, output_tokens: 0 },
            },
          };
          const { APIError: MockAPIError } = await import("@anthropic-ai/sdk");
          throw new MockAPIError(529, "Overloaded");
        },
      };
      (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } })
        .client.messages.create = vi.fn().mockResolvedValue(mockStream);

      const err = await collectStreamError(provider) as { code: string; recoverable: boolean; retryAfterMs?: number };
      expect(err).not.toBeNull();
      expect(err.code).toBe("PROVIDER_OVERLOADED");
      expect(err.recoverable).toBe(true);
      expect(err.retryAfterMs).toBe(30_000);
    });

    it("converts mid-stream APIError with undefined status and overloaded_error body to PROVIDER_OVERLOADED", async () => {
      const provider = new AnthropicProvider({ apiKey: "sk-test" });
      const mockStream = {
        async *[Symbol.asyncIterator]() {
          yield {
            type: "message_start",
            message: {
              model: "claude-sonnet-4-6",
              usage: { input_tokens: 10, output_tokens: 0 },
            },
          };
          const { APIError: MockAPIError } = await import("@anthropic-ai/sdk");
          throw new MockAPIError(undefined, "Overloaded", { type: "overloaded_error" });
        },
      };
      (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } })
        .client.messages.create = vi.fn().mockResolvedValue(mockStream);

      const err = await collectStreamError(provider) as { code: string; recoverable: boolean };
      expect(err).not.toBeNull();
      expect(err.code).toBe("PROVIDER_OVERLOADED");
      expect(err.recoverable).toBe(true);
    });

    it("wraps mid-stream non-APIError as recoverable ProviderError", async () => {
      const provider = new AnthropicProvider({ apiKey: "sk-test" });
      const mockStream = {
        async *[Symbol.asyncIterator]() {
          yield {
            type: "message_start",
            message: {
              model: "claude-sonnet-4-6",
              usage: { input_tokens: 10, output_tokens: 0 },
            },
          };
          throw new TypeError("Connection reset");
        },
      };
      (provider as unknown as { client: { messages: { create: ReturnType<typeof vi.fn> } } })
        .client.messages.create = vi.fn().mockResolvedValue(mockStream);

      const err = await collectStreamError(provider) as ProviderError;
      expect(err).toBeInstanceOf(ProviderError);
      expect(err.code).toBe("PROVIDER_INVALID_RESPONSE");
      expect(err.recoverable).toBe(true);
      expect(err.message).toContain("Connection reset");
    });
  });
});
