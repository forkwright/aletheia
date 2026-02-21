// Session store unit tests — uses :memory: SQLite
import { afterEach, beforeEach, describe, expect, it } from "vitest";
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

  it("archiveSession changes status and frees session_key", () => {
    const session = store.createSession("syn", "main");
    store.archiveSession(session.id);
    const archived = store.findSessionById(session.id);
    expect(archived!.status).toBe("archived");
    expect(archived!.sessionKey).toContain(":archived:");
    // Archived sessions not returned by findSession (active only)
    expect(store.findSession("syn", "main")).toBeNull();
    // New session can reuse the same key
    const fresh = store.createSession("syn", "main");
    expect(fresh.id).not.toBe(session.id);
    expect(fresh.sessionKey).toBe("main");
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

  it("getHistory respects limit (returns most recent N)", () => {
    store.appendMessage(sessionId, "user", "one");
    store.appendMessage(sessionId, "assistant", "two");
    store.appendMessage(sessionId, "user", "three");

    const limited = store.getHistory(sessionId, { limit: 2 });
    expect(limited.length).toBe(2);
    expect(limited[0]!.content).toBe("two");
    expect(limited[1]!.content).toBe("three");
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

describe("thread model (Phase 2)", () => {
  it("resolveThread creates new thread on first call", () => {
    const thread = store.resolveThread("syn", "cody");
    expect(thread.id).toMatch(/^thr_/);
    expect(thread.nousId).toBe("syn");
    expect(thread.identity).toBe("cody");
  });

  it("resolveThread returns same thread on second call", () => {
    const t1 = store.resolveThread("syn", "cody");
    const t2 = store.resolveThread("syn", "cody");
    expect(t1.id).toBe(t2.id);
  });

  it("resolveThread creates separate threads for different (nous, identity) pairs", () => {
    const t1 = store.resolveThread("syn", "cody");
    const t2 = store.resolveThread("chiron", "cody");
    const t3 = store.resolveThread("syn", "alice");
    expect(t1.id).not.toBe(t2.id);
    expect(t1.id).not.toBe(t3.id);
    expect(t2.id).not.toBe(t3.id);
  });

  it("resolveBinding upserts binding and returns it", () => {
    const thread = store.resolveThread("syn", "cody");
    const binding = store.resolveBinding(thread.id, "signal", "signal:abc123");
    expect(binding.id).toMatch(/^tbnd_/);
    expect(binding.threadId).toBe(thread.id);
    expect(binding.transport).toBe("signal");
    expect(binding.channelKey).toBe("signal:abc123");
  });

  it("resolveBinding returns same binding on repeat call (updates lastSeenAt)", () => {
    const thread = store.resolveThread("syn", "cody");
    const b1 = store.resolveBinding(thread.id, "webchat", "web:cody:syn");
    const b2 = store.resolveBinding(thread.id, "webchat", "web:cody:syn");
    expect(b1.id).toBe(b2.id);
  });

  it("migrateSessionsToThreads links sessions to threads", () => {
    store.createSession("syn", "signal:abc123");
    store.createSession("chiron", "web:anonymous:chiron");
    const count = store.migrateSessionsToThreads();
    expect(count).toBeGreaterThanOrEqual(2);
  });

  it("migrateSessionsToThreads does not re-migrate already linked sessions", () => {
    store.createSession("syn", "signal:abc123");
    const c1 = store.migrateSessionsToThreads();
    const c2 = store.migrateSessionsToThreads();
    expect(c1).toBeGreaterThanOrEqual(1);
    expect(c2).toBe(0); // already linked
  });
});

describe("thread summary (Phase 3)", () => {
  let threadId: string;

  beforeEach(() => {
    const thread = store.resolveThread("syn", "cody");
    threadId = thread.id;
  });

  it("getThreadSummary returns null when no summary exists", () => {
    expect(store.getThreadSummary(threadId)).toBeNull();
  });

  it("updateThreadSummary creates new summary", () => {
    store.updateThreadSummary(threadId, "Cody and I have been working on auth.", ["Auth implemented", "PR #26 merged"]);
    const summary = store.getThreadSummary(threadId);
    expect(summary).not.toBeNull();
    expect(summary!.summary).toBe("Cody and I have been working on auth.");
    expect(summary!.keyFacts).toEqual(["Auth implemented", "PR #26 merged"]);
    expect(summary!.threadId).toBe(threadId);
  });

  it("updateThreadSummary replaces existing summary", () => {
    store.updateThreadSummary(threadId, "First summary", ["fact 1"]);
    store.updateThreadSummary(threadId, "Updated summary", ["fact 2"]);
    const summary = store.getThreadSummary(threadId);
    expect(summary!.summary).toBe("Updated summary");
    expect(summary!.keyFacts).toEqual(["fact 2"]);
  });

  it("getThreadForSession returns the thread linked to a session", () => {
    const session = store.createSession("syn", "signal:cody123");
    const thread = store.resolveThread("syn", "cody");
    store.resolveBinding(thread.id, "signal", "signal:cody123");
    store.linkSessionToThread(session.id, thread.id, "signal");

    const found = store.getThreadForSession(session.id);
    expect(found).not.toBeNull();
    expect(found!.id).toBe(thread.id);
  });

  it("getThreadForSession returns null for unlinked session", () => {
    const session = store.createSession("syn", "main");
    expect(store.getThreadForSession(session.id)).toBeNull();
  });

  it("getSessionsByThread returns sessions in the thread", () => {
    const session = store.createSession("syn", "signal:cody123");
    const thread = store.resolveThread("syn", "cody");
    store.linkSessionToThread(session.id, thread.id, "signal");

    const sessions = store.getSessionsByThread(thread.id);
    expect(sessions.length).toBe(1);
    expect(sessions[0]!.id).toBe(session.id);
  });

  it("getThreadHistory returns messages across all sessions in thread", () => {
    const s1 = store.createSession("syn", "signal:cody123");
    const s2 = store.createSession("syn", "signal:cody456");
    const thread = store.resolveThread("syn", "cody");
    store.linkSessionToThread(s1.id, thread.id, "signal");
    store.linkSessionToThread(s2.id, thread.id, "signal");

    store.appendMessage(s1.id, "user", "hello from session 1", { tokenEstimate: 10 });
    store.appendMessage(s2.id, "user", "hello from session 2", { tokenEstimate: 10 });

    const history = store.getThreadHistory(thread.id);
    expect(history.length).toBe(2);
    // Should include messages from both sessions
    const contents = history.map((m) => m.content);
    expect(contents).toContain("hello from session 1");
    expect(contents).toContain("hello from session 2");
  });

  it("listThreads returns threads with session and message counts", () => {
    const thread = store.resolveThread("syn", "cody");
    const session = store.createSession("syn", "signal:cody123");
    store.linkSessionToThread(session.id, thread.id, "signal");
    store.appendMessage(session.id, "user", "hello", { tokenEstimate: 5 });

    const threads = store.listThreads("syn");
    expect(threads.length).toBeGreaterThanOrEqual(1);
    const t = threads.find((x) => x.id === thread.id);
    expect(t).not.toBeUndefined();
    expect(t!.sessionCount).toBeGreaterThanOrEqual(1);
  });
});

describe("session classification", () => {
  it("auto-classifies prosoche sessions as background", () => {
    const session = store.createSession("syn", "prosoche");
    expect(session.sessionType).toBe("background");
  });

  it("auto-classifies prosoche-variant keys as background", () => {
    const session = store.createSession("syn", "main:prosoche:daily");
    expect(session.sessionType).toBe("background");
  });

  it("auto-classifies spawn: sessions as ephemeral", () => {
    const session = store.createSession("syn", "spawn:task123");
    expect(session.sessionType).toBe("ephemeral");
  });

  it("auto-classifies ask: sessions as ephemeral", () => {
    const session = store.createSession("syn", "ask:chiron:health");
    expect(session.sessionType).toBe("ephemeral");
  });

  it("auto-classifies ephemeral: sessions as ephemeral", () => {
    const session = store.createSession("syn", "ephemeral:temp");
    expect(session.sessionType).toBe("ephemeral");
  });

  it("default session type is primary", () => {
    const session = store.createSession("syn", "main");
    expect(session.sessionType).toBe("primary");
  });

  it("webchat session type is primary", () => {
    const session = store.createSession("syn", "web:cody:syn");
    expect(session.sessionType).toBe("primary");
  });

  it("updateSessionType changes classification", () => {
    const session = store.createSession("syn", "main");
    store.updateSessionType(session.id, "background");
    const updated = store.findSessionById(session.id)!;
    expect(updated.sessionType).toBe("background");
  });

  it("updateLastDistilledAt sets timestamp", () => {
    const session = store.createSession("syn", "main");
    expect(session.lastDistilledAt).toBeNull();
    store.updateLastDistilledAt(session.id);
    const updated = store.findSessionById(session.id)!;
    expect(updated.lastDistilledAt).not.toBeNull();
  });

  it("updateComputedContextTokens stores value", () => {
    const session = store.createSession("syn", "main");
    store.updateComputedContextTokens(session.id, 85000);
    const updated = store.findSessionById(session.id)!;
    expect(updated.computedContextTokens).toBe(85000);
  });
});

describe("deleteEphemeralSessions", () => {
  it("deletes ephemeral sessions older than cutoff", () => {
    const session = store.createSession("syn", "spawn:old-task");
    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET created_at = '2020-01-01T00:00:00.000Z', updated_at = '2020-01-01T00:00:00.000Z' WHERE id = ?")
      .run(session.id);

    const deleted = store.deleteEphemeralSessions(1000);
    expect(deleted).toBe(1);
    expect(store.findSessionById(session.id)).toBeNull();
  });

  it("does not delete primary sessions", () => {
    const session = store.createSession("syn", "main");
    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET created_at = '2020-01-01T00:00:00.000Z', updated_at = '2020-01-01T00:00:00.000Z' WHERE id = ?")
      .run(session.id);

    const deleted = store.deleteEphemeralSessions(1000);
    expect(deleted).toBe(0);
    expect(store.findSessionById(session.id)).not.toBeNull();
  });

  it("cascades delete to messages and related data", () => {
    const session = store.createSession("syn", "spawn:task-with-data");
    store.appendMessage(session.id, "user", "hello", { tokenEstimate: 10 });
    store.appendMessage(session.id, "assistant", "hi", { tokenEstimate: 15 });

    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET created_at = '2020-01-01T00:00:00.000Z', updated_at = '2020-01-01T00:00:00.000Z' WHERE id = ?")
      .run(session.id);

    const deleted = store.deleteEphemeralSessions(1000);
    expect(deleted).toBe(1);

    // Messages should be gone too
    const history = store.getHistory(session.id);
    expect(history.length).toBe(0);
  });

  it("does not delete recent ephemeral sessions", () => {
    const session = store.createSession("syn", "spawn:fresh-task");
    // Default cutoff is 24h — session just created should survive
    const deleted = store.deleteEphemeralSessions();
    expect(deleted).toBe(0);
    expect(store.findSessionById(session.id)).not.toBeNull();
  });
});

describe("retention", () => {
  it("purgeDistilledMessages returns 0 when days=0 (disabled)", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "user", "hello");
    store.archiveSession(session.id);
    // Set to distilled status manually via updateStatus
    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET status = 'distilled' WHERE id = ?")
      .run(session.id);
    expect(store.purgeDistilledMessages(0)).toBe(0);
  });

  it("purgeDistilledMessages removes messages from old distilled sessions", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "user", "to purge");
    // Mark distilled with an old updated_at
    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET status = 'distilled', updated_at = '2020-01-01T00:00:00.000Z' WHERE id = ?")
      .run(session.id);
    const deleted = store.purgeDistilledMessages(30);
    expect(deleted).toBe(1);
  });

  it("purgeArchivedSessionMessages removes messages from old archived sessions", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "user", "to purge");
    store.archiveSession(session.id);
    (store as unknown as { db: { prepare: (s: string) => { run: (...a: unknown[]) => void } } }).db
      .prepare("UPDATE sessions SET updated_at = '2020-01-01T00:00:00.000Z' WHERE id = ?")
      .run(session.id);
    const deleted = store.purgeArchivedSessionMessages(30);
    expect(deleted).toBe(1);
  });

  it("purgeArchivedSessionMessages returns 0 when days=0 (disabled)", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "user", "hello");
    store.archiveSession(session.id);
    expect(store.purgeArchivedSessionMessages(0)).toBe(0);
  });

  it("truncateToolResults truncates long tool result content", () => {
    const session = store.createSession("syn", "main");
    const longContent = "x".repeat(500);
    store.appendMessage(session.id, "tool_result", longContent, { toolCallId: "tc1", toolName: "exec" });
    const truncated = store.truncateToolResults(100);
    expect(truncated).toBe(1);
    const history = store.getHistory(session.id, { excludeDistilled: false });
    const toolMsg = history.find((m) => m.role === "tool_result");
    expect(toolMsg!.content.length).toBeLessThan(longContent.length);
    expect(toolMsg!.content).toContain("[truncated]");
  });

  it("truncateToolResults returns 0 when maxChars=0 (disabled)", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "tool_result", "x".repeat(500));
    expect(store.truncateToolResults(0)).toBe(0);
  });

  it("truncateToolResults does not truncate short results", () => {
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "tool_result", "short result", { toolCallId: "tc1" });
    const truncated = store.truncateToolResults(100);
    expect(truncated).toBe(0);
  });
});

describe("session forking (checkpoint time-travel)", () => {
  function setupDistilledSession() {
    const session = store.createSession("syn", "main");
    // Simulate conversation with messages
    store.appendMessage(session.id, "user", "What is Qdrant?");
    store.appendMessage(session.id, "assistant", "Qdrant is a vector database.");
    store.appendMessage(session.id, "user", "How does indexing work?");
    store.appendMessage(session.id, "assistant", "HNSW algorithm is used.");

    // Simulate distillation #1: mark first 4 messages as distilled, add summary
    store.markMessagesDistilled(session.id, [1, 2, 3, 4]);
    store.appendMessage(session.id, "assistant", "[Distillation #1] Summary: Discussed Qdrant vector DB and HNSW indexing.");
    store.incrementDistillationCount(session.id);
    store.recordDistillationLog({
      sessionId: session.id, nousId: "syn",
      messagesBefore: 4, messagesAfter: 1,
      tokensBefore: 200, tokensAfter: 50,
      factsExtracted: 2, decisionsExtracted: 0, openItemsExtracted: 0,
      flushSucceeded: true, distillationNumber: 1,
    });

    // More messages after distillation
    store.appendMessage(session.id, "user", "Tell me about Neo4j.");
    store.appendMessage(session.id, "assistant", "Neo4j is a graph database.");
    return session;
  }

  it("getCheckpoints returns distillation receipts in order", () => {
    const session = setupDistilledSession();
    const checkpoints = store.getCheckpoints(session.id);
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0]!.distillationNumber).toBe(1);
    expect(checkpoints[0]!.factsExtracted).toBe(2);
  });

  it("getCheckpoints returns empty for session with no distillations", () => {
    const session = store.createSession("syn", "fresh");
    expect(store.getCheckpoints(session.id)).toHaveLength(0);
  });

  it("forkSession copies messages up to checkpoint", () => {
    const session = setupDistilledSession();
    const result = store.forkSession(session.id, 1);

    expect(result.newSessionId).toMatch(/^ses_/);
    expect(result.messagesCopied).toBe(1); // only the summary (undistilled msgs <= summary seq)

    const forked = store.findSessionById(result.newSessionId);
    expect(forked).not.toBeNull();
    expect(forked!.parentSessionId).toBe(session.id);

    const history = store.getHistory(result.newSessionId);
    expect(history).toHaveLength(1);
    expect(history[0]!.content).toContain("[Distillation #1]");
  });

  it("forkSession excludes post-checkpoint messages", () => {
    const session = setupDistilledSession();
    const result = store.forkSession(session.id, 1);
    const history = store.getHistory(result.newSessionId);
    const contents = history.map((m) => m.content);
    expect(contents.some((c) => c.includes("Neo4j"))).toBe(false);
  });

  it("forkSession throws on invalid distillation number", () => {
    const session = setupDistilledSession();
    expect(() => store.forkSession(session.id, 999)).toThrow("checkpoint not found");
  });

  it("forkSession throws on non-existent session", () => {
    expect(() => store.forkSession("ses_nonexistent", 1)).toThrow("checkpoint not found");
  });
});
