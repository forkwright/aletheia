// Provider router tests
import { describe, expect, it, vi } from "vitest";
import { ProviderRouter } from "./router.js";

function mockProvider(result = { content: [{ type: "text" as const, text: "ok" }], stopReason: "end_turn", usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 }, model: "test" }) {
  return { complete: vi.fn().mockResolvedValue(result) };
}

describe("ProviderRouter", () => {
  it("routes to provider by exact model match", async () => {
    const router = new ProviderRouter();
    const provider = mockProvider();
    router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);
    const result = await router.complete({
      model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
    });
    expect(provider.complete).toHaveBeenCalled();
    expect(result.content[0]).toEqual({ type: "text", text: "ok" });
  });

  it("strips provider prefix for resolution", async () => {
    const router = new ProviderRouter();
    const provider = mockProvider();
    router.registerProvider("anthropic", provider as never, ["claude-opus"]);
    await router.complete({
      model: "anthropic/claude-opus", system: "", messages: [], maxTokens: 100,
    });
    // Should strip prefix and call with "claude-opus"
    const callArg = provider.complete.mock.calls[0]![0];
    expect(callArg.model).toBe("claude-opus");
  });

  it("falls back to first provider for claude-* models", async () => {
    const router = new ProviderRouter();
    const provider = mockProvider();
    router.registerProvider("anthropic", provider as never, []);
    await router.complete({
      model: "claude-new-model", system: "", messages: [], maxTokens: 100,
    });
    expect(provider.complete).toHaveBeenCalled();
  });

  it("throws for non-claude unknown model with no providers", () => {
    const router = new ProviderRouter();
    expect(
      router.complete({ model: "gpt-4", system: "", messages: [], maxTokens: 100 }),
    ).rejects.toThrow("No provider found");
  });

  it("completeWithFailover tries fallback models", async () => {
    const router = new ProviderRouter();
    const provider = mockProvider();
    provider.complete
      .mockRejectedValueOnce(new Error("overloaded"))
      .mockResolvedValueOnce({
        content: [{ type: "text", text: "fallback" }],
        stopReason: "end_turn",
        usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
        model: "claude-haiku",
      });
    router.registerProvider("anthropic", provider as never, ["claude-sonnet", "claude-haiku"]);

    const result = await router.completeWithFailover(
      { model: "claude-sonnet", system: "", messages: [], maxTokens: 100 },
      ["claude-haiku"],
    );
    expect(provider.complete).toHaveBeenCalledTimes(2);
    expect(result.content[0]).toEqual({ type: "text", text: "fallback" });
  });

  it("completeWithFailover rethrows when all fallbacks fail", async () => {
    const router = new ProviderRouter();
    const provider = mockProvider();
    provider.complete.mockRejectedValue(new Error("all down"));
    router.registerProvider("anthropic", provider as never, ["a", "b"]);

    await expect(
      router.completeWithFailover(
        { model: "a", system: "", messages: [], maxTokens: 100 },
        ["b"],
      ),
    ).rejects.toThrow("all down");
  });
});
