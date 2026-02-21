// Context stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeServices, SystemBlock, TurnState } from "../types.js";

vi.mock("../../../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  updateTurnContext: vi.fn(),
}));

vi.mock("../../../hermeneus/token-counter.js", () => ({
  estimateTokens: vi.fn().mockReturnValue(10),
  estimateToolDefTokens: vi.fn().mockReturnValue(500),
}));

vi.mock("../../bootstrap.js", () => ({
  assembleBootstrap: vi.fn().mockReturnValue({
    staticBlocks: [{ type: "text", text: "static" }],
    dynamicBlocks: [{ type: "text", text: "dynamic" }],
    totalTokens: 2000,
    fileHashes: { "IDENTITY.md": "abc123" },
    contentHash: "hash123",
    droppedFiles: [],
  }),
}));

vi.mock("../../bootstrap-diff.js", () => ({
  detectBootstrapDiff: vi.fn().mockReturnValue(null),
  logBootstrapDiff: vi.fn(),
}));

vi.mock("../../recall.js", () => ({
  recallMemories: vi.fn().mockResolvedValue({ block: null, tokens: 0, count: 0, durationMs: 5 }),
}));

vi.mock("../../working-state.js", () => ({
  formatWorkingState: vi.fn().mockReturnValue("## Working State\nTask: testing"),
}));

vi.mock("../../../distillation/pipeline.js", () => ({
  distillSession: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

import { buildContext } from "./context.js";
import { recallMemories } from "../../recall.js";
import { distillSession } from "../../../distillation/pipeline.js";

function makeTrace() {
  return {
    setBootstrap: vi.fn(),
    setDegradedServices: vi.fn(),
    setRecall: vi.fn(),
    addToolCall: vi.fn(),
    finalize: vi.fn(),
  };
}

function makeState(overrides?: Record<string, unknown>): TurnState {
  return {
    msg: { text: "hello" },
    nousId: "syl",
    sessionId: "ses_1",
    sessionKey: "main",
    nous: { id: "syl", tools: { allow: [], deny: [] }, domains: [] },
    workspace: "/workspaces/syl",
    model: "claude-sonnet-4-5-20250514",
    seq: 0,
    trace: makeTrace(),
    systemPrompt: [],
    toolDefs: [],
    ...overrides,
  } as unknown as TurnState;
}

function makeServices(overrides?: Record<string, unknown>): RuntimeServices {
  return {
    config: {
      agents: {
        defaults: {
          bootstrapMaxTokens: 10000,
          contextTokens: 200000,
          maxOutputTokens: 4096,
          compaction: {
            distillationModel: "haiku",
            preserveRecentMessages: 4,
            preserveRecentMaxTokens: 8000,
          },
        },
      },
    },
    store: {
      updateBootstrapHash: vi.fn(),
      findSessionById: vi.fn().mockReturnValue({
        messageCount: 5,
        workingState: null,
        tokenCountEstimate: 50000,
        lastInputTokens: 50000,
        createdAt: new Date().toISOString(),
      }),
      blackboardReadPrefix: vi.fn().mockReturnValue([]),
      getDistillationPriming: vi.fn().mockReturnValue(null),
      clearDistillationPriming: vi.fn(),
      getNotes: vi.fn().mockReturnValue([]),
      getUsageForSession: vi.fn().mockReturnValue([]),
      getThreadSummary: vi.fn().mockReturnValue(null),
      getThreadForSession: vi.fn().mockReturnValue(null),
    },
    tools: {
      getDefinitions: vi.fn().mockReturnValue([]),
    },
    ...overrides,
  } as unknown as RuntimeServices;
}

describe("buildContext", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("assembles bootstrap and sets system prompt", async () => {
    const state = makeState();
    const result = await buildContext(state, makeServices());

    expect(result.systemPrompt.length).toBeGreaterThanOrEqual(2);
    expect(result.systemPrompt[0]!.text).toBe("static");
    expect(result.systemPrompt[1]!.text).toBe("dynamic");
  });

  it("calculates history budget from context minus bootstrap minus tools minus output", async () => {
    const state = makeState();
    const result = await buildContext(state, makeServices());
    const budget = (result as TurnState & { _historyBudget: number })._historyBudget;

    // 200000 - 2000 (bootstrap) - 500 (tools) - 4096 (maxOutput) - 0 (recall)
    expect(budget).toBe(193404);
  });

  it("recalls memories when sidecar is not degraded", async () => {
    vi.mocked(recallMemories).mockResolvedValue({
      block: { type: "text", text: "recalled memory" } as SystemBlock,
      tokens: 100,
      count: 3,
      durationMs: 20,
    } as never);

    const state = makeState();
    const result = await buildContext(state, makeServices());

    expect(recallMemories).toHaveBeenCalledWith("hello", "syl", expect.any(Object));
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts).toContain("recalled memory");
  });

  it("skips memory recall when sidecar is degraded", async () => {
    const services = makeServices({
      watchdog: {
        getStatus: vi.fn().mockReturnValue([{ name: "mem0-sidecar", healthy: false }]),
      },
    });

    await buildContext(makeState(), services);
    expect(recallMemories).not.toHaveBeenCalled();
  });

  it("injects working state when present", async () => {
    const services = makeServices();
    (services.store.findSessionById as ReturnType<typeof vi.fn>).mockReturnValue({
      messageCount: 5,
      workingState: "doing something",
      tokenCountEstimate: 50000,
    });

    const result = await buildContext(makeState(), services);
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts.some((t) => t.includes("Working State"))).toBe(true);
  });

  it("injects post-distillation priming and clears it", async () => {
    const services = makeServices();
    (services.store.getDistillationPriming as ReturnType<typeof vi.fn>).mockReturnValue({
      distillationNumber: 2,
      facts: ["fact1"],
      decisions: ["decision1"],
      openItems: [],
    });

    const result = await buildContext(makeState(), services);
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts.some((t) => t.includes("Post-Distillation"))).toBe(true);
    expect(texts.some((t) => t.includes("fact1"))).toBe(true);
    expect(services.store.clearDistillationPriming).toHaveBeenCalledWith("ses_1");
  });

  it("injects broadcasts from blackboard", async () => {
    const services = makeServices();
    (services.store.blackboardReadPrefix as ReturnType<typeof vi.fn>).mockReturnValue([
      { key: "broadcast:alert", value: "system update" },
    ]);

    const result = await buildContext(makeState(), services);
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts.some((t) => t.includes("Broadcasts"))).toBe(true);
  });

  it("triggers emergency distillation at >=90% context usage", async () => {
    const services = makeServices();
    (services.store.findSessionById as ReturnType<typeof vi.fn>).mockReturnValue({
      messageCount: 50,
      lastInputTokens: 185000,
      tokenCountEstimate: 185000,
      workingState: null,
    });

    await buildContext(makeState(), services);
    expect(distillSession).toHaveBeenCalled();
  });

  it("does not trigger emergency distillation below 90%", async () => {
    const services = makeServices();
    (services.store.findSessionById as ReturnType<typeof vi.fn>).mockReturnValue({
      messageCount: 10,
      lastInputTokens: 100000,
      tokenCountEstimate: 100000,
      workingState: null,
    });

    await buildContext(makeState(), services);
    expect(distillSession).not.toHaveBeenCalled();
  });

  it("injects session metrics every 8th turn", async () => {
    const services = makeServices();
    (services.store.findSessionById as ReturnType<typeof vi.fn>).mockReturnValue({
      messageCount: 16,
      lastInputTokens: 50000,
      tokenCountEstimate: 50000,
      workingState: null,
      createdAt: new Date(Date.now() - 600000).toISOString(),
      distillationCount: 1,
    });
    (services.store.getUsageForSession as ReturnType<typeof vi.fn>).mockReturnValue([
      { inputTokens: 1000, outputTokens: 500, cacheReadTokens: 800 },
    ]);

    const result = await buildContext(makeState(), services);
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts.some((t) => t.includes("Session Metrics"))).toBe(true);
  });

  it("injects agent notes with token cap", async () => {
    const services = makeServices();
    (services.store.getNotes as ReturnType<typeof vi.fn>).mockReturnValue([
      { category: "task", content: "working on tests" },
      { category: "decision", content: "use vitest" },
    ]);

    const result = await buildContext(makeState(), services);
    const texts = result.systemPrompt.map((b) => b.text);
    expect(texts.some((t) => t.includes("Agent Notes"))).toBe(true);
  });
});
