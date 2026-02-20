import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { SessionStore } from "./store.js";

describe("message queue", () => {
  let tmpDir: string;
  let store: SessionStore;

  beforeEach(() => {
    tmpDir = mkdtempSync(join(tmpdir(), "queue-test-"));
    store = new SessionStore(join(tmpDir, "test.db"));
  });

  afterEach(() => {
    store.close();
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("queues and drains messages", () => {
    const session = store.findOrCreateSession("test-nous", "main");

    store.queueMessage(session.id, "Stop what you're doing", "user");
    store.queueMessage(session.id, "Try a different approach", "user");

    const msgs = store.drainQueue(session.id);
    expect(msgs).toHaveLength(2);
    expect(msgs[0]!.content).toBe("Stop what you're doing");
    expect(msgs[1]!.content).toBe("Try a different approach");

    // Queue should be empty after drain
    const again = store.drainQueue(session.id);
    expect(again).toHaveLength(0);
  });

  it("returns empty array when no messages", () => {
    const session = store.findOrCreateSession("test-nous", "main");
    expect(store.drainQueue(session.id)).toHaveLength(0);
  });

  it("tracks queue length", () => {
    const session = store.findOrCreateSession("test-nous", "main");
    expect(store.getQueueLength(session.id)).toBe(0);

    store.queueMessage(session.id, "msg1");
    expect(store.getQueueLength(session.id)).toBe(1);

    store.queueMessage(session.id, "msg2");
    expect(store.getQueueLength(session.id)).toBe(2);

    store.drainQueue(session.id);
    expect(store.getQueueLength(session.id)).toBe(0);
  });

  it("preserves message order", () => {
    const session = store.findOrCreateSession("test-nous", "main");

    for (let i = 0; i < 5; i++) {
      store.queueMessage(session.id, `msg-${i}`);
    }

    const msgs = store.drainQueue(session.id);
    expect(msgs.map((m) => m.content)).toEqual([
      "msg-0", "msg-1", "msg-2", "msg-3", "msg-4",
    ]);
  });

  it("isolates queues by session", () => {
    const s1 = store.findOrCreateSession("nous1", "main");
    const s2 = store.findOrCreateSession("nous2", "main");

    store.queueMessage(s1.id, "for session 1");
    store.queueMessage(s2.id, "for session 2");

    const msgs1 = store.drainQueue(s1.id);
    expect(msgs1).toHaveLength(1);
    expect(msgs1[0]!.content).toBe("for session 1");

    // s2 queue should be unaffected
    expect(store.getQueueLength(s2.id)).toBe(1);
  });

  it("stores sender when provided", () => {
    const session = store.findOrCreateSession("test-nous", "main");

    store.queueMessage(session.id, "with sender", "cody");
    store.queueMessage(session.id, "without sender");

    const msgs = store.drainQueue(session.id);
    expect(msgs[0]!.sender).toBe("cody");
    expect(msgs[1]!.sender).toBeNull();
  });
});
