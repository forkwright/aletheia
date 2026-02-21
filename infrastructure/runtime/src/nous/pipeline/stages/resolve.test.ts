// Resolve stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InboundMessage, RuntimeServices } from "../types.js";

vi.mock("../../../taxis/loader.js", () => ({
  resolveNous: vi.fn(),
  resolveModel: vi.fn(),
  resolveWorkspace: vi.fn(),
  resolveDefaultNous: vi.fn(),
}));

vi.mock("../../../hermeneus/complexity.js", () => ({
  scoreComplexity: vi.fn(),
  selectModel: vi.fn(),
  selectTemperature: vi.fn(),
}));

vi.mock("../../../taxis/paths.js", () => ({
  paths: { root: "/mock/root" },
}));

vi.mock("../../trace.js", () => ({
  TraceBuilder: vi.fn().mockImplementation(() => ({
    finalize: vi.fn(),
    addToolCall: vi.fn(),
    setUsage: vi.fn(),
    setResponseLength: vi.fn(),
    setToolLoops: vi.fn(),
    setBootstrap: vi.fn(),
    setRecall: vi.fn(),
    setDegradedServices: vi.fn(),
  })),
}));

vi.mock("../../loop-detector.js", () => ({
  LoopDetector: vi.fn().mockImplementation(() => ({
    record: vi.fn().mockReturnValue({ verdict: "ok" }),
  })),
}));

import { resolveNousId, resolveStage } from "./resolve.js";
import { resolveNous, resolveModel, resolveWorkspace, resolveDefaultNous } from "../../../taxis/loader.js";
import { scoreComplexity, selectModel, selectTemperature } from "../../../hermeneus/complexity.js";

function makeMsg(overrides?: Partial<InboundMessage>): InboundMessage {
  return { text: "hello", ...overrides };
}

function makeServices(overrides?: Record<string, unknown>): RuntimeServices {
  return {
    config: {
      agents: {
        defaults: {
          routing: { enabled: false, tiers: {} },
          contextTokens: 200000,
          maxOutputTokens: 4096,
          allowedRoots: [],
          compaction: { distillationModel: "haiku" },
        },
        nous: [],
      },
    },
    store: {
      resolveRoute: vi.fn().mockReturnValue(null),
      findSession: vi.fn().mockReturnValue(null),
      findOrCreateSession: vi.fn().mockReturnValue({ id: "ses_1", messageCount: 0 }),
      getThinkingConfig: vi.fn().mockReturnValue({ enabled: false, budget: 8000 }),
      setThinkingConfig: vi.fn(),
    },
    router: {},
    tools: { hasTools: vi.fn().mockReturnValue(true) },
    ...overrides,
  } as unknown as RuntimeServices;
}

describe("resolveNousId", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("returns explicit nousId from message", () => {
    const id = resolveNousId(makeMsg({ nousId: "chiron" }), makeServices());
    expect(id).toBe("chiron");
  });

  it("routes via channel binding when no explicit nousId", () => {
    const services = makeServices();
    (services.store.resolveRoute as ReturnType<typeof vi.fn>).mockReturnValue("arbor");

    const id = resolveNousId(
      makeMsg({ channel: "signal", peerKind: "user", peerId: "+1234" }),
      services,
    );
    expect(id).toBe("arbor");
    expect(services.store.resolveRoute).toHaveBeenCalledWith("signal", "user", "+1234", undefined);
  });

  it("falls back to default nous when no binding matches", () => {
    vi.mocked(resolveDefaultNous).mockReturnValue({ id: "syl" } as never);

    const id = resolveNousId(makeMsg(), makeServices());
    expect(id).toBe("syl");
  });

  it("falls back to 'syn' when no default configured", () => {
    vi.mocked(resolveDefaultNous).mockReturnValue(null);

    const id = resolveNousId(makeMsg(), makeServices());
    expect(id).toBe("syn");
  });
});

describe("resolveStage", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("returns false for unknown nous", () => {
    vi.mocked(resolveDefaultNous).mockReturnValue({ id: "syl" } as never);
    vi.mocked(resolveNous).mockReturnValue(null);

    const result = resolveStage(makeMsg({ nousId: "unknown" }), makeServices());
    expect(result).toBe(false);
  });

  it("returns TurnState with initialized fields for valid nous", () => {
    vi.mocked(resolveNous).mockReturnValue({
      id: "syl", tools: { allow: [], deny: [] },
      allowedRoots: [],
    } as never);
    vi.mocked(resolveModel).mockReturnValue("claude-sonnet-4-5-20250514");
    vi.mocked(resolveWorkspace).mockReturnValue("/workspaces/syl");

    const result = resolveStage(makeMsg({ nousId: "syl" }), makeServices());
    expect(result).not.toBe(false);
    if (result === false) return;

    expect(result.nousId).toBe("syl");
    expect(result.model).toBe("claude-sonnet-4-5-20250514");
    expect(result.seq).toBe(0);
    expect(result.totalToolCalls).toBe(0);
    expect(result.systemPrompt).toEqual([]);
    expect(result.toolContext.workspace).toBe("/workspaces/syl");
    expect(result.toolContext.allowedRoots).toContain("/mock/root");
  });

  it("auto-enables extended thinking for opus models", () => {
    vi.mocked(resolveNous).mockReturnValue({
      id: "syn", tools: { allow: [], deny: [] }, allowedRoots: [],
    } as never);
    vi.mocked(resolveModel).mockReturnValue("claude-opus-4-20250514");
    vi.mocked(resolveWorkspace).mockReturnValue("/workspaces/syn");

    const services = makeServices();
    const result = resolveStage(makeMsg({ nousId: "syn" }), services);
    expect(result).not.toBe(false);
    expect(services.store.setThinkingConfig).toHaveBeenCalledWith("ses_1", true, 8000);
  });

  it("uses complexity routing when enabled", () => {
    vi.mocked(resolveNous).mockReturnValue({
      id: "syl", tools: { allow: [], deny: [] }, allowedRoots: [],
    } as never);
    vi.mocked(resolveWorkspace).mockReturnValue("/workspaces/syl");
    vi.mocked(scoreComplexity).mockReturnValue({ tier: "complex", score: 75, reason: "long msg" } as never);
    vi.mocked(selectModel).mockReturnValue("claude-opus-4-20250514");
    vi.mocked(selectTemperature).mockReturnValue(0.3);

    const services = makeServices();
    (services.config as Record<string, unknown>).agents = {
      defaults: {
        routing: { enabled: true, tiers: { complex: "claude-opus-4-20250514" }, agentOverrides: {} },
        contextTokens: 200000,
        maxOutputTokens: 4096,
        allowedRoots: [],
        compaction: { distillationModel: "haiku" },
      },
      nous: [],
    };

    const result = resolveStage(makeMsg({ nousId: "syl" }), services);
    expect(result).not.toBe(false);
    if (result === false) return;

    expect(result.model).toBe("claude-opus-4-20250514");
    expect(result.temperature).toBe(0.3);
    expect(scoreComplexity).toHaveBeenCalled();
  });
});
