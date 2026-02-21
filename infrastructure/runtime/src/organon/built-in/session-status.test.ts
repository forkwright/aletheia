// Session status tool tests
import { describe, expect, it, vi } from "vitest";
import { createSessionStatusTool } from "./session-status.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createSessionStatusTool", () => {
  it("has valid definition", () => {
    const tool = createSessionStatusTool();
    expect(tool.definition.name).toBe("session_status");
  });

  it("returns note when store not available", async () => {
    const tool = createSessionStatusTool();
    const result = await tool.execute({}, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.note).toContain("not available");
    expect(parsed.nousId).toBe("syn");
  });

  it("returns session details when store available", async () => {
    const store = {
      findSessionById: vi.fn().mockReturnValue({
        id: "ses_1",
        nousId: "syn",
        model: "claude-sonnet",
        messageCount: 15,
        tokenCountEstimate: 5000,
        status: "active",
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-02T00:00:00Z",
      }),
    };
    const tool = createSessionStatusTool(store as never);
    const result = await tool.execute({}, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.model).toBe("claude-sonnet");
    expect(parsed.messageCount).toBe(15);
    expect(parsed.tokenCount).toBe(5000);
  });

  it("returns error when session not found", async () => {
    const store = {
      findSessionById: vi.fn().mockReturnValue(null),
    };
    const tool = createSessionStatusTool(store as never);
    const result = await tool.execute({}, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("not found");
  });
});
