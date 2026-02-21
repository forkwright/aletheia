// Finalize stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TurnState, RuntimeServices, TurnOutcome } from "../types.js";

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

vi.mock("../../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

vi.mock("../../../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

import { finalize } from "./finalize.js";
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
    router: {},
    ...overrides,
  } as unknown as RuntimeServices;
}

describe("finalize", () => {
  beforeEach(() => { vi.clearAllMocks(); });

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
});
