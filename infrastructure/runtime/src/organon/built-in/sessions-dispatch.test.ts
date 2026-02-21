// Sessions dispatch tool tests — parallel sub-agent batch spawning
import { describe, expect, it, vi } from "vitest";
import { createSessionsDispatchTool } from "./sessions-dispatch.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createSessionsDispatchTool", () => {
  it("has valid definition", () => {
    const tool = createSessionsDispatchTool();
    expect(tool.definition.name).toBe("sessions_dispatch");
    expect(tool.definition.input_schema.required).toContain("tasks");
  });

  it("returns error without dispatcher", async () => {
    const tool = createSessionsDispatchTool();
    const result = await tool.execute({ tasks: [{ task: "do thing" }] }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("returns error for empty tasks array", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({ tasks: [] }, ctx);
    expect(JSON.parse(result).error).toContain("required");
  });

  it("returns error for >10 tasks", async () => {
    const dispatcher = { handleMessage: vi.fn() };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const tasks = Array.from({ length: 11 }, (_, i) => ({ task: `task ${i}` }));
    const result = await tool.execute({ tasks }, ctx);
    expect(JSON.parse(result).error).toContain("Maximum 10");
  });

  it("dispatches single task successfully", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "result 1",
        sessionId: "ses_spawn_1",
        toolCalls: 2,
        inputTokens: 100,
        outputTokens: 50,
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({
      tasks: [{ task: "find all TODOs" }],
    }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.taskCount).toBe(1);
    expect(parsed.succeeded).toBe(1);
    expect(parsed.failed).toBe(0);
    expect(parsed.results[0].status).toBe("success");
    expect(parsed.results[0].result).toBe("result 1");
  });

  it("dispatches multiple tasks in parallel", async () => {
    // Track call order and add delays to verify parallelism
    const callOrder: number[] = [];
    const dispatcher = {
      handleMessage: vi.fn().mockImplementation(async (msg: { sessionKey: string }) => {
        const index = parseInt(msg.sessionKey.split(":").pop()!);
        callOrder.push(index);
        // Stagger returns slightly
        await new Promise(r => setTimeout(r, 10));
        return {
          text: `result for task ${index}`,
          sessionId: `ses_spawn_${index}`,
          toolCalls: 1,
          inputTokens: 100,
          outputTokens: 50,
        };
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({
      tasks: [
        { task: "grep for errors" },
        { task: "grep for warnings" },
        { task: "grep for TODOs" },
      ],
    }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.taskCount).toBe(3);
    expect(parsed.succeeded).toBe(3);
    expect(parsed.results).toHaveLength(3);
    // All dispatched concurrently (handleMessage called 3 times)
    expect(dispatcher.handleMessage).toHaveBeenCalledTimes(3);
  });

  it("handles mixed success and failure", async () => {
    let callCount = 0;
    const dispatcher = {
      handleMessage: vi.fn().mockImplementation(async () => {
        callCount++;
        if (callCount === 2) {
          throw new Error("connection failed");
        }
        return {
          text: `result ${callCount}`,
          sessionId: `ses_${callCount}`,
          toolCalls: 0,
          inputTokens: 50,
          outputTokens: 25,
        };
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({
      tasks: [
        { task: "task 1" },
        { task: "task 2 (will fail)" },
        { task: "task 3" },
      ],
    }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.succeeded).toBe(2);
    expect(parsed.failed).toBe(1);
    expect(parsed.results.find((r: { status: string }) => r.status === "error").error).toContain("connection failed");
  });

  it("passes role to spawned tasks", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 25,
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    await tool.execute({
      tasks: [{ task: "review code", role: "reviewer" }],
    }, ctx);
    // Role should set model override
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        model: expect.stringContaining("sonnet"),
      }),
    );
  });

  it("prepends context to task message", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 25,
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    await tool.execute({
      tasks: [{ task: "find usages", context: "Looking at module X" }],
    }, ctx);
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        text: expect.stringContaining("Looking at module X"),
      }),
    );
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        text: expect.stringContaining("find usages"),
      }),
    );
  });

  it("records audit trail when store available", async () => {
    const store = {
      recordCrossAgentCall: vi.fn().mockReturnValue(1),
      updateCrossAgentCall: vi.fn(),
      logSubAgentCall: vi.fn(),
    };
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 50,
        outputTokens: 25,
      }),
      store,
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    await tool.execute({
      tasks: [{ task: "investigate" }, { task: "explore" }],
    }, ctx);
    expect(store.recordCrossAgentCall).toHaveBeenCalledTimes(2);
    expect(store.logSubAgentCall).toHaveBeenCalledTimes(2);
  });

  it("returns timing data showing parallel savings", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockImplementation(async () => {
        await new Promise(r => setTimeout(r, 50)); // Each takes ~50ms
        return {
          text: "done",
          sessionId: "ses_2",
          toolCalls: 0,
          inputTokens: 50,
          outputTokens: 25,
        };
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({
      tasks: [
        { task: "task 1" },
        { task: "task 2" },
        { task: "task 3" },
      ],
    }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.timing.wallClockMs).toBeDefined();
    expect(parsed.timing.sequentialMs).toBeDefined();
    expect(parsed.timing.savedMs).toBeDefined();
    // Sequential would be ~150ms, wall clock should be ~50ms (parallel)
    // Use generous bounds for CI
    expect(parsed.timing.wallClockMs).toBeLessThan(parsed.timing.sequentialMs);
  });

  it("parses structured results from sub-agents", async () => {
    const structuredResponse = `Here's what I found.

\`\`\`json
{
  "role": "explorer",
  "task": "find TODOs",
  "status": "success",
  "summary": "Found 5 TODOs across 3 files",
  "details": {"count": 5},
  "confidence": 0.9
}
\`\`\``;
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: structuredResponse,
        sessionId: "ses_2",
        toolCalls: 3,
        inputTokens: 200,
        outputTokens: 100,
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    const result = await tool.execute({
      tasks: [{ task: "find TODOs", role: "explorer" }],
    }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.results[0].structuredResult).toBeDefined();
    expect(parsed.results[0].structuredResult.status).toBe("success");
    expect(parsed.results[0].structuredResult.confidence).toBe(0.9);
  });

  it("sets depth correctly for sub-agents", async () => {
    const dispatcher = {
      handleMessage: vi.fn().mockResolvedValue({
        text: "done",
        sessionId: "ses_2",
        toolCalls: 0,
        inputTokens: 0,
        outputTokens: 0,
      }),
    };
    const tool = createSessionsDispatchTool(dispatcher as never);
    await tool.execute({
      tasks: [{ task: "do thing" }],
    }, { ...ctx, depth: 2 } as never);
    expect(dispatcher.handleMessage).toHaveBeenCalledWith(
      expect.objectContaining({ depth: 3 }),
    );
  });
});
