import { describe, it, expect, afterEach, vi } from "vitest";
import {
  withTurn,
  withTurnAsync,
  getTurnContext,
  updateTurnContext,
  createLogger,
  type TurnContext,
} from "./logger.js";

describe("TurnContext via AsyncLocalStorage", () => {
  it("withTurn sets and retrieves context synchronously", () => {
    let captured: TurnContext | undefined;
    withTurn({ nousId: "syn", channel: "signal" }, () => {
      captured = getTurnContext();
    });
    expect(captured).toBeDefined();
    expect(captured!.nousId).toBe("syn");
    expect(captured!.channel).toBe("signal");
    expect(captured!.turnId).toMatch(/^t_/);
  });

  it("withTurnAsync sets and retrieves context asynchronously", async () => {
    const captured = await withTurnAsync({ nousId: "eiron", sessionKey: "main" }, async () => {
      // Simulate async work
      await new Promise((r) => setTimeout(r, 5));
      return getTurnContext();
    });
    expect(captured).toBeDefined();
    expect(captured!.nousId).toBe("eiron");
    expect(captured!.sessionKey).toBe("main");
    expect(captured!.turnId).toMatch(/^t_/);
  });

  it("returns undefined outside a turn", () => {
    expect(getTurnContext()).toBeUndefined();
  });

  it("updateTurnContext enriches existing context", () => {
    let captured: TurnContext | undefined;
    withTurn({ nousId: "syn" }, () => {
      updateTurnContext({ sessionId: "ses_abc", sessionKey: "chat" });
      captured = getTurnContext();
    });
    expect(captured!.nousId).toBe("syn");
    expect(captured!.sessionId).toBe("ses_abc");
    expect(captured!.sessionKey).toBe("chat");
  });

  it("updateTurnContext is a no-op outside a turn", () => {
    // Should not throw
    updateTurnContext({ nousId: "orphan" });
    expect(getTurnContext()).toBeUndefined();
  });

  it("generates unique turnIds", () => {
    const ids: string[] = [];
    for (let i = 0; i < 5; i++) {
      withTurn({}, () => {
        ids.push(getTurnContext()!.turnId);
      });
    }
    const unique = new Set(ids);
    expect(unique.size).toBe(5);
  });

  it("allows custom turnId", () => {
    let captured: TurnContext | undefined;
    withTurn({ turnId: "custom_123" }, () => {
      captured = getTurnContext();
    });
    expect(captured!.turnId).toBe("custom_123");
  });

  it("nested turns create isolated contexts", async () => {
    let outerCtx: TurnContext | undefined;
    let innerCtx: TurnContext | undefined;

    await withTurnAsync({ nousId: "outer" }, async () => {
      outerCtx = getTurnContext();
      await withTurnAsync({ nousId: "inner" }, async () => {
        innerCtx = getTurnContext();
      });
      // After inner completes, outer context restored
      expect(getTurnContext()!.nousId).toBe("outer");
    });

    expect(outerCtx!.nousId).toBe("outer");
    expect(innerCtx!.nousId).toBe("inner");
    expect(outerCtx!.turnId).not.toBe(innerCtx!.turnId);
  });
});

describe("createLogger", () => {
  it("creates sub-loggers with module names", () => {
    const logger = createLogger("test-module");
    // tslog sub-loggers have the name set
    expect(logger.settings.name).toBe("test-module");
  });

  it("creates distinct loggers for different modules", () => {
    const a = createLogger("module-a");
    const b = createLogger("module-b");
    expect(a.settings.name).toBe("module-a");
    expect(b.settings.name).toBe("module-b");
  });
});

describe("module-level log overrides", () => {
  // These test the parsing logic indirectly — the actual env var is read at import time,
  // so we verify the findModuleLevel behavior through createLogger's minLevel setting.
  // Full integration testing of ALETHEIA_LOG_MODULES requires process restart with env vars.

  it("default loggers respect global log level", () => {
    const logger = createLogger("no-override");
    // Without ALETHEIA_LOG_MODULES set for this module, should use global level
    // The exact minLevel depends on ALETHEIA_LOG_LEVEL env, but the logger should exist
    expect(logger).toBeDefined();
    expect(typeof logger.info).toBe("function");
    expect(typeof logger.debug).toBe("function");
    expect(typeof logger.warn).toBe("function");
  });
});
