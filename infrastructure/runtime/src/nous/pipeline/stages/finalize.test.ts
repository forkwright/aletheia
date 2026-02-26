// Finalize stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeServices, TurnOutcome, TurnState } from "../types.js";

const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

vi.mock("../../trace.js", () => ({
  persistTrace: vi.fn(),
}));

vi.mock("../../interaction-signals.js", () => ({
  classifyInteraction: vi.fn().mockReturnValue({ signal: "neutral", confidence: 0.4 }),
}));

vi.mock("../../../organon/skill-learner.js", () => ({
  extractSkillCandidate: vi.fn().mockResolvedValue(null),
  saveLearnedSkill: vi.fn(),
}));

vi.mock("../../working-state.js", () => ({
  extractWorkingState: vi.fn().mockResolvedValue(null),
}));

vi.mock("../../turn-facts.js", () => ({
  extractTurnFacts: vi.fn().mockResolvedValue({ facts: [], durationMs: 5 }),
}));

vi.mock("../../../taxis/loader.js", () => ({
  resolveWorkspace: vi.fn().mockReturnValue("/workspaces/syl"),
}));

vi.mock("../../pipeline-config.js", () => ({
  loadPipelineConfig: vi.fn().mockReturnValue({ tools: { expiryTurns: 20 } }),
}));

vi.mock("../../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

vi.mock("../../../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

vi.mock("../../../koina/memory-client.js", () => ({
  getSidecarUrl: vi.fn().mockReturnValue("http://localhost:9231"),
  getUserId: vi.fn().mockReturnValue("default"),
}));

import { finalize, tokenJaccardOverlap } from "./finalize.js";
import { persistTrace } from "../../trace.js";
import { classifyInteraction } from "../../interaction-signals.js";
import { extractSkillCandidate } from "../../../organon/skill-learner.js";
import { eventBus } from "../../../koina/event-bus.js";

function makeOutcome(overrides?: Partial<TurnOutcome>): TurnOutcome {
  return {
    text: "Here is the answer.",
    nousId: "syl",
    sessionId: "ses_1",
    toolCalls: 0,
    inputTokens: 1000,
    outputTokens: 200,
    cacheReadTokens: 800,
    cacheWriteTokens: 100,
    ...overrides,
  };
}

function makeTrace() {
  return {
    finalize: vi.fn().mockReturnValue({ traceId: "t1" }),
    addToolCall: vi.fn(),
    setUsage: vi.fn(),
    setResponseLength: vi.fn(),
    setToolLoops: vi.fn(),
    setBootstrap: vi.fn(),
    setRecall: vi.fn(),
  };
}

function makeState(overrides?: Record<string, unknown>): TurnState {
  return {
    msg: { text: "hello" },
    nousId: "syl",
    sessionId: "ses_1",
    sessionKey: "main",
    nous: { id: "syl" },
    workspace: "/workspaces/syl",
    model: "sonnet",
    seq: 3,
    trace: makeTrace(),
    totalToolCalls: 0,
    totalInputTokens: 1000,
    totalOutputTokens: 200,
    turnToolCalls: [],
    outcome: makeOutcome(),
    ...overrides,
  } as unknown as TurnState;
}

function makeServices(overrides?: Record<string, unknown>): RuntimeServices {
  return {
    config: {
      agents: {
        defaults: {
          compaction: { distillationModel: "haiku" },
        },
      },
    },
    store: {
      updateSessionActualTokens: vi.fn(),
      updateComputedContextTokens: vi.fn(),
      recordSignal: vi.fn(),
      getWorkingState: vi.fn().mockReturnValue(null),
      updateWorkingState: vi.fn(),
    },
    tools: {
      expireUnusedTools: vi.fn().mockReturnValue([]),
    },
    router: {},
    ...overrides,
  } as unknown as RuntimeServices;
}

describe("tokenJaccardOverlap", () => {
  it("returns 1.0 for identical strings", () => {
    expect(tokenJaccardOverlap("the quick brown fox", "the quick brown fox")).toBe(1.0);
  });

  it("returns 0.0 for strings with no shared tokens", () => {
    expect(tokenJaccardOverlap("apple banana cherry", "dinosaur elephant ferret")).toBe(0.0);
  });

  it("computes partial overlap correctly", () => {
    // "the quick brown fox" vs "the quick blue car"
    // Tokens >=3: [the, quick, brown, fox] vs [the, quick, blue, car]
    // Intersection: {the, quick} = 2, Union = 6
    const result = tokenJaccardOverlap("the quick brown fox", "the quick blue car");
    expect(result).toBeCloseTo(2 / 6, 5);
  });

  it("excludes tokens shorter than 3 characters from computation", () => {
    // "hi it is" — all tokens < 3 chars get filtered, so both sets are empty
    const result = tokenJaccardOverlap("hi it", "hi it");
    expect(result).toBe(0);
  });

  it("is case-insensitive", () => {
    const lower = tokenJaccardOverlap("user prefers dark mode", "user prefers dark mode");
    const mixed = tokenJaccardOverlap("User Prefers Dark Mode", "user prefers dark mode");
    expect(lower).toBe(mixed);
  });
});

describe("finalize", () => {
  beforeEach(() => { vi.clearAllMocks(); mockFetch.mockReset(); });

  it("returns early when no outcome", async () => {
    const state = makeState({ outcome: undefined });
    await finalize(state, makeServices());
    expect(persistTrace).not.toHaveBeenCalled();
  });

  it("persists trace to workspace", async () => {
    await finalize(makeState(), makeServices());
    expect(persistTrace).toHaveBeenCalledWith(
      expect.objectContaining({ traceId: "t1" }),
      "/workspaces/syl",
    );
  });

  it("updates session token counts", async () => {
    const services = makeServices();
    await finalize(makeState(), services);
    expect(services.store.updateSessionActualTokens).toHaveBeenCalledWith("ses_1", 1000);
    expect(services.store.updateComputedContextTokens).toHaveBeenCalledWith("ses_1", 1000);
  });

  it("dispatches plugin afterTurn", async () => {
    const dispatchAfterTurn = vi.fn();
    const services = makeServices({ plugins: { dispatchAfterTurn } });
    await finalize(makeState(), services);
    expect(dispatchAfterTurn).toHaveBeenCalledWith(expect.objectContaining({
      nousId: "syl",
      sessionId: "ses_1",
      responseText: "Here is the answer.",
    }));
  });

  it("emits turn:after event", async () => {
    await finalize(makeState(), makeServices());
    expect(eventBus.emit).toHaveBeenCalledWith("turn:after", expect.objectContaining({
      nousId: "syl",
      sessionId: "ses_1",
    }));
  });

  it("classifies interaction signal and records it", async () => {
    vi.mocked(classifyInteraction).mockReturnValue({ signal: "approval", confidence: 0.8 } as never);
    const services = makeServices();
    await finalize(makeState(), services);

    expect(classifyInteraction).toHaveBeenCalledWith("hello", "Here is the answer.");
    expect(services.store.recordSignal).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: "ses_1",
      signal: "approval",
      confidence: 0.8,
    }));
  });

  it("records correction on competence model when signal is correction", async () => {
    vi.mocked(classifyInteraction).mockReturnValue({ signal: "correction", confidence: 0.8 } as never);
    const competence = { recordCorrection: vi.fn(), recordSuccess: vi.fn() };
    const services = makeServices({ competence });
    await finalize(makeState(), services);
    expect(competence.recordCorrection).toHaveBeenCalledWith("syl", "general");
  });

  it("records competence success when tool calls > 0", async () => {
    const competence = { recordCorrection: vi.fn(), recordSuccess: vi.fn() };
    const services = makeServices({ competence });
    await finalize(makeState({ totalToolCalls: 3 }), services);
    expect(competence.recordSuccess).toHaveBeenCalledWith("syl", "general");
  });

  it("triggers skill extraction when >=3 tool calls", async () => {
    const toolCalls = [
      { name: "read", input: {}, output: "content" },
      { name: "write", input: {}, output: "ok" },
      { name: "exec", input: {}, output: "done" },
    ];
    const state = makeState({ turnToolCalls: toolCalls });
    await finalize(state, makeServices());

    // extractSkillCandidate is called async (fire-and-forget), give it a tick
    await new Promise((r) => setTimeout(r, 10));
    expect(extractSkillCandidate).toHaveBeenCalled();
  });

  it("does not trigger skill extraction with <3 tool calls", async () => {
    const state = makeState({ turnToolCalls: [{ name: "read", input: {}, output: "x" }] });
    await finalize(state, makeServices());
    await new Promise((r) => setTimeout(r, 10));
    expect(extractSkillCandidate).not.toHaveBeenCalled();
  });

  it("reinforces a memory with Jaccard overlap >= 0.25 against the response", async () => {
    mockFetch.mockResolvedValue({ ok: true });
    // Response must be > 100 chars to trigger reinforcement
    const responseText = "The user prefers dark mode for all their applications and interfaces, which provides a better experience for night-time work sessions.";
    const memoryTexts = new Map([["mem-001", "User prefers dark mode for applications and interfaces"]]);
    const state = makeState({
      outcome: makeOutcome({ text: responseText }),
      recalledMemoryIds: ["mem-001"],
      recalledMemoryTexts: memoryTexts,
    });
    await finalize(state, makeServices());
    // Allow fire-and-forget to dispatch
    await new Promise((r) => setTimeout(r, 20));
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/evolution/reinforce"),
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("does not reinforce a memory with Jaccard overlap < 0.25 against the response", async () => {
    mockFetch.mockResolvedValue({ ok: true });
    const responseText = "The weather forecast shows sunshine tomorrow in the morning hours";
    // memory text has no meaningful overlap with responseText
    const memoryTexts = new Map([["mem-002", "User builds electronics circuits hobby"]]);
    const state = makeState({
      outcome: makeOutcome({ text: responseText }),
      recalledMemoryIds: ["mem-002"],
      recalledMemoryTexts: memoryTexts,
    });
    await finalize(state, makeServices());
    await new Promise((r) => setTimeout(r, 20));
    expect(mockFetch).not.toHaveBeenCalledWith(
      expect.stringContaining("/evolution/reinforce"),
      expect.any(Object),
    );
  });

  it("does not call reinforce when no recalled memory IDs", async () => {
    mockFetch.mockResolvedValue({ ok: true });
    const state = makeState({
      outcome: makeOutcome({ text: "A long response with many words about various topics" }),
      recalledMemoryIds: [],
      recalledMemoryTexts: new Map(),
    });
    await finalize(state, makeServices());
    await new Promise((r) => setTimeout(r, 20));
    expect(mockFetch).not.toHaveBeenCalledWith(
      expect.stringContaining("/evolution/reinforce"),
      expect.any(Object),
    );
  });

  it("completes without error when reinforce fetch rejects", async () => {
    mockFetch.mockRejectedValue(new Error("network failure"));
    const responseText = "The user prefers dark mode for all their applications and interfaces settings";
    const memoryTexts = new Map([["mem-001", "User prefers dark mode for applications settings"]]);
    const state = makeState({
      outcome: makeOutcome({ text: responseText }),
      recalledMemoryIds: ["mem-001"],
      recalledMemoryTexts: memoryTexts,
    });
    // Should not throw
    await expect(finalize(state, makeServices())).resolves.toBeUndefined();
    await new Promise((r) => setTimeout(r, 20));
  });

  it("only reinforces memories with overlap >= 0.25 in a mixed scenario", async () => {
    mockFetch.mockResolvedValue({ ok: true });
    // Response must be > 100 chars to trigger reinforcement
    const responseText = "User prefers dark mode themes and Fish shell for terminal work sessions, which makes their daily development workflow much more comfortable and productive.";
    const memoryTexts = new Map([
      ["mem-001", "User prefers dark mode themes for their applications"],  // high overlap
      ["mem-002", "User builds robot circuits as a weekend hobby project"],  // low overlap
      ["mem-003", "User uses Fish shell for terminal sessions and scripting"], // high overlap
    ]);
    const state = makeState({
      outcome: makeOutcome({ text: responseText }),
      recalledMemoryIds: ["mem-001", "mem-002", "mem-003"],
      recalledMemoryTexts: memoryTexts,
    });
    await finalize(state, makeServices());
    await new Promise((r) => setTimeout(r, 20));

    const reinforceCalls = mockFetch.mock.calls.filter(
      ([url]: [string]) => typeof url === "string" && url.includes("/evolution/reinforce"),
    );
    const reinforcedIds = reinforceCalls.map(([, init]: [string, RequestInit]) => {
      const body = JSON.parse(init.body as string) as { memory_id: string };
      return body.memory_id;
    });

    expect(reinforcedIds).toContain("mem-001");
    expect(reinforcedIds).toContain("mem-003");
    expect(reinforcedIds).not.toContain("mem-002");
  });
});
