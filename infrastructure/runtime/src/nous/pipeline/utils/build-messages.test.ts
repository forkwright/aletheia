// build-messages tests — thinking block handling, orphan repair, message construction
import { describe, it, expect, vi } from "vitest";
import { buildMessages } from "./build-messages.js";

vi.mock("../../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

function msg(role: string, content: string, extra: Record<string, unknown> = {}) {
  return { id: 1, sessionId: "s1", seq: 1, role, content, tokenEstimate: 10, isDistilled: false, createdAt: "2026-01-01T00:00:00Z", ...extra };
}

describe("buildMessages", () => {
  it("strips thinking blocks without signatures from history", () => {
    const history = [
      msg("user", "hello"),
      msg("assistant", JSON.stringify([
        { type: "thinking", thinking: "let me think..." },
        { type: "text", text: "response" },
      ])),
    ];

    const result = buildMessages(history as never[], "next message");

    const assistantMsg = result.find(m => m.role === "assistant");
    expect(assistantMsg).toBeDefined();
    const content = assistantMsg!.content as Array<{ type: string }>;
    expect(content).toHaveLength(1);
    expect(content[0]!.type).toBe("text");
  });

  it("preserves thinking blocks with signatures", () => {
    const history = [
      msg("user", "hello"),
      msg("assistant", JSON.stringify([
        { type: "thinking", thinking: "let me think...", signature: "abc123" },
        { type: "text", text: "response" },
      ])),
    ];

    const result = buildMessages(history as never[], "next message");

    const assistantMsg = result.find(m => m.role === "assistant");
    const content = assistantMsg!.content as Array<{ type: string; signature?: string }>;
    expect(content).toHaveLength(2);
    expect(content[0]!.type).toBe("thinking");
    expect(content[0]!.signature).toBe("abc123");
  });

  it("handles assistant message with only thinking blocks (no signature)", () => {
    const history = [
      msg("user", "hello"),
      msg("assistant", JSON.stringify([
        { type: "thinking", thinking: "only thinking here" },
      ])),
    ];

    const result = buildMessages(history as never[], "next message");

    const assistantMsg = result.find(m => m.role === "assistant");
    expect(assistantMsg).toBeDefined();
    expect(assistantMsg!.content).toBe("");
  });

  it("passes through plain text assistant messages unchanged", () => {
    const history = [
      msg("user", "hello"),
      msg("assistant", "plain text response"),
    ];

    const result = buildMessages(history as never[], "next message");

    const assistantMsg = result.find(m => m.role === "assistant");
    expect(assistantMsg!.content).toBe("plain text response");
  });
});
