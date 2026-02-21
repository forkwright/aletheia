// Execute stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeServices, TurnState, TurnStreamEvent } from "../types.js";

vi.mock("../../../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

vi.mock("../../../hermeneus/token-counter.js", () => ({
  estimateTokens: vi.fn().mockReturnValue(10),
}));

vi.mock("../../../organon/reversibility.js", () => ({
  getReversibility: vi.fn().mockReturnValue("reversible"),
  requiresSimulation: vi.fn().mockReturnValue(false),
}));

vi.mock("../../../organon/timeout.js", () => ({
  executeWithTimeout: vi.fn().mockImplementation(async (fn: () => Promise<string>) => fn()),
  resolveTimeout: vi.fn().mockReturnValue(30000),
  ToolTimeoutError: class extends Error { timeoutMs: number; constructor(m: string, t: number) { super(m); this.timeoutMs = t; } },
}));

vi.mock("../../../organon/approval.js", () => ({
  requiresApproval: vi.fn().mockReturnValue({ required: false }),
}));

vi.mock("../../circuit-breaker.js", () => ({
  checkResponseQuality: vi.fn().mockReturnValue({ triggered: false }),
}));

vi.mock("../../narration-filter.js", () => ({
  NarrationFilter: vi.fn().mockImplementation(() => ({
    feed: vi.fn().mockImplementation((text: string) => [{ type: "text_delta", text }]),
    flush: vi.fn().mockReturnValue([]),
  })),
}));

vi.mock("../../../organon/parallel.js", () => ({
  groupForParallelExecution: vi.fn().mockImplementation((tools: unknown[]) =>
    tools.map((t) => [t]),
  ),
}));

vi.mock("../../../koina/event-bus.js", () => ({
  eventBus: { emit: vi.fn(), on: vi.fn(), off: vi.fn() },
}));

vi.mock("./truncate.js", () => ({
  truncateToolResult: vi.fn().mockImplementation((_name: string, result: string) => result),
}));

import { executeStreaming, executeBuffered } from "./execute.js";

function makeTrace() {
  return {
    finalize: vi.fn(),
    addToolCall: vi.fn(),
    setUsage: vi.fn(),
    setResponseLength: vi.fn(),
    setToolLoops: vi.fn(),
  };
}

function makeLoopDetector() {
  return { record: vi.fn().mockReturnValue({ verdict: "ok" }) };
}

function makeState(overrides?: Record<string, unknown>): TurnState {
  return {
    msg: { text: "hello" },
    nousId: "syl",
    sessionId: "ses_1",
    sessionKey: "main",
    model: "claude-sonnet-4-5-20250514",
    nous: { id: "syl" },
    workspace: "/workspaces/syl",
    seq: 0,
    systemPrompt: [{ type: "text", text: "You are syl." }],
    toolDefs: [],
    toolContext: { nousId: "syl", sessionId: "ses_1", workspace: "/w", allowedRoots: ["/"], depth: 0 },
    trace: makeTrace(),
    totalToolCalls: 0,
    totalInputTokens: 0,
    totalOutputTokens: 0,
    totalCacheReadTokens: 0,
    totalCacheWriteTokens: 0,
    currentMessages: [{ role: "user", content: "hello" }],
    turnToolCalls: [],
    loopDetector: makeLoopDetector(),
    ...overrides,
  } as unknown as TurnState;
}

function makeServices(overrides?: Record<string, unknown>): RuntimeServices {
  return {
    config: {
      agents: {
        defaults: {
          maxOutputTokens: 4096,
          contextTokens: 200000,
          narrationFilter: true,
          toolTimeouts: {},
        },
      },
    },
    store: {
      getThinkingConfig: vi.fn().mockReturnValue({ enabled: false, budget: 8000 }),
      recordUsage: vi.fn(),
      appendMessage: vi.fn(),
      drainQueue: vi.fn().mockReturnValue([]),
    },
    router: {
      complete: vi.fn(),
      completeStreaming: vi.fn(),
    },
    tools: {
      execute: vi.fn().mockResolvedValue("tool output"),
      expireUnusedTools: vi.fn(),
      recordToolUse: vi.fn(),
      hasTools: vi.fn().mockReturnValue(true),
    },
    ...overrides,
  } as unknown as RuntimeServices;
}

describe("executeBuffered", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("completes a text-only response turn", async () => {
    const services = makeServices();
    (services.router.complete as ReturnType<typeof vi.fn>).mockResolvedValue({
      content: [{ type: "text", text: "Hello back!" }],
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 80, cacheWriteTokens: 10 },
      model: "claude-sonnet-4-5-20250514",
    });

    const result = await executeBuffered(makeState(), services);

    expect(result.outcome).toBeDefined();
    expect(result.outcome!.text).toBe("Hello back!");
    expect(result.outcome!.toolCalls).toBe(0);
    expect(result.totalInputTokens).toBe(100);
    expect(result.totalOutputTokens).toBe(50);
    expect(services.store.appendMessage).toHaveBeenCalledWith("ses_1", "assistant", "Hello back!", expect.any(Object));
    expect(services.store.recordUsage).toHaveBeenCalled();
  });

  it("executes a single tool call then completes", async () => {
    const services = makeServices();
    const completeFn = services.router.complete as ReturnType<typeof vi.fn>;

    // First call: tool use
    completeFn.mockResolvedValueOnce({
      content: [{ type: "tool_use", id: "tu_1", name: "read", input: { path: "/test" } }],
      usage: { inputTokens: 100, outputTokens: 30, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-sonnet-4-5-20250514",
    });
    // Second call: text response
    completeFn.mockResolvedValueOnce({
      content: [{ type: "text", text: "File content is: hello" }],
      usage: { inputTokens: 150, outputTokens: 40, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-sonnet-4-5-20250514",
    });

    const result = await executeBuffered(makeState(), services);

    expect(result.outcome).toBeDefined();
    expect(result.outcome!.text).toBe("File content is: hello");
    expect(result.outcome!.toolCalls).toBe(1);
    expect(result.totalInputTokens).toBe(250);
    expect(services.tools.execute).toHaveBeenCalledWith("read", { path: "/test" }, expect.any(Object));
  });
});

describe("executeStreaming", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("streams text deltas and completes", async () => {
    const services = makeServices();
    (services.router.completeStreaming as ReturnType<typeof vi.fn>).mockReturnValue(
      (async function* () {
        yield { type: "text_delta", text: "Hello " };
        yield { type: "text_delta", text: "back!" };
        yield {
          type: "message_complete",
          result: {
            content: [{ type: "text", text: "Hello back!" }],
            usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 80, cacheWriteTokens: 10 },
            model: "claude-sonnet-4-5-20250514",
          },
        };
      })(),
    );

    const events: TurnStreamEvent[] = [];
    const gen = executeStreaming(makeState(), services);
    let next = await gen.next();
    while (!next.done) {
      events.push(next.value);
      next = await gen.next();
    }

    const textDeltas = events.filter((e) => e.type === "text_delta");
    expect(textDeltas.length).toBe(2);
    expect(next.value.outcome!.text).toBe("Hello back!");
  });

  it("handles tool use in streaming mode", async () => {
    const services = makeServices();
    const streamFn = services.router.completeStreaming as ReturnType<typeof vi.fn>;

    // First: tool use
    streamFn.mockReturnValueOnce(
      (async function* () {
        yield {
          type: "message_complete",
          result: {
            content: [{ type: "tool_use", id: "tu_1", name: "read", input: { path: "/x" } }],
            usage: { inputTokens: 100, outputTokens: 30, cacheReadTokens: 0, cacheWriteTokens: 0 },
            model: "sonnet",
          },
        };
      })(),
    );

    // Second: text
    streamFn.mockReturnValueOnce(
      (async function* () {
        yield { type: "text_delta", text: "Done." };
        yield {
          type: "message_complete",
          result: {
            content: [{ type: "text", text: "Done." }],
            usage: { inputTokens: 150, outputTokens: 20, cacheReadTokens: 0, cacheWriteTokens: 0 },
            model: "sonnet",
          },
        };
      })(),
    );

    const events: TurnStreamEvent[] = [];
    const gen = executeStreaming(makeState(), services);
    let next = await gen.next();
    while (!next.done) {
      events.push(next.value);
      next = await gen.next();
    }

    const toolStarts = events.filter((e) => e.type === "tool_start");
    expect(toolStarts.length).toBe(1);
    expect(next.value.outcome!.toolCalls).toBe(1);
  });

  it("handles abort signal during tool execution", async () => {
    const controller = new AbortController();
    const services = makeServices();
    const streamFn = services.router.completeStreaming as ReturnType<typeof vi.fn>;

    streamFn.mockReturnValueOnce(
      (async function* () {
        yield {
          type: "message_complete",
          result: {
            content: [
              { type: "tool_use", id: "tu_1", name: "read", input: {} },
              { type: "tool_use", id: "tu_2", name: "write", input: {} },
            ],
            usage: { inputTokens: 100, outputTokens: 30, cacheReadTokens: 0, cacheWriteTokens: 0 },
            model: "sonnet",
          },
        };
      })(),
    );

    // Abort after first batch starts
    controller.abort();

    const events: TurnStreamEvent[] = [];
    const gen = executeStreaming(
      makeState({ abortSignal: controller.signal }),
      services,
    );
    let next = await gen.next();
    while (!next.done) {
      events.push(next.value);
      next = await gen.next();
    }

    const aborts = events.filter((e) => e.type === "turn_abort");
    expect(aborts.length).toBe(1);
  });
});
