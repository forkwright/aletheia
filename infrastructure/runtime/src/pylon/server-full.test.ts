// Extended server tests — admin endpoints, costs, auth modes
import { describe, expect, it, vi } from "vitest";
import { createGateway, setCronRef, setSkillsRef, setWatchdogRef } from "./server.js";

function makeConfig(overrides: Record<string, unknown> = {}) {
  return {
    gateway: { port: 18789, auth: { mode: "none", token: undefined } },
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn" },
        { id: "eiron", name: "Eiron", workspace: "/tmp/eiron" },
      ],
      defaults: { model: { primary: "claude-sonnet" } },
    },
    bindings: [{ channel: "signal", nousId: "syn" }],
    plugins: { entries: { "aletheia-memory": {} } },
    cron: { jobs: [] },
    ...overrides,
  } as never;
}

function makeStore(overrides: Record<string, unknown> = {}) {
  return {
    getMetrics: vi.fn().mockReturnValue({
      usage: { totalInputTokens: 100000, totalOutputTokens: 50000, totalCacheReadTokens: 20000, totalCacheWriteTokens: 5000, turnCount: 42 },
      perNous: { syn: { activeSessions: 2, totalMessages: 15, lastActivity: new Date().toISOString() } },
      usageByNous: {
        syn: { inputTokens: 80000, outputTokens: 40000, cacheReadTokens: 15000, cacheWriteTokens: 4000, turns: 30 },
        eiron: { inputTokens: 20000, outputTokens: 10000, cacheReadTokens: 5000, cacheWriteTokens: 1000, turns: 12 },
      },
    }),
    listSessions: vi.fn().mockReturnValue([
      { id: "ses_1", nousId: "syn", messageCount: 10, sessionKey: "main" },
    ]),
    findSessionById: vi.fn().mockReturnValue({ id: "ses_1", nousId: "syn", messageCount: 10 }),
    getHistory: vi.fn().mockReturnValue([
      { seq: 1, role: "user", content: "hello" },
      { seq: 2, role: "assistant", content: "hi" },
    ]),
    archiveSession: vi.fn(),
    getPendingRequests: vi.fn().mockReturnValue([
      { senderName: "John", code: "ABC123", createdAt: new Date().toISOString() },
    ]),
    approveContactByCode: vi.fn().mockReturnValue({ sender: "+999", channel: "signal" }),
    denyContactByCode: vi.fn().mockReturnValue(true),
    getCostsBySession: vi.fn().mockReturnValue([
      { turnSeq: 1, inputTokens: 1000, outputTokens: 500, cacheReadTokens: 100, cacheWriteTokens: 50, model: "claude-sonnet", createdAt: new Date().toISOString() },
    ]),
    getCostsByAgent: vi.fn().mockReturnValue([
      { model: "claude-sonnet", inputTokens: 80000, outputTokens: 40000, cacheReadTokens: 15000, cacheWriteTokens: 4000, turns: 30 },
    ]),
    ...overrides,
  } as never;
}

function makeManager(overrides: Record<string, unknown> = {}) {
  return {
    handleMessage: vi.fn().mockResolvedValue({
      text: "response", nousId: "syn", sessionId: "ses_1", toolCalls: 0,
      inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0,
    }),
    triggerDistillation: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as never;
}

describe("admin API endpoints", () => {
  it("GET /api/agents/:id returns agent detail", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/agents/syn");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.id).toBe("syn");
    expect(body.name).toBe("Syn");
    expect(body.sessions).toBeDefined();
    expect(body.usage).toBeDefined();
  });

  it("GET /api/agents/:id returns 404 for unknown", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/agents/unknown");
    expect(res.status).toBe(404);
  });

  it("GET /api/config returns config overview", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/config");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.agents).toHaveLength(2);
    expect(body.bindings).toBe(1);
    expect(body.plugins).toBe(1);
  });

  it("GET /api/sessions/:id/history returns messages", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/ses_1/history");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.messages).toHaveLength(2);
  });

  it("GET /api/sessions with nousId filter", async () => {
    const store = makeStore();
    const app = createGateway(makeConfig(), makeManager(), store);
    await app.request("/api/sessions?nousId=syn");
    expect((store as unknown as { listSessions: ReturnType<typeof vi.fn> }).listSessions).toHaveBeenCalledWith("syn");
  });
});

describe("cron API", () => {
  it("GET /api/cron returns empty when no cronRef", async () => {
    setCronRef(null as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/cron");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.jobs).toEqual([]);
  });

  it("GET /api/cron returns jobs from cronRef", async () => {
    setCronRef({
      getStatus: vi.fn().mockReturnValue([
        { id: "heartbeat", agentId: "syn", schedule: "every 45m", lastRun: null, nextRun: new Date().toISOString() },
      ]),
    } as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/cron");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.jobs).toHaveLength(1);
    setCronRef(null as never);
  });

  it("POST /api/cron/:id/trigger returns 400 when no cronRef", async () => {
    setCronRef(null as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/cron/heartbeat/trigger", { method: "POST" });
    expect(res.status).toBe(400);
  });

  it("POST /api/cron/:id/trigger returns 404 for unknown job", async () => {
    setCronRef({
      getStatus: vi.fn().mockReturnValue([]),
    } as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/cron/unknown/trigger", { method: "POST" });
    expect(res.status).toBe(404);
    setCronRef(null as never);
  });
});

describe("session management API", () => {
  it("POST /api/sessions/:id/archive archives session", async () => {
    const store = makeStore();
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/sessions/ses_1/archive", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
    expect(body.archived).toBe("ses_1");
  });

  it("POST /api/sessions/:id/archive returns 404 for unknown", async () => {
    const store = makeStore({ findSessionById: vi.fn().mockReturnValue(null) });
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/sessions/unknown/archive", { method: "POST" });
    expect(res.status).toBe(404);
  });

  it("POST /api/sessions/:id/distill triggers distillation", async () => {
    const manager = makeManager();
    const app = createGateway(makeConfig(), manager, makeStore());
    const res = await app.request("/api/sessions/ses_1/distill", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  it("POST /api/sessions/:id/distill returns 404 for unknown", async () => {
    const store = makeStore({ findSessionById: vi.fn().mockReturnValue(null) });
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/sessions/unknown/distill", { method: "POST" });
    expect(res.status).toBe(404);
  });

  it("POST /api/sessions/:id/distill handles error", async () => {
    const manager = makeManager({
      triggerDistillation: vi.fn().mockRejectedValue(new Error("distill failed")),
    });
    const app = createGateway(makeConfig(), manager, makeStore());
    const res = await app.request("/api/sessions/ses_1/distill", { method: "POST" });
    expect(res.status).toBe(500);
  });
});

describe("contacts API", () => {
  it("POST /api/contacts/:code/approve approves contact", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/contacts/ABC123/approve", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  it("POST /api/contacts/:code/approve returns 404 when no match", async () => {
    const store = makeStore({ approveContactByCode: vi.fn().mockReturnValue(null) });
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/contacts/BAD/approve", { method: "POST" });
    expect(res.status).toBe(404);
  });

  it("POST /api/contacts/:code/deny denies contact", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/contacts/ABC123/deny", { method: "POST" });
    expect(res.status).toBe(200);
  });

  it("POST /api/contacts/:code/deny returns 404 when no match", async () => {
    const store = makeStore({ denyContactByCode: vi.fn().mockReturnValue(false) });
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/contacts/BAD/deny", { method: "POST" });
    expect(res.status).toBe(404);
  });
});

describe("skills API", () => {
  it("GET /api/skills returns empty when no skillsRef", async () => {
    setSkillsRef(null as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/skills");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.skills).toEqual([]);
    setSkillsRef(null as never);
  });

  it("GET /api/skills returns skill list", async () => {
    setSkillsRef({
      listAll: vi.fn().mockReturnValue([
        { id: "web-research", name: "Web Research", description: "Research a topic" },
      ]),
    } as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/skills");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.skills).toHaveLength(1);
    expect(body.skills[0].name).toBe("Web Research");
    setSkillsRef(null as never);
  });
});

describe("costs API", () => {
  it("GET /api/costs/summary returns cost breakdown", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/costs/summary");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.totalCost).toBeGreaterThan(0);
    expect(body.agents).toHaveLength(2);
  });

  it("GET /api/costs/session/:id returns session costs", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/costs/session/ses_1");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.sessionId).toBe("ses_1");
    expect(body.turns).toHaveLength(1);
    expect(body.totalCost).toBeGreaterThan(0);
  });

  it("GET /api/costs/session/:id returns 404 for unknown", async () => {
    const store = makeStore({ findSessionById: vi.fn().mockReturnValue(null) });
    const app = createGateway(makeConfig(), makeManager(), store);
    const res = await app.request("/api/costs/session/unknown");
    expect(res.status).toBe(404);
  });

  it("GET /api/costs/agent/:id returns agent costs", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/costs/agent/syn");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.agentId).toBe("syn");
    expect(body.byModel).toHaveLength(1);
    expect(body.totalCost).toBeGreaterThan(0);
  });

  it("GET /api/costs/agent/:id returns 404 for unknown", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/costs/agent/unknown");
    expect(res.status).toBe(404);
  });
});

describe("metrics API", () => {
  it("GET /api/metrics returns full metrics with nous details", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/metrics");
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.status).toBe("ok");
    expect(body.uptime).toBeGreaterThanOrEqual(0);
    expect(body.nous).toHaveLength(2);
    expect(body.usage.cacheHitRate).toBeDefined();
    // syn has usage data
    const syn = body.nous.find((n: { id: string }) => n.id === "syn");
    expect(syn.tokens).toBeDefined();
    expect(syn.tokens.input).toBe(80000);
  });

  it("GET /api/metrics includes watchdog services", async () => {
    setWatchdogRef({
      getStatus: vi.fn().mockReturnValue([
        { name: "neo4j", healthy: true, since: new Date().toISOString() },
      ]),
    } as never);
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/metrics");
    const body = await res.json();
    expect(body.services).toHaveLength(1);
    expect(body.services[0].name).toBe("neo4j");
    setWatchdogRef(null as never);
  });
});

describe("password auth mode", () => {
  it("rejects missing Basic auth header", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "password", token: "mypassword" } };
    const app = createGateway(config, makeManager(), makeStore());
    const res = await app.request("/api/status");
    expect(res.status).toBe(401);
    expect(res.headers.get("WWW-Authenticate")).toContain("Basic");
  });

  it("accepts valid Basic auth credentials", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "password", token: "mypassword" } };
    const app = createGateway(config, makeManager(), makeStore());
    const encoded = btoa("admin:mypassword");
    const res = await app.request("/api/status", {
      headers: { Authorization: `Basic ${encoded}` },
    });
    expect(res.status).toBe(200);
  });

  it("rejects wrong Basic auth password", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "password", token: "mypassword" } };
    const app = createGateway(config, makeManager(), makeStore());
    const encoded = btoa("admin:wrong");
    const res = await app.request("/api/status", {
      headers: { Authorization: `Basic ${encoded}` },
    });
    expect(res.status).toBe(401);
  });
});

describe("token auth query parameter", () => {
  it("accepts token as query parameter", async () => {
    const config = makeConfig();
    (config as Record<string, unknown>).gateway = { port: 18789, auth: { mode: "token", token: "secret123" } };
    const app = createGateway(config, makeManager(), makeStore());
    const res = await app.request("/api/status?token=secret123");
    expect(res.status).toBe(200);
  });
});

describe("error handling", () => {
  it("POST /api/sessions/send with invalid JSON returns 400", async () => {
    const app = createGateway(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/api/sessions/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "not-json",
    });
    expect(res.status).toBe(400);
  });

  it("POST /api/sessions/send handles manager error", async () => {
    const manager = makeManager({
      handleMessage: vi.fn().mockRejectedValue(new Error("boom")),
    });
    const app = createGateway(makeConfig(), manager, makeStore());
    const res = await app.request("/api/sessions/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agentId: "syn", message: "hello" }),
    });
    expect(res.status).toBe(500);
  });
});
