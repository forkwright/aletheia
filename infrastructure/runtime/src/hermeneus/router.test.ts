// Provider router tests — routing, retry with backoff, credential failover
import { describe, expect, it, vi } from "vitest";
import { ProviderRouter } from "./router.js";
import { ProviderError } from "../koina/errors.js";

function mockProvider(result = { content: [{ type: "text" as const, text: "ok" }], stopReason: "end_turn", usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 }, model: "test" }) {
  return { complete: vi.fn().mockResolvedValue(result) };
}

/** Create a ProviderError that the router considers retryable (5xx). */
function transient500(message = "Internal server error"): ProviderError {
  return new ProviderError(`Anthropic API error: 500 ${message}`, {
    code: "PROVIDER_INVALID_RESPONSE",
    recoverable: true,
    context: { status: 500 },
  });
}

/** Create a ProviderError for 529 overloaded. */
function overloaded529(): ProviderError {
  return new ProviderError("Anthropic API error: 529 overloaded", {
    code: "PROVIDER_OVERLOADED",
    recoverable: true,
    retryAfterMs: 30_000,
    context: { status: 529 },
  });
}

/** Create a 429 rate limit error (not retryable — goes to failover). */
function rateLimited429(): ProviderError {
  return new ProviderError("Anthropic API error: 429 rate limited", {
    code: "PROVIDER_RATE_LIMITED",
    recoverable: true,
    retryAfterMs: 60_000,
    context: { status: 429 },
  });
}

/** Create a non-recoverable error (bad request). */
function badRequest400(): ProviderError {
  return new ProviderError("Anthropic API error: 400 bad request", {
    code: "PROVIDER_INVALID_RESPONSE",
    recoverable: false,
    context: { status: 400 },
  });
}

/** Build a router with zero-delay retry for fast tests. */
function fastRouter(): ProviderRouter {
  const router = new ProviderRouter();
  router.setRetryConfig({ maxAttempts: 3, baseDelayMs: 0, maxDelayMs: 0 });
  return router;
}

describe("ProviderRouter", () => {
  describe("routing", () => {
    it("routes to provider by exact model match", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);
      const result = await router.complete({
        model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
      });
      expect(provider.complete).toHaveBeenCalled();
      expect(result.content[0]).toEqual({ type: "text", text: "ok" });
    });

    it("strips provider prefix for resolution", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      router.registerProvider("anthropic", provider as never, ["claude-opus"]);
      await router.complete({
        model: "anthropic/claude-opus", system: "", messages: [], maxTokens: 100,
      });
      const callArg = provider.complete.mock.calls[0]![0];
      expect(callArg.model).toBe("claude-opus");
    });

    it("falls back to first provider for claude-* models", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      router.registerProvider("anthropic", provider as never, []);
      await router.complete({
        model: "claude-new-model", system: "", messages: [], maxTokens: 100,
      });
      expect(provider.complete).toHaveBeenCalled();
    });

    it("throws for non-claude unknown model with no providers", () => {
      const router = fastRouter();
      expect(
        router.complete({ model: "gpt-4", system: "", messages: [], maxTokens: 100 }),
      ).rejects.toThrow("No provider found");
    });
  });

  describe("retry with backoff", () => {
    it("retries transient 500 errors and succeeds", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      provider.complete
        .mockRejectedValueOnce(transient500())
        .mockRejectedValueOnce(transient500())
        .mockResolvedValueOnce({
          content: [{ type: "text", text: "recovered" }],
          stopReason: "end_turn",
          usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
          model: "test",
        });
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);

      const result = await router.complete({
        model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
      });
      expect(provider.complete).toHaveBeenCalledTimes(3);
      expect(result.content[0]).toEqual({ type: "text", text: "recovered" });
    });

    it("retries 529 overloaded errors", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      provider.complete
        .mockRejectedValueOnce(overloaded529())
        .mockResolvedValueOnce({
          content: [{ type: "text", text: "ok" }],
          stopReason: "end_turn",
          usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
          model: "test",
        });
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);

      const result = await router.complete({
        model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
      });
      expect(provider.complete).toHaveBeenCalledTimes(2);
      expect(result.content[0]).toEqual({ type: "text", text: "ok" });
    });

    it("falls through to backup after exhausting retries", async () => {
      const router = fastRouter();
      const primary = mockProvider();
      primary.complete.mockRejectedValue(transient500());

      const backup = mockProvider({
        content: [{ type: "text", text: "backup-ok" }],
        stopReason: "end_turn",
        usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
        model: "test",
      });

      router.registerProvider("anthropic", primary as never, ["claude-sonnet"]);
      router.registerBackupCredentials([backup as never]);

      const result = await router.complete({
        model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
      });
      // 3 retries on primary, then backup
      expect(primary.complete).toHaveBeenCalledTimes(3);
      expect(backup.complete).toHaveBeenCalledTimes(1);
      expect(result.content[0]).toEqual({ type: "text", text: "backup-ok" });
    });

    it("does not retry 429 rate limits — goes straight to failover", async () => {
      const router = fastRouter();
      const primary = mockProvider();
      primary.complete.mockRejectedValue(rateLimited429());

      const backup = mockProvider({
        content: [{ type: "text", text: "backup-ok" }],
        stopReason: "end_turn",
        usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
        model: "test",
      });

      router.registerProvider("anthropic", primary as never, ["claude-sonnet"]);
      router.registerBackupCredentials([backup as never]);

      const result = await router.complete({
        model: "claude-sonnet", system: "", messages: [], maxTokens: 100,
      });
      // Only 1 attempt on primary — no retry for 429
      expect(primary.complete).toHaveBeenCalledTimes(1);
      expect(backup.complete).toHaveBeenCalledTimes(1);
      expect(result.content[0]).toEqual({ type: "text", text: "backup-ok" });
    });

    it("does not retry non-recoverable errors", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      provider.complete.mockRejectedValue(badRequest400());
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);

      await expect(
        router.complete({ model: "claude-sonnet", system: "", messages: [], maxTokens: 100 }),
      ).rejects.toThrow("400");
      expect(provider.complete).toHaveBeenCalledTimes(1);
    });

    it("does not retry non-ProviderError exceptions", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      provider.complete.mockRejectedValue(new Error("unexpected"));
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);

      await expect(
        router.complete({ model: "claude-sonnet", system: "", messages: [], maxTokens: 100 }),
      ).rejects.toThrow("unexpected");
      expect(provider.complete).toHaveBeenCalledTimes(1);
    });

    it("throws after exhausting all retries with no backups", async () => {
      const router = fastRouter();
      const provider = mockProvider();
      provider.complete.mockRejectedValue(transient500());
      router.registerProvider("anthropic", provider as never, ["claude-sonnet"]);

      await expect(
        router.complete({ model: "claude-sonnet", system: "", messages: [], maxTokens: 100 }),
      ).rejects.toThrow("500");
      expect(provider.complete).toHaveBeenCalledTimes(3);
    });
  });

  describe("completeWithFailover", () => {
    it("tries fallback models", async () => {
      const router = fastRouter();
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
      expect(result.content[0]).toEqual({ type: "text", text: "fallback" });
    });

    it("rethrows when all fallbacks fail", async () => {
      const router = fastRouter();
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
});
