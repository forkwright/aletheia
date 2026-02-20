// Pipeline runner tests — error boundaries, stage identification
import { beforeEach, describe, expect, it, vi } from "vitest";
import { runBufferedPipeline, runStreamingPipeline } from "./runner.js";

vi.mock("./stages/resolve.js", () => ({
  resolveStage: vi.fn(),
  resolveNousId: vi.fn().mockReturnValue("syn"),
}));

vi.mock("./stages/guard.js", () => ({
  checkGuards: vi.fn().mockReturnValue(null),
}));

vi.mock("./stages/context.js", () => ({
  buildContext: vi.fn(),
}));

vi.mock("./stages/history.js", () => ({
  prepareHistory: vi.fn(),
}));

vi.mock("./stages/execute.js", () => ({
  executeBuffered: vi.fn(),
  executeStreaming: vi.fn(),
}));

vi.mock("./stages/finalize.js", () => ({
  finalize: vi.fn(),
}));

vi.mock("../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

function makeState(overrides: Record<string, unknown> = {}) {
  return {
    nousId: "syn",
    sessionId: "ses_1",
    sessionKey: "main",
    model: "claude-sonnet",
    nous: { id: "syn", name: "Syn" },
    workspace: "/tmp/syn",
    seq: 1,
    systemPrompt: [{ type: "text", text: "system" }],
    messages: [],
    toolDefs: [],
    toolContext: {},
    trace: { addStage: vi.fn(), finalize: vi.fn() },
    totalToolCalls: 0,
    totalInputTokens: 100,
    totalOutputTokens: 50,
    totalCacheReadTokens: 0,
    totalCacheWriteTokens: 0,
    currentMessages: [],
    turnToolCalls: [],
    loopDetector: { check: vi.fn() },
    msg: { text: "hello" },
    ...overrides,
  };
}

const services = {} as never;

describe("runBufferedPipeline", () => {
  beforeEach(() => vi.clearAllMocks());

  it("returns error outcome when context stage throws", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    const { buildContext } = await import("./stages/context.js");

    const state = makeState({ systemPrompt: undefined });
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(state);
    (buildContext as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("context boom"));

    const outcome = await runBufferedPipeline({ text: "hi", nousId: "syn" }, services);

    expect(outcome.error).toBe("context boom");
    expect(outcome.nousId).toBe("syn");
    expect(outcome.sessionId).toBe("ses_1");
    expect(outcome.text).toBe("");
  });

  it("returns error outcome when history stage throws", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    const { buildContext } = await import("./stages/context.js");
    const { prepareHistory } = await import("./stages/history.js");

    const state = makeState();
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(state);
    (buildContext as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
    (prepareHistory as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("history boom"));

    const outcome = await runBufferedPipeline({ text: "hi", nousId: "syn" }, services);

    expect(outcome.error).toBe("history boom");
    expect(outcome.text).toBe("");
  });

  it("emits pipeline:error event on failure", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    const { buildContext } = await import("./stages/context.js");
    const { eventBus } = await import("../../koina/event-bus.js");

    const state = makeState({ systemPrompt: undefined });
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(state);
    (buildContext as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("fail"));

    await runBufferedPipeline({ text: "hi", nousId: "syn" }, services);

    expect(eventBus.emit).toHaveBeenCalledWith("pipeline:error", expect.objectContaining({
      nousId: "syn",
      stage: "context",
      error: "fail",
    }));
  });

  it("returns normal outcome on success", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    const { buildContext } = await import("./stages/context.js");
    const { prepareHistory } = await import("./stages/history.js");
    const { executeBuffered } = await import("./stages/execute.js");
    const { finalize } = await import("./stages/finalize.js");

    const outcome = {
      text: "response",
      nousId: "syn",
      sessionId: "ses_1",
      toolCalls: 0,
      inputTokens: 100,
      outputTokens: 50,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    };
    const state = makeState({ outcome });
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(state);
    (buildContext as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
    (prepareHistory as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
    (executeBuffered as ReturnType<typeof vi.fn>).mockResolvedValue(state);
    (finalize as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);

    const result = await runBufferedPipeline({ text: "hi", nousId: "syn" }, services);

    expect(result.text).toBe("response");
    expect(result.error).toBeUndefined();
  });

  it("throws on unknown nous (resolve returns null)", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(null);

    await expect(runBufferedPipeline({ text: "hi", nousId: "bad" }, services))
      .rejects.toThrow("Unknown nous");
  });
});

describe("runStreamingPipeline", () => {
  beforeEach(() => vi.clearAllMocks());

  it("yields error event when context stage throws", async () => {
    const { resolveStage } = await import("./stages/resolve.js");
    const { buildContext } = await import("./stages/context.js");

    const state = makeState({ systemPrompt: undefined });
    (resolveStage as ReturnType<typeof vi.fn>).mockReturnValue(state);
    (buildContext as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("context boom"));

    const events: unknown[] = [];
    for await (const event of runStreamingPipeline({ text: "hi", nousId: "syn" }, services)) {
      events.push(event);
    }

    const errorEvent = events.find((e: any) => e.type === "error");
    expect(errorEvent).toBeDefined();
    expect((errorEvent as any).message).toContain("context boom");
  });
});
