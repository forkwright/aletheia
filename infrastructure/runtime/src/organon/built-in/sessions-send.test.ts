// Sessions send tool tests
import { describe, it, expect, vi } from "vitest";
import { createSessionsSendTool } from "./sessions-send.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createSessionsSendTool", () => {
  it("has valid definition", () => {
    const tool = createSessionsSendTool();
    expect(tool.definition.name).toBe("sessions_send");
    expect(tool.definition.input_schema.required).toContain("agentId");
    expect(tool.definition.input_schema.required).toContain("message");
  });

  it("returns error without dispatcher", async () => {
    const tool = createSessionsSendTool();
    const result = await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("prevents sending to self", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsSendTool(dispatcher as never);
    const result = await tool.execute({ agentId: "syn", message: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("Cannot send to yourself");
  });

  it("sends fire-and-forget message", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({ text: "ok", sessionId: "ses_2" }),
    };
    const tool = createSessionsSendTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.sent).toBe(true);
    expect(parsed.agentId).toBe("eiron");
  });

  it("uses default session key 'main'", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({ text: "ok", sessionId: "ses_2" }),
    };
    const tool = createSessionsSendTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.sessionKey).toBe("main");
  });

  it("uses custom session key", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({ text: "ok", sessionId: "ses_2" }),
    };
    const tool = createSessionsSendTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "hello", sessionKey: "custom" }, ctx);
    expect(JSON.parse(result).sessionKey).toBe("custom");
  });

  it("records audit trail when store available", async () => {
    const store = {
      recordCrossAgentCall: vi.fn().mockReturnValue(1),
      updateCrossAgentCall: vi.fn(),
    };
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({ text: "ok", sessionId: "ses_2" }),
      store,
    };
    const tool = createSessionsSendTool(dispatcher as never);
    await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    expect(store.recordCrossAgentCall).toHaveBeenCalledWith(expect.objectContaining({
      sourceNousId: "syn",
      targetNousId: "eiron",
      kind: "send",
    }));
  });
});
