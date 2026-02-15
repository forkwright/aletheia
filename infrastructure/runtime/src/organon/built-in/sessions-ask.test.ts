// Sessions ask tool tests
import { describe, it, expect, vi } from "vitest";
import { createSessionsAskTool } from "./sessions-ask.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createSessionsAskTool", () => {
  it("has valid definition", () => {
    const tool = createSessionsAskTool();
    expect(tool.definition.name).toBe("sessions_ask");
    expect(tool.definition.input_schema.required).toContain("agentId");
    expect(tool.definition.input_schema.required).toContain("message");
  });

  it("returns error without dispatcher", async () => {
    const tool = createSessionsAskTool();
    const result = await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("prevents asking self", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsAskTool(dispatcher as never);
    const result = await tool.execute({ agentId: "syn", message: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("Cannot ask yourself");
  });

  it("sends and waits for response", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "the answer is 42",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 20,
      }),
    };
    const tool = createSessionsAskTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "what is life?" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.response).toContain("42");
    expect(parsed.agentId).toBe("eiron");
  });

  it("detects disagreement in response", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "I disagree with that assessment",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 20,
      }),
    };
    const tool = createSessionsAskTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "x is true" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.disagreement).toBe("explicit disagreement");
  });

  it("detects correction pattern", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "Actually, that's not correct. The real answer is different.",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 20,
      }),
    };
    const tool = createSessionsAskTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "x is y" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.disagreement).toBeDefined();
  });

  it("handles error from target agent", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockRejectedValue(new Error("Agent busy")),
    };
    const tool = createSessionsAskTool(dispatcher as never);
    const result = await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("Agent busy");
  });

  it("uses default session key ask:<caller>", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "ok", sessionId: "ses_2", toolCalls: 0, inputTokens: 0, outputTokens: 0,
      }),
    };
    const tool = createSessionsAskTool(dispatcher as never);
    await tool.execute({ agentId: "eiron", message: "hello" }, ctx);
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(expect.objectContaining({
      sessionKey: "ask:syn",
    }));
  });
});
