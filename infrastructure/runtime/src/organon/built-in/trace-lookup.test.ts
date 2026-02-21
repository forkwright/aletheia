// Trace lookup tool tests
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { traceLookupTool } from "./trace-lookup.js";

let tmpDir: string;
let workspace: string;
let tracesDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "trace-lookup-"));
  workspace = join(tmpDir, "nous", "syn");
  mkdirSync(workspace, { recursive: true });
  tracesDir = join(tmpDir, "shared", "traces");
  mkdirSync(tracesDir, { recursive: true });
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("traceLookupTool", () => {
  it("has valid definition", () => {
    expect(traceLookupTool.definition.name).toBe("trace_lookup");
  });

  it("returns error when no traces file", async () => {
    const result = await traceLookupTool.execute(
      {},
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("No traces found");
  });

  it("reads and returns recent traces", async () => {
    const traces = [
      JSON.stringify({ turnSeq: 1, model: "claude-sonnet", startedAt: "2026-01-01T00:00:00Z", durationMs: 500, toolCalls: [], usage: { input: 100, output: 50 } }),
      JSON.stringify({ turnSeq: 2, model: "claude-sonnet", startedAt: "2026-01-01T00:01:00Z", durationMs: 300, toolCalls: [{ name: "read", durationMs: 10, input: {}, isError: false }], usage: { input: 200, output: 80 } }),
    ];
    writeFileSync(join(tracesDir, "syn.jsonl"), traces.join("\n"));

    const result = await traceLookupTool.execute(
      { turns_back: 5, filter: "all" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.tracesFound).toBe(2);
    expect(parsed.traces[0].turnSeq).toBe(1);
  });

  it("filters by tools only", async () => {
    const trace = JSON.stringify({
      turnSeq: 1, model: "claude-sonnet", startedAt: "2026-01-01T00:00:00Z",
      durationMs: 500, toolCalls: [{ name: "exec", durationMs: 100, input: { command: "ls" }, isError: false }],
      crossAgentCalls: [{ target: "eiron" }], usage: { input: 100, output: 50 },
      bootstrapFiles: ["SOUL.md"],
    });
    writeFileSync(join(tracesDir, "syn.jsonl"), trace);

    const result = await traceLookupTool.execute(
      { filter: "tools" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.traces[0].toolCalls).toBeDefined();
    expect(parsed.traces[0].crossAgentCalls).toBeUndefined();
  });

  it("filters by cross-agent only", async () => {
    const trace = JSON.stringify({
      turnSeq: 1, model: "claude-sonnet", startedAt: "2026-01-01T00:00:00Z",
      durationMs: 500, toolCalls: [{ name: "exec", durationMs: 100 }],
      crossAgentCalls: [{ target: "eiron" }], usage: { input: 100, output: 50 },
    });
    writeFileSync(join(tracesDir, "syn.jsonl"), trace);

    const result = await traceLookupTool.execute(
      { filter: "cross-agent" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.traces[0].crossAgentCalls).toBeDefined();
    expect(parsed.traces[0].toolCalls).toBeUndefined();
  });

  it("respects turns_back limit", async () => {
    const traces = Array.from({ length: 10 }, (_, i) =>
      JSON.stringify({ turnSeq: i, model: "claude-sonnet", startedAt: "2026-01-01T00:00:00Z", durationMs: 100 }),
    );
    writeFileSync(join(tracesDir, "syn.jsonl"), traces.join("\n"));

    const result = await traceLookupTool.execute(
      { turns_back: 3 },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.tracesFound).toBe(3);
  });

  it("handles invalid JSON lines gracefully", async () => {
    writeFileSync(join(tracesDir, "syn.jsonl"), "not json\n" + JSON.stringify({ turnSeq: 1, model: "claude", startedAt: "now", durationMs: 100 }));

    const result = await traceLookupTool.execute(
      {},
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.tracesFound).toBe(1);
  });
});
