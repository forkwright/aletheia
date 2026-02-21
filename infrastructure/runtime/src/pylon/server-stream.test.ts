// Streaming endpoint tests
import { describe, expect, it, vi } from "vitest";
import { createGateway } from "./server.js";

function makeConfig() {
  return {
    gateway: { port: 18789, auth: { mode: "none" as const, token: undefined } },
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn" },
      ],
      default: "syn",
    },
    cron: { jobs: [] },
    signal: { accounts: [] },
  } as never;
}

function makeStore() {
  return {
    getMetrics: vi.fn().mockReturnValue({
      usage: { totalInputTokens: 0, totalOutputTokens: 0, totalCacheReadTokens: 0, totalCacheWriteTokens: 0, turnCount: 0 },
      perNous: {},
      usageByNous: {},
    }),
    listSessions: vi.fn().mockReturnValue([]),
    findSessionById: vi.fn().mockReturnValue(null),
    getHistory: vi.fn().mockReturnValue([]),
    findOrCreateSession: vi.fn().mockReturnValue({ id: "ses_1", nousId: "syn", messageCount: 0, tokenCountEstimate: 0 }),
    archiveSession: vi.fn(),
    getPendingRequests: vi.fn().mockReturnValue([]),
    approveContactByCode: vi.fn().mockReturnValue(null),
    denyContactByCode: vi.fn().mockReturnValue(false),
    getCostsBySession: vi.fn().mockReturnValue([]),
    getCanonicalSessionKey: vi.fn().mockReturnValue(null),
    resolveRoute: vi.fn().mockReturnValue(null),
  } as never;
}

function makeManager(streamEvents?: Array<Record<string, unknown>>) {
  const events = streamEvents ?? [
    { type: "turn_start", sessionId: "ses_1", nousId: "syn" },
    { type: "text_delta", text: "hello" },
    { type: "text_delta", text: " world" },
    { type: "turn_complete", outcome: { text: "hello world", nousId: "syn", sessionId: "ses_1", toolCalls: 0, inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 } },
  ];

  return {
    handleMessage: vi.fn().mockResolvedValue({
      text: "response",
      nousId: "syn",
      sessionId: "ses_1",
      toolCalls: 0,
      inputTokens: 100,
      outputTokens: 50,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    }),
    handleMessageStreaming: vi.fn().mockImplementation(async function* () {
      for (const event of events) {
        yield event;
      }
    }),
  } as never;
}

async function readSSE(response: Response): Promise<string[]> {
  const text = await response.text();
  return text.split("\n\n").filter((chunk) => chunk.trim().length > 0);
}

describe("POST /api/sessions/stream", () => {
  it("returns SSE content type", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("text/event-stream");
    expect(res.headers.get("Cache-Control")).toBe("no-cache");
    expect(res.headers.get("Connection")).toBe("keep-alive");
  });

  it("requires agentId and message in body", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn" }),
    });

    expect(res.status).toBe(400);
    const body = await res.json();
    expect(body.error).toBe("agentId and message required");
  });

  it("returns 400 for missing agentId", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: "hello" }),
    });

    expect(res.status).toBe(400);
  });

  it("returns 400 for missing both fields", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });

    expect(res.status).toBe(400);
  });

  it("returns 400 for invalid JSON body", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "not-json{{{",
    });

    expect(res.status).toBe(400);
    const body = await res.json();
    expect(body.error).toBe("Invalid JSON body");
  });

  it("streams events from handleMessageStreaming", async () => {
    const manager = makeManager();
    const app = createGateway(makeConfig(), manager, makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    expect(res.status).toBe(200);
    const chunks = await readSSE(res);

    expect(chunks.length).toBeGreaterThanOrEqual(4);

    // Verify SSE format: "event: <type>\ndata: <json>"
    const firstChunk = chunks[0];
    expect(firstChunk).toContain("event: turn_start");
    expect(firstChunk).toContain("data: ");

    // Verify text_delta events
    const textChunks = chunks.filter((c) => c.includes("event: text_delta"));
    expect(textChunks).toHaveLength(2);
  });

  it("passes sessionKey to handleMessageStreaming", async () => {
    const manager = makeManager();
    const app = createGateway(makeConfig(), manager, makeStore());
    await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello", sessionKey: "custom-key" }),
    });

    const mockFn = (manager as unknown as { handleMessageStreaming: ReturnType<typeof vi.fn> }).handleMessageStreaming;
    expect(mockFn).toHaveBeenCalledWith({
      text: "hello",
      nousId: "syn",
      sessionKey: "custom-key",
    });
  });

  it("defaults sessionKey to main", async () => {
    const manager = makeManager();
    const app = createGateway(makeConfig(), manager, makeStore());
    await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    const mockFn = (manager as unknown as { handleMessageStreaming: ReturnType<typeof vi.fn> }).handleMessageStreaming;
    expect(mockFn).toHaveBeenCalledWith({
      text: "hello",
      nousId: "syn",
      sessionKey: "main",
    });
  });

  it("sends error event when streaming fails", async () => {
    const failingManager = {
      handleMessage: vi.fn(),
      handleMessageStreaming: vi.fn().mockImplementation(async function* () {
        yield { type: "turn_start", sessionId: "ses_1", nousId: "syn" };
        throw new Error("provider exploded");
      }),
    } as never;

    const app = createGateway(makeConfig(), failingManager, makeStore());
    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    expect(res.status).toBe(200);
    const chunks = await readSSE(res);

    const errorChunks = chunks.filter((c) => c.includes("event: error"));
    expect(errorChunks).toHaveLength(1);
    expect(errorChunks[0]).toContain("Internal error");
  });

  it("respects auth middleware on stream endpoint", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret123" } };
    const app = createGateway(config, makeManager(), makeStore());

    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    expect(res.status).toBe(401);
  });

  it("allows stream with valid auth token", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret123" } };
    const app = createGateway(config, makeManager(), makeStore());

    const res = await app.request("/api/sessions/stream", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": "Bearer secret123",
      },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("text/event-stream");
  });
});
