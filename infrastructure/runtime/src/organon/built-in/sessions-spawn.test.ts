// Sessions spawn tool tests
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mkdtempSync, rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createSessionsSpawnTool } from "./sessions-spawn.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createSessionsSpawnTool", () => {
  it("has valid definition", () => {
    const tool = createSessionsSpawnTool();
    expect(tool.definition.name).toBe("sessions_spawn");
    expect(tool.definition.input_schema.required).toContain("task");
  });

  it("returns error without dispatcher", async () => {
    const tool = createSessionsSpawnTool();
    const result = await tool.execute({ task: "do something" }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("spawns standard task and returns result", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "task done",
        sessionId: "ses_2",
        toolCalls: 1,
        inputTokens: 100,
        outputTokens: 50,
      }),
    };
    const tool = createSessionsSpawnTool(dispatcher as never);
    const result = await tool.execute({ task: "research topic" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.result).toBe("task done");
    expect(parsed.agentId).toBe("syn"); // defaults to caller
  });

  it("spawns as different agent", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done", sessionId: "ses_2", toolCalls: 0, inputTokens: 0, outputTokens: 0,
      }),
    };
    const tool = createSessionsSpawnTool(dispatcher as never);
    await tool.execute({ task: "do thing", agentId: "eiron" }, ctx);
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(expect.objectContaining({
      nousId: "eiron",
    }));
  });

  it("handles timeout gracefully", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockImplementation(() => new Promise((_, reject) =>
        setTimeout(() => reject(new Error("Spawn timeout after 1s")), 50),
      )),
    };
    const tool = createSessionsSpawnTool(dispatcher as never);
    const result = await tool.execute({ task: "slow task", timeoutSeconds: 1 }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("timeout");
  });

  it("requires ephemeralSoul when ephemeral=true", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsSpawnTool(dispatcher as never, "/tmp/shared");
    const result = await tool.execute({ task: "thing", ephemeral: true }, ctx);
    expect(JSON.parse(result).error).toContain("ephemeralSoul is required");
  });

  it("requires shared root for ephemeral agents", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsSpawnTool(dispatcher as never); // no sharedRoot
    const result = await tool.execute({
      task: "thing",
      ephemeral: true,
      ephemeralSoul: "You are a specialist",
    }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("records audit trail when store available", async () => {
    const store = {
      recordCrossAgentCall: vi.fn().mockReturnValue(1),
      updateCrossAgentCall: vi.fn(),
    };
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done", sessionId: "ses_2", toolCalls: 0, inputTokens: 0, outputTokens: 0,
      }),
      store,
    };
    const tool = createSessionsSpawnTool(dispatcher as never);
    await tool.execute({ task: "do thing" }, ctx);
    expect(store.recordCrossAgentCall).toHaveBeenCalledWith(expect.objectContaining({
      sourceNousId: "syn",
      kind: "spawn",
    }));
  });
});
