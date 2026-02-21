// Event bus tests
import { describe, expect, it, vi } from "vitest";
import { eventBus, type EventHandler, type EventName } from "./event-bus.js";

vi.mock("./logger.js", () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

describe("EventBus", () => {
  it("emit fires registered handlers", () => {
    const handler = vi.fn();
    eventBus.on("boot:start", handler);

    eventBus.emit("boot:start", { timestamp: Date.now() });

    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith({ timestamp: expect.any(Number) });

    eventBus.off("boot:start", handler);
  });

  it("emit fires multiple handlers for the same event", () => {
    const handler1 = vi.fn();
    const handler2 = vi.fn();
    eventBus.on("boot:ready", handler1);
    eventBus.on("boot:ready", handler2);

    eventBus.emit("boot:ready", { ok: true });

    expect(handler1).toHaveBeenCalledTimes(1);
    expect(handler2).toHaveBeenCalledTimes(1);

    eventBus.off("boot:ready", handler1);
    eventBus.off("boot:ready", handler2);
  });

  it("off removes handlers", () => {
    const handler = vi.fn();
    eventBus.on("session:created", handler);

    eventBus.emit("session:created", { id: "ses_1" });
    expect(handler).toHaveBeenCalledTimes(1);

    eventBus.off("session:created", handler);

    eventBus.emit("session:created", { id: "ses_2" });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("off with unregistered handler is a no-op", () => {
    const handler = vi.fn();
    // Should not throw
    eventBus.off("session:archived", handler);
  });

  it("emit with no handlers is a no-op", () => {
    // Should not throw
    eventBus.emit("memory:added", { fact: "test" });
  });

  it("streaming event types are valid", () => {
    const streamingEvents: EventName[] = [
      "turn:text_delta",
      "turn:tool_start",
      "turn:tool_result",
    ];

    for (const eventName of streamingEvents) {
      const handler = vi.fn();
      eventBus.on(eventName, handler);

      eventBus.emit(eventName, { data: "test" });
      expect(handler).toHaveBeenCalledTimes(1);

      eventBus.off(eventName, handler);
    }
  });

  it("errors in sync handlers do not block other handlers", () => {
    const badHandler: EventHandler = () => {
      throw new Error("handler blew up");
    };
    const goodHandler = vi.fn();

    eventBus.on("tool:called", badHandler);
    eventBus.on("tool:called", goodHandler);

    // Should not throw
    eventBus.emit("tool:called", { name: "read" });

    expect(goodHandler).toHaveBeenCalledTimes(1);

    eventBus.off("tool:called", badHandler);
    eventBus.off("tool:called", goodHandler);
  });

  it("errors in async handlers do not block other handlers", async () => {
    const badHandler: EventHandler = async () => {
      throw new Error("async handler blew up");
    };
    const goodHandler = vi.fn();

    eventBus.on("tool:failed", badHandler);
    eventBus.on("tool:failed", goodHandler);

    eventBus.emit("tool:failed", { name: "write", error: "denied" });

    expect(goodHandler).toHaveBeenCalledTimes(1);

    // Let async rejection be caught
    await new Promise((r) => setTimeout(r, 10));

    eventBus.off("tool:failed", badHandler);
    eventBus.off("tool:failed", goodHandler);
  });

  it("listenerCount returns correct number", () => {
    const h1 = vi.fn();
    const h2 = vi.fn();
    const h3 = vi.fn();

    expect(eventBus.listenerCount("distill:before")).toBe(0);

    eventBus.on("distill:before", h1);
    expect(eventBus.listenerCount("distill:before")).toBe(1);

    eventBus.on("distill:before", h2);
    expect(eventBus.listenerCount("distill:before")).toBe(2);

    eventBus.on("distill:before", h3);
    expect(eventBus.listenerCount("distill:before")).toBe(3);

    eventBus.off("distill:before", h2);
    expect(eventBus.listenerCount("distill:before")).toBe(2);

    eventBus.off("distill:before", h1);
    eventBus.off("distill:before", h3);
    expect(eventBus.listenerCount("distill:before")).toBe(0);
  });

  it("listenerCount returns 0 for events with no registered handlers", () => {
    expect(eventBus.listenerCount("signal:received")).toBe(0);
  });

  it("handler receives the exact payload passed to emit", () => {
    const handler = vi.fn();
    const payload = { sessionId: "ses_42", nousId: "chiron", tokens: 1500 };

    eventBus.on("turn:before", handler);
    eventBus.emit("turn:before", payload);

    expect(handler).toHaveBeenCalledWith(payload);

    eventBus.off("turn:before", handler);
  });
});
