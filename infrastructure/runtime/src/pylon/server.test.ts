// Pylon server tests
import { describe, expect, it, vi } from "vitest";
import { createGateway, setCronRef, setWatchdogRef } from "./server.js";

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
      usage: { totalInputTokens: 1000, totalOutputTokens: 500, totalCacheReadTokens: 200, totalCacheWriteTokens: 100, turnCount: 10 },
      perNous: {},
      usageByNous: { syn: { inputTokens: 1000, outputTokens: 500 } },
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

function makeManager() {
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
  } as never;
}

describe("createGateway", () => {
  it("creates a Hono app", () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    expect(app).toBeDefined();
    expect(app.fetch).toBeDefined();
  });

  it("/health returns ok", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/health");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty("status", "ok");
  });

  it("/api/status returns status", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/status");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty("status", "ok");
    expect(body).toHaveProperty("agents");
  });

  it("/api/sessions returns session list", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions");
    expect(res.status).toBe(200);
  });

  it("/api/agents returns agent list", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/agents");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty("agents");
  });

  it("/api/metrics returns usage data", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/metrics");
    expect(res.status).toBe(200);
  });

  it("/api/contacts/pending returns pending list", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/contacts/pending");
    expect(res.status).toBe(200);
  });

  it("POST /api/sessions/send accepts messages", async () => {
    const manager = makeManager();
    const app = createGateway(makeConfig(), manager, makeStore());
    const res = await app.request("/api/sessions/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });
    expect(res.status).toBe(200);
  });

  it("POST /api/sessions/send rejects missing fields", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hello" }),
    });
    expect(res.status).toBe(400);
  });
});

describe("auth middleware", () => {
  it("token auth rejects missing token", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret123" } };
    const app = createGateway(config, makeManager(), makeStore());
    const res = await app.request("/api/status");
    expect(res.status).toBe(401);
  });

  it("token auth accepts valid bearer token", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret123" } };
    const app = createGateway(config, makeManager(), makeStore());
    const res = await app.request("/api/status", {
      headers: { Authorization: "Bearer secret123" },
    });
    expect(res.status).toBe(200);
  });

  it("health endpoint skips auth", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret" } };
    const app = createGateway(config, makeManager(), makeStore());
    const res = await app.request("/health");
    expect(res.status).toBe(200);
  });
});
