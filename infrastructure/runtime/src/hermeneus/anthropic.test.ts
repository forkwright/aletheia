// Anthropic provider tests
import { describe, expect, it, vi } from "vitest";
import { AnthropicProvider } from "./anthropic.js";

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
    constructor(_opts: Record<string, unknown>) {}
  }

  class APIError extends Error {
    status: number;
    constructor(status: number, message: string) {
      super(message);
      this.status = status;
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
});
