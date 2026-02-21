// Causal tracing tests
import { beforeEach, describe, expect, it } from "vitest";
import { TraceBuilder } from "./trace.js";

describe("TraceBuilder", () => {
  let builder: TraceBuilder;

  beforeEach(() => {
    builder = new TraceBuilder("ses_1", "syn", 1, "claude-sonnet");
  });

  it("constructs with initial values", () => {
    const trace = builder.finalize();
    expect(trace.sessionId).toBe("ses_1");
    expect(trace.nousId).toBe("syn");
    expect(trace.turnSeq).toBe(1);
    expect(trace.model).toBe("claude-sonnet");
    expect(trace.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(trace.toolCalls).toEqual([]);
    expect(trace.crossAgentCalls).toEqual([]);
    expect(trace.inputTokens).toBe(0);
    expect(trace.outputTokens).toBe(0);
  });

  it("setBootstrap records files and tokens", () => {
    builder.setBootstrap(["SOUL.md", "USER.md"], 5000);
    const trace = builder.finalize();
    expect(trace.bootstrapFiles).toEqual(["SOUL.md", "USER.md"]);
    expect(trace.bootstrapTokens).toBe(5000);
  });

  it("setDegradedServices records services", () => {
    builder.setDegradedServices(["neo4j", "qdrant"]);
    const trace = builder.finalize();
    expect(trace.degradedServices).toEqual(["neo4j", "qdrant"]);
  });

  it("addToolCall appends to array", () => {
    builder.addToolCall({ name: "grep", input: { pattern: "test" }, output: "found", durationMs: 50, isError: false });
    builder.addToolCall({ name: "exec", input: { command: "ls" }, output: "files", durationMs: 100, isError: false });
    const trace = builder.finalize();
    expect(trace.toolCalls).toHaveLength(2);
    expect(trace.toolCalls[0]!.name).toBe("grep");
  });

  it("addCrossAgentCall appends to array", () => {
    builder.addCrossAgentCall({ targetNousId: "chiron", message: "schedule", durationMs: 200 });
    const trace = builder.finalize();
    expect(trace.crossAgentCalls).toHaveLength(1);
    expect(trace.crossAgentCalls[0]!.targetNousId).toBe("chiron");
  });

  it("setUsage accumulates tokens", () => {
    builder.setUsage(100, 50, 80, 20);
    builder.setUsage(200, 100, 160, 40);
    const trace = builder.finalize();
    expect(trace.inputTokens).toBe(300);
    expect(trace.outputTokens).toBe(150);
    expect(trace.cacheReadTokens).toBe(240);
    expect(trace.cacheWriteTokens).toBe(60);
  });

  it("setResponseLength records value", () => {
    builder.setResponseLength(500);
    const trace = builder.finalize();
    expect(trace.responseLength).toBe(500);
  });

  it("setToolLoops records count", () => {
    builder.setToolLoops(3);
    const trace = builder.finalize();
    expect(trace.toolLoops).toBe(3);
  });

  it("finalize stamps duration", () => {
    const trace = builder.finalize();
    expect(trace.totalDurationMs).toBeGreaterThanOrEqual(0);
  });
});
