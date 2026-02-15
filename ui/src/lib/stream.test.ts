import { describe, it, expect, vi, beforeEach } from "vitest";
import { streamMessage } from "./stream";

// Mock localStorage for getToken dependency
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key];
    }),
    clear: vi.fn(() => {
      store = {};
    }),
    get length() {
      return Object.keys(store).length;
    },
    key: vi.fn((_i: number) => null),
  };
})();

Object.defineProperty(globalThis, "localStorage", { value: localStorageMock });
vi.stubGlobal("window", { location: { origin: "http://localhost:3000" } });

const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

function makeSSEBody(frames: string): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      controller.enqueue(encoder.encode(frames));
      controller.close();
    },
  });
}

beforeEach(() => {
  localStorageMock.clear();
  mockFetch.mockReset();
});

async function collectEvents(gen: AsyncGenerator<unknown>): Promise<unknown[]> {
  const events: unknown[] = [];
  for await (const event of gen) {
    events.push(event);
  }
  return events;
}

describe("streamMessage", () => {
  it("yields parsed events from SSE response", async () => {
    const ssePayload =
      'event: text_delta\ndata: {"type":"text_delta","text":"hello"}\n\n' +
      'event: turn_complete\ndata: {"type":"turn_complete","outcome":{"text":"done","nousId":"n1","sessionId":"s1","toolCalls":0,"inputTokens":10,"outputTokens":5,"cacheReadTokens":0,"cacheWriteTokens":0}}\n\n';

    mockFetch.mockResolvedValueOnce({
      ok: true,
      status: 200,
      body: makeSSEBody(ssePayload),
    });

    const events = await collectEvents(
      streamMessage("agent-1", "hi", "session-1"),
    );

    expect(events).toHaveLength(2);
    expect(events[0]).toEqual({ type: "text_delta", text: "hello" });
    expect((events[1] as { type: string }).type).toBe("turn_complete");
  });

  it("yields error event on HTTP failure", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      text: async () => "Server Error",
    });

    const events = await collectEvents(
      streamMessage("agent-1", "hi", "session-1"),
    );

    expect(events).toHaveLength(1);
    expect(events[0]).toEqual({
      type: "error",
      message: "HTTP 500: Server Error",
    });
  });

  it("yields error event when response has no body", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      status: 200,
      body: null,
    });

    const events = await collectEvents(
      streamMessage("agent-1", "hi", "session-1"),
    );

    expect(events).toHaveLength(1);
    expect(events[0]).toEqual({
      type: "error",
      message: "No response body",
    });
  });

  it("handles aborted requests", async () => {
    const controller = new AbortController();
    controller.abort();

    mockFetch.mockRejectedValueOnce(new DOMException("Aborted", "AbortError"));

    await expect(
      collectEvents(streamMessage("agent-1", "hi", "session-1", controller.signal)),
    ).rejects.toThrow("Aborted");
  });

  it("skips malformed JSON in SSE data", async () => {
    const ssePayload =
      'event: text_delta\ndata: {not valid json}\n\n' +
      'event: text_delta\ndata: {"type":"text_delta","text":"ok"}\n\n';

    mockFetch.mockResolvedValueOnce({
      ok: true,
      status: 200,
      body: makeSSEBody(ssePayload),
    });

    const events = await collectEvents(
      streamMessage("agent-1", "hi", "session-1"),
    );

    expect(events).toHaveLength(1);
    expect(events[0]).toEqual({ type: "text_delta", text: "ok" });
  });
});
