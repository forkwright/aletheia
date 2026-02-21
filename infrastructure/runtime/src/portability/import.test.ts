// Agent import tests
import { describe, expect, it, vi, beforeEach, afterAll } from "vitest";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";

const TEST_ROOT = "/tmp/aletheia-import-test";

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
  const root = "/tmp/aletheia-import-test";
  return {
    paths: {
      root,
      nous: pj(root, "nous"),
      shared: pj(root, "shared"),
      nousDir: (id: string) => pj(root, "nous", id),
    },
  };
});

import { importAgent, type ImportResult } from "./import.js";
import type { AgentFile } from "./export.js";

function makeAgentFile(overrides: Partial<AgentFile> = {}): AgentFile {
  return {
    version: 1,
    exportedAt: "2026-02-21T12:00:00Z",
    generator: "aletheia-export/1.0",
    nous: {
      id: "test-agent",
      name: "Test Agent",
      model: "claude-opus-4-6",
      config: {},
    },
    workspace: {
      files: {
        "SOUL.md": "# Test Agent",
        "MEMORY.md": "# Memory\n\nSome facts.",
        "memory/notes.md": "# Notes",
      },
      binaryFiles: [],
    },
    sessions: [
      {
        id: "ses_old_001",
        sessionKey: "main",
        status: "active",
        sessionType: "primary",
        messageCount: 2,
        tokenCountEstimate: 100,
        distillationCount: 0,
        createdAt: "2026-02-20T08:00:00Z",
        updatedAt: "2026-02-21T10:00:00Z",
        workingState: null,
        distillationPriming: null,
        notes: [{ category: "task", content: "Build import feature", createdAt: "2026-02-21T10:00:00Z" }],
        messages: [
          { role: "user", content: "Hello", seq: 1, tokenEstimate: 10, isDistilled: false, createdAt: "2026-02-21T10:00:00Z" },
          { role: "assistant", content: "Hi there!", seq: 2, tokenEstimate: 15, isDistilled: false, createdAt: "2026-02-21T10:01:00Z" },
        ],
      },
    ],
    ...overrides,
  };
}

// Minimal mock store
function createMockStore() {
  const sessions: Array<{ id: string; nousId: string; sessionKey: string }> = [];
  const messages: Array<{ sessionId: string; role: string; content: string }> = [];
  const notes: Array<{ sessionId: string; nousId: string; category: string; content: string }> = [];
  let sessionCounter = 0;

  return {
    _sessions: sessions,
    _messages: messages,
    _notes: notes,
    createSession: vi.fn((nousId: string, sessionKey?: string) => {
      sessionCounter++;
      const session = { id: `ses_new_${sessionCounter}`, nousId, sessionKey: sessionKey ?? "default", status: "active" as const, sessionType: "primary" as const, messageCount: 0, tokenCountEstimate: 0, distillationCount: 0, lastInputTokens: 0, bootstrapHash: null, model: null, parentSessionId: null, lastDistilledAt: null, computedContextTokens: 0, workingState: null, distillationPriming: null, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
      sessions.push({ id: session.id, nousId, sessionKey: session.sessionKey });
      return session;
    }),
    appendMessage: vi.fn((sessionId: string, role: string, content: string) => {
      messages.push({ sessionId, role, content });
      return messages.length;
    }),
    addNote: vi.fn((sessionId: string, nousId: string, category: string, content: string) => {
      notes.push({ sessionId, nousId, category, content });
      return notes.length;
    }),
    updateWorkingState: vi.fn(),
    setDistillationPriming: vi.fn(),
  } as unknown as import("../mneme/store.js").SessionStore & {
    _sessions: typeof sessions;
    _messages: typeof messages;
    _notes: typeof notes;
  };
}

describe("importAgent", () => {
  beforeEach(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
    mkdirSync(join(TEST_ROOT, "nous"), { recursive: true });
  });

  afterAll(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
  });

  it("restores workspace files", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile({ sessions: [] });

    const result = await importAgent(agentFile, store);

    expect(result.filesRestored).toBe(3);
    expect(existsSync(join(TEST_ROOT, "nous", "test-agent", "SOUL.md"))).toBe(true);
    expect(readFileSync(join(TEST_ROOT, "nous", "test-agent", "SOUL.md"), "utf-8")).toBe("# Test Agent");
    expect(existsSync(join(TEST_ROOT, "nous", "test-agent", "memory", "notes.md"))).toBe(true);
  });

  it("imports sessions with messages and notes", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile();

    const result = await importAgent(agentFile, store);

    expect(result.sessionsImported).toBe(1);
    expect(result.messagesImported).toBe(2);
    expect(result.notesImported).toBe(1);
    expect(store.createSession).toHaveBeenCalledWith("test-agent", "main");
    expect(store._messages).toHaveLength(2);
    expect(store._messages[0]!.role).toBe("user");
    expect(store._messages[1]!.role).toBe("assistant");
    expect(store._notes[0]!.category).toBe("task");
  });

  it("supports targetNousId override", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile({ sessions: [] });

    const result = await importAgent(agentFile, store, { targetNousId: "cloned-agent" });

    expect(result.nousId).toBe("cloned-agent");
    expect(existsSync(join(TEST_ROOT, "nous", "cloned-agent", "SOUL.md"))).toBe(true);
  });

  it("skips sessions when requested", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile();

    const result = await importAgent(agentFile, store, { skipSessions: true });

    expect(result.sessionsImported).toBe(0);
    expect(result.messagesImported).toBe(0);
    expect(result.filesRestored).toBe(3);
  });

  it("skips workspace when requested", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile();

    const result = await importAgent(agentFile, store, { skipWorkspace: true });

    expect(result.filesRestored).toBe(0);
    expect(result.sessionsImported).toBe(1);
  });

  it("generates new session IDs (no collision)", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile();

    await importAgent(agentFile, store);

    // The new session ID should not match the exported one
    expect(store._sessions[0]!.id).not.toBe("ses_old_001");
  });

  it("rejects unsupported version", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile({ version: 99 as 1 });

    await expect(importAgent(agentFile, store)).rejects.toThrow("Unsupported agent file version");
  });

  it("handles empty agent file", async () => {
    const store = createMockStore();
    const agentFile = makeAgentFile({
      workspace: { files: {}, binaryFiles: [] },
      sessions: [],
    });

    const result = await importAgent(agentFile, store);

    expect(result.filesRestored).toBe(0);
    expect(result.sessionsImported).toBe(0);
  });
});
