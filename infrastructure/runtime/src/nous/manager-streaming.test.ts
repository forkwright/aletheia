// Tests for handleMessageStreaming — real-time event delivery via AsyncChannel
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NousManager } from "./manager.js";
import type { TurnStreamEvent } from "./pipeline/types.js";
import { runStreamingPipeline } from "./pipeline/runner.js";

function makeConfig(overrides: Record<string, unknown> = {}) {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn", tools: { allow: [], deny: [] }, heartbeat: null },
      ],
      default: "syn",
      defaults: {
        model: "claude-sonnet",
        contextTokens: 200000,
        maxOutputTokens: 4096,
        bootstrapMaxTokens: 30000,
        narrationFilter: false,
        routing: { enabled: false, tiers: {}, agentOverrides: {} },
        compaction: { maxHistoryShare: 0.7, distillationModel: "claude-haiku" },
      },
    },
    session: { agentToAgent: { maxPingPongTurns: 5 } },
    bindings: [],
    ...overrides,
  } as never;
}

function makeStore() {
  return {
    findSession: vi.fn().mockReturnValue(null),
    findOrCreateSession: vi.fn().mockReturnValue({ id: "ses_1", nousId: "syn", messageCount: 0 }),
    findSessionById: vi.fn().mockReturnValue({ id: "ses_1", nousId: "syn", messageCount: 5, tokenCountEstimate: 1000 }),
    getHistoryWithBudget: vi.fn().mockReturnValue([]),
    appendMessage: vi.fn().mockReturnValue(1),
    recordUsage: vi.fn(),
    updateBootstrapHash: vi.fn(),
    resolveRoute: vi.fn().mockReturnValue(null),
    getUnsurfacedMessages: vi.fn().mockReturnValue([]),
    markMessagesSurfaced: vi.fn(),
    updateSessionActualTokens: vi.fn(),
    getMetrics: vi.fn().mockReturnValue({ usage: {}, perNous: {}, usageByNous: {} }),
    listSessions: vi.fn().mockReturnValue([]),
    recordSignal: vi.fn(),
    getSignalHistory: vi.fn().mockReturnValue([]),
    blackboardReadPrefix: vi.fn().mockReturnValue([]),
    getThreadForSession: vi.fn().mockReturnValue(null),
    getHistory: vi.fn().mockReturnValue([]),
    getThinkingConfig: vi.fn().mockReturnValue({ enabled: false, budget: 10000 }),
    getRecentToolCalls: vi.fn().mockReturnValue([]),
    getNotes: vi.fn().mockReturnValue([]),
    getWorkingState: vi.fn().mockReturnValue(null),
    updateComputedContextTokens: vi.fn(),
    queueMessage: vi.fn(),
    drainQueue: vi.fn().mockReturnValue([]),
    getQueueLength: vi.fn().mockReturnValue(0),
    getDistillationPriming: vi.fn().mockReturnValue(null),
  } as never;
}

function makeRouter() {
  return { complete: vi.fn(), completeStreaming: vi.fn() } as never;
}

function makeTools() {
  return {
    getDefinitions: vi.fn().mockReturnValue([]),
    execute: vi.fn().mockResolvedValue("tool result"),
    recordToolUse: vi.fn(),
    expireUnusedTools: vi.fn().mockReturnValue([]),
    hasTools: vi.fn().mockReturnValue(false),
  } as never;
}

// Mock entire pipeline runner
vi.mock("./pipeline/runner.js", () => ({
  runBufferedPipeline: vi.fn(),
  runStreamingPipeline: vi.fn(),
}));

vi.mock("../melete/pipeline.js", () => ({
  distillSession: vi.fn().mockResolvedValue(undefined),
}));

// Collect all events from an async generator
async function collectEvents<T>(gen: AsyncGenerator<T>): Promise<T[]> {
  const events: T[] = [];
  for await (const e of gen) events.push(e);
  return events;
}

function setupStreamMock(events: TurnStreamEvent[]) {
  (runStreamingPipeline as ReturnType<typeof vi.fn>).mockImplementation(
    async function* () {
      for (const e of events) yield e;
    },
  );
}

describe("handleMessageStreaming", () => {
  let store: ReturnType<typeof makeStore>;
  let tools: ReturnType<typeof makeTools>;

  beforeEach(() => {
    vi.clearAllMocks();
    store = makeStore();
    tools = makeTools();
  });

  it("yields turn_start followed by text_delta and turn_complete", async () => {
    setupStreamMock([
      { type: "turn_start", nousId: "syn", sessionId: "ses_1", model: "claude-sonnet" },
      { type: "text_delta", text: "Hello world" },
      { type: "turn_complete", nousId: "syn", sessionId: "ses_1", text: "Hello world", toolCalls: 0, inputTokens: 100, outputTokens: 50, cacheReadTokens: 10, cacheWriteTokens: 5, model: "claude-sonnet" },
    ]);
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    const events = await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn" }));
    const types = events.map((e) => e.type);
    expect(types[0]).toBe("turn_start");
    expect(types).toContain("text_delta");
    expect(types[types.length - 1]).toBe("turn_complete");

    const textDelta = events.find((e) => e.type === "text_delta");
    expect(textDelta).toHaveProperty("text", "Hello world");
  });

  it("streams events in real-time (not buffered)", async () => {
    (runStreamingPipeline as ReturnType<typeof vi.fn>).mockImplementation(
      async function* (): AsyncGenerator<TurnStreamEvent> {
        yield { type: "turn_start", nousId: "syn", sessionId: "ses_1", model: "claude-sonnet" } as TurnStreamEvent;
        yield { type: "text_delta", text: "chunk1" } as TurnStreamEvent;
        await new Promise((r) => setTimeout(r, 15));
        yield { type: "text_delta", text: "chunk2" } as TurnStreamEvent;
        await new Promise((r) => setTimeout(r, 15));
        yield { type: "turn_complete", nousId: "syn", sessionId: "ses_1", text: "chunk1chunk2", toolCalls: 0, inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0, model: "claude-sonnet" } as TurnStreamEvent;
      },
    );

    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    const gen = manager.handleMessageStreaming({ text: "hi", nousId: "syn" });

    const eventTimestamps: Array<{ type: string; time: number }> = [];
    const start = Date.now();
    for await (const event of gen) {
      eventTimestamps.push({ type: event.type, time: Date.now() - start });
    }

    const textDeltas = eventTimestamps.filter((e) => e.type === "text_delta");
    expect(textDeltas).toHaveLength(2);
    expect(textDeltas[1]!.time - textDeltas[0]!.time).toBeGreaterThanOrEqual(5);
  });

  it("yields tool_start and tool_result during tool loops", async () => {
    setupStreamMock([
      { type: "turn_start", nousId: "syn", sessionId: "ses_1", model: "claude-sonnet" },
      { type: "tool_start", toolName: "read_file", toolId: "tu_1" },
      { type: "tool_result", toolId: "tu_1", toolName: "read_file", result: "file content" },
      { type: "text_delta", text: "Done" },
      { type: "turn_complete", nousId: "syn", sessionId: "ses_1", text: "Done", toolCalls: 1, inputTokens: 80, outputTokens: 30, cacheReadTokens: 0, cacheWriteTokens: 0, model: "claude-sonnet" },
    ]);
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    const events = await collectEvents(manager.handleMessageStreaming({ text: "read files", nousId: "syn" }));
    const types = events.map((e) => e.type);

    expect(types).toContain("tool_start");
    expect(types).toContain("tool_result");
    expect(types).toContain("turn_complete");

    const toolStart = events.find((e) => e.type === "tool_start");
    expect(toolStart).toHaveProperty("toolName", "read_file");
  });

  it("yields error for draining", async () => {
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    manager.isDraining = () => true;
    const events = await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn" }));
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("error");
  });

  it("yields error when depth limit exceeded", async () => {
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    const events = await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn", depth: 10 }));
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("error");
  });

  it("decrements activeTurns after completion", async () => {
    setupStreamMock([
      { type: "turn_start", nousId: "syn", sessionId: "ses_1", model: "claude-sonnet" },
      { type: "turn_complete", nousId: "syn", sessionId: "ses_1", text: "hi", toolCalls: 0, inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0, model: "claude-sonnet" },
    ]);
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    expect(manager.activeTurns).toBe(0);
    await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn" }));
    expect(manager.activeTurns).toBe(0);
  });

  it("decrements activeTurns even after error", async () => {
    (runStreamingPipeline as ReturnType<typeof vi.fn>).mockImplementation(
      // oxlint-disable-next-line require-yield
      async function* () {
        throw new Error("API down");
      },
    );
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    const events = await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn" }));
    expect(manager.activeTurns).toBe(0);
    const errorEvent = events.find((e) => e.type === "error");
    expect(errorEvent).toBeDefined();
  });

  it("records usage after streaming completion", async () => {
    setupStreamMock([
      { type: "turn_start", nousId: "syn", sessionId: "ses_1", model: "claude-sonnet" },
      { type: "turn_complete", nousId: "syn", sessionId: "ses_1", text: "Hello", toolCalls: 0, inputTokens: 100, outputTokens: 50, cacheReadTokens: 10, cacheWriteTokens: 5, model: "claude-sonnet" },
    ]);
    const manager = new NousManager(makeConfig(), store, makeRouter(), tools);
    await collectEvents(manager.handleMessageStreaming({ text: "hi", nousId: "syn" }));

    // Usage recording happens in finalize stage (inside pipeline), which is mocked.
    // Manager itself doesn't call recordUsage for streaming — that's pipeline's job.
    // Verify the pipeline was called correctly instead.
    expect(runStreamingPipeline).toHaveBeenCalledWith(
      expect.objectContaining({ text: "hi", nousId: "syn" }),
      expect.any(Object),
      expect.any(Object),
    );
  });
});
