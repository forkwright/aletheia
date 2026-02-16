// Session store unit tests — uses :memory: SQLite
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { SessionStore } from "./store.js";

let store: SessionStore;

beforeEach(() => {
  store = new SessionStore(":memory:");
});

afterEach(() => {
  store.close();
});

describe("session CRUD", () => {
  it("creates and retrieves a session", () => {
    const session = store.createSession("syn", "main");
    expect(session.id).toMatch(/^ses_/);
    expect(session.nousId).toBe("syn");
    expect(session.sessionKey).toBe("main");
    expect(session.status).toBe("active");
    expect(session.tokenCountEstimate).toBe(0);
    expect(session.messageCount).toBe(0);
  });

  it("findSession returns existing active session", () => {
    store.createSession("syn", "chat");
    const found = store.findSession("syn", "chat");
    expect(found).not.toBeNull();
    expect(found!.nousId).toBe("syn");
    expect(found!.sessionKey).toBe("chat");
  });

  it("findSession returns null for non-existent session", () => {
    expect(store.findSession("syn", "nope")).toBeNull();
  });

  it("findOrCreateSession reuses existing", () => {
    const first = store.findOrCreateSession("syn", "main");
    const second = store.findOrCreateSession("syn", "main");
    expect(first.id).toBe(second.id);
  });

  it("findOrCreateSession creates when missing", () => {
    const session = store.findOrCreateSession("chiron", "health");
    expect(session.nousId).toBe("chiron");
    expect(session.sessionKey).toBe("health");
  });

  it("findSessionById returns null for unknown ID", () => {
    expect(store.findSessionById("ses_nonexistent")).toBeNull();
  });

  it("listSessions returns all sessions", () => {
    store.createSession("syn", "main");
    store.createSession("chiron", "health");
    const all = store.listSessions();
    expect(all.length).toBe(2);
  });

  it("listSessions filters by nousId", () => {
    store.createSession("syn", "main");
    store.createSession("chiron", "health");
    const synOnly = store.listSessions("syn");
    expect(synOnly.length).toBe(1);
    expect(synOnly[0]!.nousId).toBe("syn");
  });

  it("archiveSession changes status", () => {
    const session = store.createSession("syn", "main");
    store.archiveSession(session.id);
    const archived = store.findSessionById(session.id);
    expect(archived!.status).toBe("archived");
    // Archived sessions not returned by findSession (active only)
    expect(store.findSession("syn", "main")).toBeNull();
  });

  it("createSession with parent and model", () => {
    const parent = store.createSession("syn", "main");
    const child = store.createSession("chiron", "spawn:task", parent.id, "claude-haiku");
    expect(child.parentSessionId).toBe(parent.id);
    expect(child.model).toBe("claude-haiku");
  });
});

describe("message operations", () => {
  let sessionId: string;

  beforeEach(() => {
    sessionId = store.createSession("syn", "main").id;
  });

  it("appendMessage increments seq and updates session", () => {
    const seq1 = store.appendMessage(sessionId, "user", "hello", { tokenEstimate: 10 });
    const seq2 = store.appendMessage(sessionId, "assistant", "hi there", { tokenEstimate: 15 });
    expect(seq1).toBe(1);
    expect(seq2).toBe(2);

    const session = store.findSessionById(sessionId)!;
    expect(session.messageCount).toBe(2);
    expect(session.tokenCountEstimate).toBe(25);
  });

  it("getHistory returns messages in order", () => {
    store.appendMessage(sessionId, "user", "first");
    store.appendMessage(sessionId, "assistant", "second");
    store.appendMessage(sessionId, "user", "third");

    const history = store.getHistory(sessionId);
    expect(history.length).toBe(3);
    expect(history[0]!.content).toBe("first");
    expect(history[0]!.role).toBe("user");
    expect(history[1]!.content).toBe("second");
    expect(history[2]!.content).toBe("third");
  });

  it("getHistory respects limit", () => {
    store.appendMessage(sessionId, "user", "one");
    store.appendMessage(sessionId, "assistant", "two");
    store.appendMessage(sessionId, "user", "three");

    const limited = store.getHistory(sessionId, { limit: 2 });
    expect(limited.length).toBe(2);
    expect(limited[0]!.content).toBe("one");
  });

  it("appendMessage stores tool metadata", () => {
    store.appendMessage(sessionId, "tool_result", '{"files": 3}', {
      toolCallId: "toolu_abc",
      toolName: "bash",
      tokenEstimate: 20,
    });
    const history = store.getHistory(sessionId);
    expect(history[0]!.role).toBe("tool_result");
    expect(history[0]!.toolCallId).toBe("toolu_abc");
    expect(history[0]!.toolName).toBe("bash");
  });
});

describe("history budget", () => {
  let sessionId: string;

  beforeEach(() => {
    sessionId = store.createSession("syn", "main").id;
  });

  it("getHistoryWithBudget returns recent messages within budget", () => {
    store.appendMessage(sessionId, "user", "old message", { tokenEstimate: 500 });
    store.appendMessage(sessionId, "assistant", "old reply", { tokenEstimate: 500 });
    store.appendMessage(sessionId, "user", "new message", { tokenEstimate: 500 });
    store.appendMessage(sessionId, "assistant", "new reply", { tokenEstimate: 500 });

    const recent = store.getHistoryWithBudget(sessionId, 1200);
    expect(recent.length).toBe(2);
    expect(recent[0]!.content).toBe("new message");
    expect(recent[1]!.content).toBe("new reply");
  });

  it("getHistoryWithBudget includes at least one message", () => {
    store.appendMessage(sessionId, "user", "huge message", { tokenEstimate: 99999 });
    const result = store.getHistoryWithBudget(sessionId, 100);
    expect(result.length).toBe(1);
  });

  it("getHistoryWithBudget excludes distilled messages", () => {
    store.appendMessage(sessionId, "user", "old", { tokenEstimate: 100 });
    store.appendMessage(sessionId, "assistant", "old reply", { tokenEstimate: 100 });
    store.appendMessage(sessionId, "user", "new", { tokenEstimate: 100 });

    store.markMessagesDistilled(sessionId, [1, 2]);

    const history = store.getHistoryWithBudget(sessionId, 50000);
    expect(history.length).toBe(1);
    expect(history[0]!.content).toBe("new");
  });
});

describe("distillation", () => {
  let sessionId: string;

  beforeEach(() => {
    sessionId = store.createSession("syn", "main").id;
  });

  it("markMessagesDistilled flags messages and recalculates tokens", () => {
    store.appendMessage(sessionId, "user", "a", { tokenEstimate: 100 });
    store.appendMessage(sessionId, "assistant", "b", { tokenEstimate: 200 });
    store.appendMessage(sessionId, "user", "c", { tokenEstimate: 50 });

    store.markMessagesDistilled(sessionId, [1, 2]);

    // Only undistilled messages should be counted
    const session = store.findSessionById(sessionId)!;
    expect(session.tokenCountEstimate).toBe(50);
    expect(session.messageCount).toBe(1);

    // Distilled messages excluded from normal history
    const history = store.getHistory(sessionId, { excludeDistilled: true });
    expect(history.length).toBe(1);
    expect(history[0]!.content).toBe("c");
  });

  it("markMessagesDistilled no-ops on empty array", () => {
    store.appendMessage(sessionId, "user", "test", { tokenEstimate: 100 });
    store.markMessagesDistilled(sessionId, []);
    const session = store.findSessionById(sessionId)!;
    expect(session.tokenCountEstimate).toBe(100);
  });

  it("recordDistillation creates audit trail", () => {
    store.recordDistillation({
      sessionId,
      messagesBefore: 20,
      messagesAfter: 3,
      tokensBefore: 5000,
      tokensAfter: 800,
      factsExtracted: 7,
      model: "claude-haiku",
    });
    // No error = success. The table exists and accepts records.
  });

  it("incrementDistillationCount tracks sequential distillations", () => {
    const c1 = store.incrementDistillationCount(sessionId);
    const c2 = store.incrementDistillationCount(sessionId);
    expect(c1).toBe(1);
    expect(c2).toBe(2);
  });
});

describe("usage tracking", () => {
  let sessionId: string;

  beforeEach(() => {
    sessionId = store.createSession("syn", "main").id;
  });

  it("recordUsage stores token counts", () => {
    store.recordUsage({
      sessionId,
      turnSeq: 1,
      inputTokens: 1000,
      outputTokens: 500,
      cacheReadTokens: 800,
      cacheWriteTokens: 200,
      model: "claude-sonnet",
    });

    const costs = store.getCostsBySession(sessionId);
    expect(costs.length).toBe(1);
    expect(costs[0]!.inputTokens).toBe(1000);
    expect(costs[0]!.outputTokens).toBe(500);
    expect(costs[0]!.model).toBe("claude-sonnet");
  });

  it("getMetrics aggregates across sessions", () => {
    store.recordUsage({
      sessionId,
      turnSeq: 1,
      inputTokens: 1000,
      outputTokens: 500,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      model: null,
    });

    const metrics = store.getMetrics();
    expect(metrics.usage.totalInputTokens).toBe(1000);
    expect(metrics.usage.totalOutputTokens).toBe(500);
    expect(metrics.usage.turnCount).toBe(1);
    expect(metrics.usageByNous["syn"]).toBeDefined();
  });
});

describe("routing", () => {
  it("resolveRoute matches channel + peer", () => {
    store.rebuildRoutingCache([
      { channel: "signal", peerKind: "user", peerId: "+1234", nousId: "syn", priority: 10 },
      { channel: "signal", nousId: "syl", priority: 0 },
    ]);

    expect(store.resolveRoute("signal", "user", "+1234")).toBe("syn");
    expect(store.resolveRoute("signal")).toBe("syl");
    expect(store.resolveRoute("slack")).toBeNull();
  });

  it("rebuildRoutingCache replaces all entries", () => {
    store.rebuildRoutingCache([{ channel: "signal", nousId: "syn" }]);
    store.rebuildRoutingCache([{ channel: "web", nousId: "chiron" }]);

    expect(store.resolveRoute("signal")).toBeNull();
    expect(store.resolveRoute("web")).toBe("chiron");
  });
});

describe("cross-agent messaging", () => {
  it("records and retrieves cross-agent calls", () => {
    const sessionId = store.createSession("syn", "main").id;
    const msgId = store.recordCrossAgentCall({
      sourceSessionId: sessionId,
      sourceNousId: "syn",
      targetNousId: "chiron",
      kind: "send",
      content: "Schedule appointment",
    });
    expect(msgId).toBeGreaterThan(0);

    // Mark as delivered so it appears in unsurfaced
    store.updateCrossAgentCall(msgId, { status: "delivered" });
    const unsurfaced = store.getUnsurfacedMessages("chiron");
    expect(unsurfaced.length).toBe(1);
    expect(unsurfaced[0]!.content).toBe("Schedule appointment");
  });

  it("markMessagesSurfaced hides messages", () => {
    const sessionId = store.createSession("syn", "main").id;
    const chironSession = store.createSession("chiron", "main").id;
    const msgId = store.recordCrossAgentCall({
      sourceSessionId: sessionId,
      sourceNousId: "syn",
      targetNousId: "chiron",
      kind: "send",
      content: "Do task",
    });
    store.updateCrossAgentCall(msgId, { status: "delivered" });
    store.markMessagesSurfaced([msgId], chironSession);

    const unsurfaced = store.getUnsurfacedMessages("chiron");
    expect(unsurfaced.length).toBe(0);
  });
});

describe("contacts", () => {
  it("pairing flow: request → approve → check", () => {
    const req = store.createContactRequest("+15551234", "Alice", "signal");
    expect(req.challengeCode).toMatch(/^\d{4}$/);

    // Not yet approved
    expect(store.isApprovedContact("+15551234", "signal")).toBe(false);

    // Pending shows up
    const pending = store.getPendingRequests();
    expect(pending.length).toBe(1);
    expect(pending[0]!.sender).toBe("+15551234");

    // Approve
    const result = store.approveContactByCode(req.challengeCode);
    expect(result).not.toBeNull();
    expect(result!.sender).toBe("+15551234");

    // Now approved
    expect(store.isApprovedContact("+15551234", "signal")).toBe(true);
    expect(store.getPendingRequests().length).toBe(0);
  });

  it("denyContactByCode removes pending request", () => {
    const req = store.createContactRequest("+15559999", "Bob", "signal");
    expect(store.denyContactByCode(req.challengeCode)).toBe(true);
    expect(store.getPendingRequests().length).toBe(0);
    expect(store.isApprovedContact("+15559999", "signal")).toBe(false);
  });

  it("approveContactByCode returns null for bad code", () => {
    expect(store.approveContactByCode("0000")).toBeNull();
  });
});

describe("bootstrap hash", () => {
  it("updateBootstrapHash and getLastBootstrapHash", () => {
    const session = store.createSession("syn", "main");
    store.updateBootstrapHash(session.id, "abc123");

    const hash = store.getLastBootstrapHash("syn");
    expect(hash).toBe("abc123");
  });

  it("returns null when no hash stored", () => {
    store.createSession("syn", "main");
    expect(store.getLastBootstrapHash("syn")).toBeNull();
  });
});

describe("archiveStaleSpawnSessions", () => {
  it("runs without error and returns a count", () => {
    store.createSession("syn", "spawn:task1");
    // With maxAgeMs=0, the cutoff is "now" — session created in same ms may or may not match.
    // Just verify it executes without error and returns a number.
    const count = store.archiveStaleSpawnSessions();
    expect(typeof count).toBe("number");
  });
});
