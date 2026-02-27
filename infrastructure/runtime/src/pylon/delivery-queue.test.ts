import { describe, it, expect, afterEach } from "vitest";
import { DeliveryQueue } from "./delivery-queue.js";

function makeEntry(overrides: Partial<{ sessionId: string; nousId: string; turnId: string }> = {}) {
  return {
    sessionId: overrides.sessionId ?? "sess-1",
    nousId: overrides.nousId ?? "main",
    turnId: overrides.turnId ?? `turn-${Date.now()}`,
    payload: { type: "turn_complete", outcome: { text: "hello" } },
  };
}

describe("DeliveryQueue", () => {
  let queue: DeliveryQueue;

  afterEach(() => {
    queue?.dispose();
  });

  it("enqueues and flushes by session", () => {
    queue = new DeliveryQueue();
    queue.enqueue(makeEntry({ sessionId: "s1" }));
    queue.enqueue(makeEntry({ sessionId: "s1" }));
    queue.enqueue(makeEntry({ sessionId: "s2" }));

    expect(queue.size).toBe(3);
    expect(queue.hasPending("s1")).toBe(true);
    expect(queue.hasPending("s2")).toBe(true);
    expect(queue.hasPending("s3")).toBe(false);

    const flushed = queue.flush("s1");
    expect(flushed).toHaveLength(2);
    expect(queue.size).toBe(1);
    expect(queue.hasPending("s1")).toBe(false);
  });

  it("flushes by nous ID", () => {
    queue = new DeliveryQueue();
    queue.enqueue(makeEntry({ sessionId: "s1", nousId: "main" }));
    queue.enqueue(makeEntry({ sessionId: "s1", nousId: "akron" }));
    queue.enqueue(makeEntry({ sessionId: "s2", nousId: "main" }));

    const flushed = queue.flushByNous("main");
    expect(flushed).toHaveLength(2);
    expect(queue.size).toBe(1); // only akron's entry remains
  });

  it("caps per-session entries at MAX_PER_SESSION", () => {
    queue = new DeliveryQueue();
    for (let i = 0; i < 12; i++) {
      queue.enqueue(makeEntry({ sessionId: "s1", turnId: `turn-${i}` }));
    }
    // MAX_PER_SESSION is 10, so oldest 2 should be dropped
    expect(queue.flush("s1")).toHaveLength(10);
  });

  it("returns empty array when flushing non-existent session", () => {
    queue = new DeliveryQueue();
    expect(queue.flush("nonexistent")).toEqual([]);
  });

  it("returns empty array when flushing non-existent nous", () => {
    queue = new DeliveryQueue();
    expect(queue.flushByNous("nonexistent")).toEqual([]);
  });

  it("sets correct metadata on enqueued entries", () => {
    queue = new DeliveryQueue();
    const before = Date.now();
    queue.enqueue(makeEntry({ sessionId: "s1" }));
    const after = Date.now();

    const [entry] = queue.flush("s1");
    expect(entry!.attempts).toBe(1);
    expect(entry!.createdAt).toBeGreaterThanOrEqual(before);
    expect(entry!.createdAt).toBeLessThanOrEqual(after);
    expect(entry!.lastAttemptAt).toBe(entry!.createdAt);
  });

  it("dispose stops cleanup timer without error", () => {
    queue = new DeliveryQueue();
    queue.dispose();
    // Double dispose should be safe
    queue.dispose();
  });
});
