// Agent export tests
import { describe, expect, it, vi, beforeEach, afterAll } from "vitest";
import { writeFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

// Fixed test root — created in beforeEach, cleaned in afterAll
const TEST_ROOT = "/tmp/aletheia-export-test";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

vi.mock("../taxis/paths.js", () => {
  const { join: pj } = require("node:path");
  const root = "/tmp/aletheia-export-test";
  return {
    paths: {
      root,
      nous: pj(root, "nous"),
      shared: pj(root, "shared"),
      nousDir: (id: string) => pj(root, "nous", id),
    },
  };
});

import { exportAgent, type AgentFile } from "./export.js";

// --- Mock store ---

function createMockStore(sessions: Array<Record<string, unknown>> = [], messages: Array<Record<string, unknown>> = []) {
  return {
    listSessions: vi.fn((nousId?: string) =>
      sessions.filter((s) => !nousId || s.nousId === nousId),
    ),
    getHistory: vi.fn((_sessionId: string, _opts?: { limit?: number }) =>
      messages,
    ),
    getNotes: vi.fn(() => []),
    findSession: vi.fn(() => null),
  } as unknown as import("../mneme/store.js").SessionStore;
}

function makeSession(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "ses_test_001",
    nousId: "test-agent",
    sessionKey: "main",
    parentSessionId: null,
    status: "active",
    model: "claude-opus-4-6",
    tokenCountEstimate: 5000,
    messageCount: 10,
    lastInputTokens: 1000,
    bootstrapHash: null,
    distillationCount: 2,
    sessionType: "primary",
    lastDistilledAt: "2026-02-21T10:00:00Z",
    computedContextTokens: 50000,
    workingState: {
      currentTask: "Building export",
      completedSteps: ["Phase 1"],
      nextSteps: ["Phase 2"],
      recentDecisions: ["Use JSON format"],
      openFiles: ["export.ts"],
      updatedAt: "2026-02-21T10:00:00Z",
    },
    distillationPriming: null,
    createdAt: "2026-02-20T08:00:00Z",
    updatedAt: "2026-02-21T10:00:00Z",
    ...overrides,
  };
}

function makeMessage(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: 1,
    sessionId: "ses_test_001",
    role: "user",
    content: "Hello, world!",
    seq: 1,
    tokenEstimate: 50,
    isDistilled: false,
    toolCallId: null,
    toolName: null,
    createdAt: "2026-02-21T10:00:00Z",
    ...overrides,
  };
}

describe("exportAgent", () => {
  let agentWorkspace: string;

  beforeEach(() => {
    // Clean and recreate test workspace
    rmSync(TEST_ROOT, { recursive: true, force: true });
    agentWorkspace = join(TEST_ROOT, "nous", "test-agent");
    mkdirSync(agentWorkspace, { recursive: true });
    writeFileSync(join(agentWorkspace, "SOUL.md"), "# Test Agent Soul");
    writeFileSync(join(agentWorkspace, "MEMORY.md"), "# Memory\n\nSome facts.");
    mkdirSync(join(agentWorkspace, "memory"), { recursive: true });
    writeFileSync(join(agentWorkspace, "memory", "2026-02-21.md"), "# Session log");
  });

  afterAll(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
  });

  it("exports basic agent with workspace files", async () => {
    const store = createMockStore([makeSession()], [makeMessage()]);
    const config = { id: "test-agent", name: "Test Agent", model: "claude-opus-4-6" };

    const result = await exportAgent("test-agent", config, store);

    expect(result.version).toBe(1);
    expect(result.exportedAt).toBeTruthy();
    expect(result.generator).toBe("aletheia-export/1.0");
    expect(result.nous.id).toBe("test-agent");
    expect(result.nous.name).toBe("Test Agent");
    expect(result.workspace.files["SOUL.md"]).toBe("# Test Agent Soul");
    expect(result.workspace.files["MEMORY.md"]).toContain("Some facts");
    expect(result.workspace.files["memory/2026-02-21.md"]).toBe("# Session log");
  });

  it("exports sessions with working state", async () => {
    const session = makeSession();
    const messages = [
      makeMessage({ seq: 1, role: "user", content: "What is the system status?" }),
      makeMessage({ seq: 2, role: "assistant", content: "All systems operational." }),
    ];
    const store = createMockStore([session], messages);

    const result = await exportAgent("test-agent", {}, store);

    expect(result.sessions).toHaveLength(1);
    expect(result.sessions[0]!.sessionKey).toBe("main");
    expect(result.sessions[0]!.workingState?.currentTask).toBe("Building export");
    expect(result.sessions[0]!.messages).toHaveLength(2);
    expect(result.sessions[0]!.messages[0]!.role).toBe("user");
    expect(result.sessions[0]!.messages[1]!.role).toBe("assistant");
  });

  it("skips archived sessions by default", async () => {
    const sessions = [
      makeSession({ id: "ses_active", status: "active" }),
      makeSession({ id: "ses_archived", status: "archived" }),
    ];
    const store = createMockStore(sessions, []);

    const result = await exportAgent("test-agent", {}, store);

    expect(result.sessions).toHaveLength(1);
    expect(result.sessions[0]!.id).toBe("ses_active");
  });

  it("includes archived sessions when requested", async () => {
    const sessions = [
      makeSession({ id: "ses_active", status: "active" }),
      makeSession({ id: "ses_archived", status: "archived" }),
    ];
    const store = createMockStore(sessions, []);

    const result = await exportAgent("test-agent", {}, store, {
      includeArchived: true,
    });

    expect(result.sessions).toHaveLength(2);
  });

  it("identifies binary files without including content", async () => {
    writeFileSync(join(agentWorkspace, "avatar.png"), Buffer.from([0x89, 0x50, 0x4e, 0x47]));
    writeFileSync(join(agentWorkspace, "data.db"), "sqlite data");

    const store = createMockStore([], []);
    const result = await exportAgent("test-agent", {}, store);

    expect(result.workspace.binaryFiles).toContain("avatar.png");
    expect(result.workspace.binaryFiles).toContain("data.db");
    expect(result.workspace.files["avatar.png"]).toBeUndefined();
    expect(result.workspace.files["data.db"]).toBeUndefined();
  });

  it("handles missing workspace gracefully", async () => {
    const store = createMockStore([], []);
    const result = await exportAgent("nonexistent-agent", {}, store);

    expect(result.workspace.files).toEqual({});
    expect(result.workspace.binaryFiles).toEqual([]);
  });

  it("skips node_modules and .git directories", async () => {
    mkdirSync(join(agentWorkspace, "node_modules", "dep"), { recursive: true });
    writeFileSync(join(agentWorkspace, "node_modules", "dep", "index.js"), "module.exports = {}");
    mkdirSync(join(agentWorkspace, ".git", "objects"), { recursive: true });
    writeFileSync(join(agentWorkspace, ".git", "HEAD"), "ref: refs/heads/main");

    const store = createMockStore([], []);
    const result = await exportAgent("test-agent", {}, store);

    expect(Object.keys(result.workspace.files).some((f) => f.includes("node_modules"))).toBe(false);
    expect(Object.keys(result.workspace.files).some((f) => f.includes(".git"))).toBe(false);
  });

  it("does not include memory by default", async () => {
    const store = createMockStore([], []);
    const result = await exportAgent("test-agent", {}, store);

    expect(result.memory).toBeUndefined();
  });

  it("produces valid JSON output", async () => {
    const store = createMockStore([makeSession()], [makeMessage()]);
    const result = await exportAgent("test-agent", { name: "Test" }, store);

    const json = JSON.stringify(result);
    const parsed = JSON.parse(json) as AgentFile;

    expect(parsed.version).toBe(1);
    expect(parsed.nous.id).toBe("test-agent");
    expect(parsed.sessions).toHaveLength(1);
  });
});
