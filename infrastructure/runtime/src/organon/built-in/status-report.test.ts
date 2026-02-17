// Status report meta-tool tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { SessionStore } from "../../mneme/store.js";
import { createStatusReportTool } from "./status-report.js";
import type { ToolContext } from "../registry.js";

let store: SessionStore;
let tmpDir: string;

const ctx: ToolContext = {
  nousId: "test-agent",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "sr-test-"));
  store = new SessionStore(join(tmpDir, "test.db"));
});

afterEach(() => {
  store.close();
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("createStatusReportTool", () => {
  it("returns tool with correct name", () => {
    const tool = createStatusReportTool(store);
    expect(tool.definition.name).toBe("status_report");
  });

  it("returns structured report", async () => {
    const tool = createStatusReportTool(store);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.agent).toBe("test-agent");
    expect(result.timestamp).toBeTruthy();
    expect(result.blackboard).toBeDefined();
    expect(result.sessions).toBeDefined();
  });

  it("includes blackboard state", async () => {
    store.blackboardWrite("test-key", "test-value", "test-agent");
    const tool = createStatusReportTool(store);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.blackboard.activeKeys).toBe(1);
  });
});
