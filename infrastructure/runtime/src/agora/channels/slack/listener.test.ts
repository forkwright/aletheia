// Tests for Slack inbound listener (Spec 34, Phase 3)

import { describe, expect, it, vi, beforeEach } from "vitest";
import { InboundDebouncer } from "./listener.js";

// ---------------------------------------------------------------------------
// InboundDebouncer tests (the listener registration needs a real Bolt App,
// so we test the debouncer independently)
// ---------------------------------------------------------------------------

describe("InboundDebouncer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it("flushes a single message after debounce window", async () => {
    const onFlush = vi.fn().mockResolvedValue(undefined);
    const debouncer = new InboundDebouncer(1500, onFlush);

    debouncer.enqueue("key1", {
      message: { text: "hello", user: "U1", channel: "C1", channel_type: "im" },
      wasMentioned: false,
    });

    expect(onFlush).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1500);

    expect(onFlush).toHaveBeenCalledTimes(1);
    expect(onFlush.mock.calls[0]![0]).toBe("key1");
    expect(onFlush.mock.calls[0]![1]).toHaveLength(1);
  });

  it("coalesces rapid messages from same key", async () => {
    const onFlush = vi.fn().mockResolvedValue(undefined);
    const debouncer = new InboundDebouncer(1500, onFlush);

    debouncer.enqueue("key1", {
      message: { text: "msg1", user: "U1", channel: "C1" },
      wasMentioned: false,
    });

    // Second message before debounce fires
    await vi.advanceTimersByTimeAsync(500);
    debouncer.enqueue("key1", {
      message: { text: "msg2", user: "U1", channel: "C1" },
      wasMentioned: false,
    });

    // Third message
    await vi.advanceTimersByTimeAsync(500);
    debouncer.enqueue("key1", {
      message: { text: "msg3", user: "U1", channel: "C1" },
      wasMentioned: false,
    });

    // Not flushed yet
    expect(onFlush).not.toHaveBeenCalled();

    // Advance past debounce window from last message
    await vi.advanceTimersByTimeAsync(1500);

    expect(onFlush).toHaveBeenCalledTimes(1);
    // All 3 messages coalesced
    expect(onFlush.mock.calls[0]![1]).toHaveLength(3);
  });

  it("keeps different keys separate", async () => {
    const onFlush = vi.fn().mockResolvedValue(undefined);
    const debouncer = new InboundDebouncer(1500, onFlush);

    debouncer.enqueue("user1", {
      message: { text: "from user 1", user: "U1", channel: "C1" },
      wasMentioned: false,
    });

    debouncer.enqueue("user2", {
      message: { text: "from user 2", user: "U2", channel: "C1" },
      wasMentioned: false,
    });

    await vi.advanceTimersByTimeAsync(1500);

    expect(onFlush).toHaveBeenCalledTimes(2);
  });

  it("flushAll() fires all pending immediately", async () => {
    const onFlush = vi.fn().mockResolvedValue(undefined);
    const debouncer = new InboundDebouncer(1500, onFlush);

    debouncer.enqueue("key1", {
      message: { text: "pending1", user: "U1", channel: "C1" },
      wasMentioned: false,
    });
    debouncer.enqueue("key2", {
      message: { text: "pending2", user: "U2", channel: "C1" },
      wasMentioned: false,
    });

    debouncer.flushAll();

    // Both should flush immediately
    expect(onFlush).toHaveBeenCalledTimes(2);
  });

  it("tracks wasMentioned across coalesced messages", async () => {
    const onFlush = vi.fn().mockResolvedValue(undefined);
    const debouncer = new InboundDebouncer(1500, onFlush);

    debouncer.enqueue("key1", {
      message: { text: "msg1", user: "U1", channel: "C1" },
      wasMentioned: false,
    });
    debouncer.enqueue("key1", {
      message: { text: "msg2", user: "U1", channel: "C1" },
      wasMentioned: true,
    });

    await vi.advanceTimersByTimeAsync(1500);

    const messages = onFlush.mock.calls[0]![1] as Array<{ wasMentioned: boolean }>;
    // At least one message had wasMentioned=true
    expect(messages.some((m) => m.wasMentioned)).toBe(true);
  });
});
