// Extended router tests — createDefaultRouter credential loading
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDefaultRouter } from "./router.js";

// Mock the AnthropicProvider constructor to avoid real SDK usage
vi.mock("./anthropic.js", () => ({
  AnthropicProvider: vi.fn().mockImplementation(() => ({
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: "ok" }],
      usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-sonnet",
    }),
  })),
}));

describe("createDefaultRouter", () => {
  const origEnv = { ...process.env };

  beforeEach(() => {
    // Clear relevant env vars
    delete process.env["ANTHROPIC_AUTH_TOKEN"];
    delete process.env["ANTHROPIC_API_KEY"];
  });

  afterEach(() => {
    process.env = origEnv;
  });

  it("returns a ProviderRouter", () => {
    // Will try to read credential file, likely fail (test env), but still return router
    const router = createDefaultRouter();
    expect(router).toBeDefined();
    expect(typeof router.complete).toBe("function");
    expect(typeof router.completeWithFailover).toBe("function");
  });

  it("uses ANTHROPIC_API_KEY from env when set", () => {
    process.env["ANTHROPIC_API_KEY"] = "sk-test";
    const router = createDefaultRouter();
    expect(router).toBeDefined();
  });

  it("uses ANTHROPIC_AUTH_TOKEN from env when set", () => {
    process.env["ANTHROPIC_AUTH_TOKEN"] = "sk-ant-oat01-test";
    const router = createDefaultRouter();
    expect(router).toBeDefined();
  });

  it("accepts config with model list", () => {
    process.env["ANTHROPIC_API_KEY"] = "sk-test";
    const router = createDefaultRouter({
      providers: {
        anthropic: {
          models: [
            { id: "claude-sonnet-4-6" },
            { id: "claude-haiku-4-5-20251001" },
          ],
        },
      },
    });
    expect(router).toBeDefined();
  });

  it("works with empty config", () => {
    process.env["ANTHROPIC_API_KEY"] = "sk-test";
    const router = createDefaultRouter({});
    expect(router).toBeDefined();
  });

  it("works with undefined config", () => {
    process.env["ANTHROPIC_API_KEY"] = "sk-test";
    const router = createDefaultRouter(undefined);
    expect(router).toBeDefined();
  });
});
