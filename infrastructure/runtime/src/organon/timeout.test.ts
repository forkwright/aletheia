// Timeout wrapper tests
import { describe, expect, it } from "vitest";
import {
  DEFAULT_TOOL_TIMEOUTS,
  executeWithTimeout,
  resolveTimeout,
  ToolTimeoutError,
} from "./timeout.js";

describe("resolveTimeout", () => {
  it("returns defaultMs for unknown tool", () => {
    expect(resolveTimeout("some_tool")).toBe(DEFAULT_TOOL_TIMEOUTS.defaultMs);
  });

  it("returns 0 for exec (no framework timeout)", () => {
    expect(resolveTimeout("exec")).toBe(0);
  });

  it("returns 0 for sessions_ask", () => {
    expect(resolveTimeout("sessions_ask")).toBe(0);
  });

  it("returns override for browser", () => {
    expect(resolveTimeout("browser")).toBe(180_000);
  });

  it("returns override for web_fetch", () => {
    expect(resolveTimeout("web_fetch")).toBe(60_000);
  });

  it("respects config overrides", () => {
    expect(resolveTimeout("custom_tool", { overrides: { custom_tool: 5000 } })).toBe(5000);
  });

  it("config defaultMs overrides built-in default", () => {
    expect(resolveTimeout("unknown_tool", { defaultMs: 30_000 })).toBe(30_000);
  });

  it("config override wins over built-in override", () => {
    // exec default is 0; config sets it to 5000
    expect(resolveTimeout("exec", { overrides: { exec: 5000 } })).toBe(5000);
  });
});

describe("executeWithTimeout", () => {
  it("returns result for fast function", async () => {
    const result = await executeWithTimeout(
      () => Promise.resolve("ok"),
      1000,
      "test_tool",
    );
    expect(result).toBe("ok");
  });

  it("skips timeout when timeoutMs is 0", async () => {
    const result = await executeWithTimeout(
      () => new Promise<string>((r) => setTimeout(() => r("done"), 20)),
      0,
      "test_tool",
    );
    expect(result).toBe("done");
  });

  it("throws ToolTimeoutError when function exceeds timeout", async () => {
    const fn = () => new Promise<string>((r) => setTimeout(() => r("late"), 500));
    await expect(executeWithTimeout(fn, 10, "slow_tool")).rejects.toThrow(ToolTimeoutError);
  });

  it("ToolTimeoutError has correct properties", async () => {
    const fn = () => new Promise<string>((r) => setTimeout(() => r("late"), 500));
    let caught: unknown;
    try {
      await executeWithTimeout(fn, 10, "my_tool");
    } catch (err) {
      caught = err;
    }
    expect(caught).toBeInstanceOf(ToolTimeoutError);
    const te = caught as ToolTimeoutError;
    expect(te.toolName).toBe("my_tool");
    expect(te.timeoutMs).toBe(10);
    expect(te.message).toContain("my_tool");
    expect(te.name).toBe("ToolTimeoutError");
  });

  it("propagates non-timeout errors", async () => {
    const fn = () => Promise.reject(new Error("tool error"));
    await expect(executeWithTimeout(fn, 1000, "test_tool")).rejects.toThrow("tool error");
  });

  it("does not throw ToolTimeoutError when function completes just before timeout", async () => {
    const result = await executeWithTimeout(
      () => new Promise<string>((r) => setTimeout(() => r("fast"), 10)),
      500,
      "test_tool",
    );
    expect(result).toBe("fast");
  });
});
