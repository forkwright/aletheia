// NousManager tests — orchestration layer around pipeline runner
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NousManager } from "./manager.js";
import { runBufferedPipeline } from "./pipeline/runner.js";

function makeConfig(overrides: Record<string, unknown> = {}) {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn", tools: { allow: [], deny: [] }, heartbeat: null },
        { id: "eiron", name: "Eiron", model: "claude-haiku", workspace: "/tmp/eiron", tools: { allow: [], deny: [] }, heartbeat: null },
      ],
      default: "syn",
      defaults: {
        model: "claude-sonnet",
        contextTokens: 200000,
        maxOutputTokens: 4096,
        bootstrapMaxTokens: 30000,
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
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: "Hello from the model" }],
      stopReason: "end_turn",
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 10, cacheWriteTokens: 5 },
      model: "claude-sonnet",
    }),
  } as never;
}

function makeTools() {
  return {
    getDefinitions: vi.fn().mockReturnValue([]),
    execute: vi.fn().mockResolvedValue("tool result"),
    recordToolUse: vi.fn(),
    expireUnusedTools: vi.fn().mockReturnValue([]),
  } as never;
}

function defaultOutcome(overrides: Record<string, unknown> = {}) {
  return {
    text: "Hello from the model",
    nousId: "syn",
    sessionId: "ses_1",
    model: "claude-sonnet",
    toolCalls: 0,
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 10,
    cacheWriteTokens: 5,
    ...overrides,
  };
}

// Mock the entire pipeline runner — manager tests exercise orchestration, not stage logic
vi.mock("./pipeline/runner.js", () => ({
  runBufferedPipeline: vi.fn().mockImplementation(async () => defaultOutcome()),
  runStreamingPipeline: vi.fn(),
}));

vi.mock("../melete/pipeline.js", () => ({
  distillSession: vi.fn().mockResolvedValue(undefined),
}));

describe("NousManager", () => {
  let manager: NousManager;
  let store: ReturnType<typeof makeStore>;
  let router: ReturnType<typeof makeRouter>;
  let tools: ReturnType<typeof makeTools>;

  beforeEach(() => {
    vi.clearAllMocks();
    store = makeStore();
    router = makeRouter();
    tools = makeTools();
    manager = new NousManager(makeConfig(), store as never, router as never, tools as never);
    // Reset the pipeline mock to default behavior each test
    (runBufferedPipeline as ReturnType<typeof vi.fn>).mockResolvedValue(defaultOutcome());
  });

  it("handles a simple text message", async () => {
    const outcome = await manager.handleMessage({ text: "hello", nousId: "syn" });
    expect(outcome.text).toBe("Hello from the model");
    expect(outcome.nousId).toBe("syn");
    expect(outcome.sessionId).toBe("ses_1");
  });

  it("delegates to runBufferedPipeline with correct args", async () => {
    await manager.handleMessage({ text: "hello", nousId: "syn" });
    expect(runBufferedPipeline).toHaveBeenCalledWith(
      expect.objectContaining({ text: "hello", nousId: "syn" }),
      expect.any(Object),
      expect.objectContaining({ abortSignal: expect.any(AbortSignal) }),
    );
  });

  it("rejects messages when draining", async () => {
    manager.isDraining = () => true;
    await expect(manager.handleMessage({ text: "hi", nousId: "syn" })).rejects.toThrow("shutting down");
  });

  it("rejects cross-agent depth exceeded", async () => {
    await expect(manager.handleMessage({ text: "hi", nousId: "syn", depth: 10 })).rejects.toThrow("depth limit");
  });

  it("rejects unknown nous when pipeline throws", async () => {
    (runBufferedPipeline as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new Error("Unknown nous: unknown_agent"),
    );
    await expect(manager.handleMessage({ text: "hi", nousId: "unknown_agent" })).rejects.toThrow("Unknown nous");
  });

  it("handles tool use outcome from pipeline", async () => {
    (runBufferedPipeline as ReturnType<typeof vi.fn>).mockResolvedValue(defaultOutcome({
      text: "Done",
      toolCalls: 2,
    }));
    const outcome = await manager.handleMessage({ text: "read files", nousId: "syn" });
    expect(outcome.text).toBe("Done");
    expect(outcome.toolCalls).toBe(2);
  });

  it("setPlugins stores plugins for services", async () => {
    const plugins = {
      dispatchBeforeTurn: vi.fn().mockResolvedValue(undefined),
      dispatchAfterTurn: vi.fn().mockResolvedValue(undefined),
    };
    manager.setPlugins(plugins as never);
    await manager.handleMessage({ text: "hello", nousId: "syn" });
    // Verify pipeline received services with plugins
    const call = (runBufferedPipeline as ReturnType<typeof vi.fn>).mock.calls[0];
    const services = call[1];
    expect(services.plugins).toBe(plugins);
  });

  it("setWatchdog stores watchdog for services", async () => {
    const watchdog = {
      getStatus: vi.fn().mockReturnValue([{ name: "neo4j", healthy: false, since: "now" }]),
    };
    manager.setWatchdog(watchdog as never);
    await manager.handleMessage({ text: "hello", nousId: "syn" });
    // Verify pipeline received services with watchdog
    const call = (runBufferedPipeline as ReturnType<typeof vi.fn>).mock.calls[0];
    const services = call[1];
    expect(services.watchdog).toBe(watchdog);
  });

  it("tracks activeTurns", async () => {
    expect(manager.activeTurns).toBe(0);
    const promise = manager.handleMessage({ text: "hello", nousId: "syn" });
    await promise;
    expect(manager.activeTurns).toBe(0);
  });

  it("resolves default nous when nousId not specified", async () => {
    (runBufferedPipeline as ReturnType<typeof vi.fn>).mockResolvedValue(defaultOutcome({ nousId: "syn" }));
    const outcome = await manager.handleMessage({ text: "hello" });
    expect(outcome.nousId).toBe("syn");
  });

  it("returns pipeline outcome including usage", async () => {
    const outcome = await manager.handleMessage({ text: "hello", nousId: "syn" });
    expect(outcome.inputTokens).toBe(100);
    expect(outcome.outputTokens).toBe(50);
  });

  it("triggerDistillation calls distillSession", async () => {
    await manager.triggerDistillation("ses_1");
    const { distillSession } = await import("../melete/pipeline.js");
    expect(distillSession).toHaveBeenCalled();
  });

  it("triggerDistillation throws on unknown session", async () => {
    (store.findSessionById as ReturnType<typeof vi.fn>).mockReturnValue(null);
    await expect(manager.triggerDistillation("unknown")).rejects.toThrow("not found");
  });

  it("passes media attachments through to pipeline", async () => {
    const media = [{ contentType: "image/png", data: "base64data" }];
    await manager.handleMessage({ text: "what is this?", nousId: "syn", media });
    expect(runBufferedPipeline).toHaveBeenCalledWith(
      expect.objectContaining({ media }),
      expect.any(Object),
      expect.any(Object),
    );
  });

  it("returns error from pipeline without throwing", async () => {
    (runBufferedPipeline as ReturnType<typeof vi.fn>).mockResolvedValue(defaultOutcome({
      text: "",
      error: "stage exploded",
    }));
    const outcome = await manager.handleMessage({ text: "hello", nousId: "syn" });
    expect(outcome.error).toBe("stage exploded");
    expect(outcome.text).toBe("");
  });

  it("subsequent turn succeeds after previous pipeline failure", async () => {
    (runBufferedPipeline as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(defaultOutcome({ text: "", error: "first turn fail" }))
      .mockResolvedValueOnce(defaultOutcome());

    const outcome1 = await manager.handleMessage({ text: "fail", nousId: "syn" });
    expect(outcome1.error).toBeDefined();

    const outcome2 = await manager.handleMessage({ text: "succeed", nousId: "syn" });
    expect(outcome2.text).toBe("Hello from the model");
    expect(outcome2.error).toBeUndefined();
  });
});
