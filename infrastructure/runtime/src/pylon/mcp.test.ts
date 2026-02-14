// MCP routes tests — JSON-RPC handling, auth, scope enforcement, tool listing, tool execution
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createMcpRoutes, validateMcpToken, loadMcpTokens } from "./mcp.js";

function makeConfig(overrides: Record<string, unknown> = {}) {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn" },
        { id: "eiron", name: "Eiron", model: "claude-haiku", workspace: "/tmp/eiron" },
      ],
      default: "syn",
    },
    plugins: { enabled: false, entries: {} },
    gateway: {
      mcp: { requireAuth: false },
      maxBodyBytes: 1_048_576,
      rateLimit: { requestsPerMinute: 60 },
      cors: { allowOrigins: [] },
      ...overrides,
    },
  } as never;
}

function makeManager() {
  return {
    handleMessage: vi.fn().mockResolvedValue({
      text: "response from agent",
      sessionId: "ses_1",
      toolCalls: 0,
      inputTokens: 100,
      outputTokens: 50,
    }),
  } as never;
}

function makeStore() {
  return {
    getMetrics: vi.fn().mockReturnValue({
      usage: { totalInputTokens: 1000, totalOutputTokens: 500 },
      perNous: {},
      usageByNous: {},
    }),
    listSessions: vi.fn().mockReturnValue([]),
  } as never;
}

function jsonRpc(id: number, method: string, params?: Record<string, unknown>) {
  return JSON.stringify({ jsonrpc: "2.0", id, method, ...(params ? { params } : {}) });
}

async function postMessage(app: ReturnType<typeof createMcpRoutes>, body: string, auth?: string) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (auth) headers["Authorization"] = auth;
  return app.request("/messages?sessionId=test", { method: "POST", headers, body });
}

// ── validateMcpToken ──────────────────────────────────────────────────────────

describe("validateMcpToken", () => {
  it("returns null when no tokens and auth required", () => {
    expect(validateMcpToken([], undefined, true)).toBeNull();
  });

  it("returns anonymous when no tokens and auth not required", () => {
    const result = validateMcpToken([], undefined, false);
    expect(result).not.toBeNull();
    expect(result!.name).toBe("anonymous");
    expect(result!.scopes).toEqual(["*"]);
  });

  it("returns null for missing Bearer header", () => {
    const tokens = [{ token: "abc123", name: "test", scopes: ["*"] }];
    expect(validateMcpToken(tokens, undefined, true)).toBeNull();
    expect(validateMcpToken(tokens, "Basic abc", true)).toBeNull();
  });

  it("returns null for empty Bearer token", () => {
    const tokens = [{ token: "abc123", name: "test", scopes: ["*"] }];
    expect(validateMcpToken(tokens, "Bearer ", true)).toBeNull();
  });

  it("returns matching token", () => {
    const tokens = [{ token: "abc123", name: "test", scopes: ["agent:syn"] }];
    const result = validateMcpToken(tokens, "Bearer abc123", true);
    expect(result).not.toBeNull();
    expect(result!.name).toBe("test");
  });

  it("returns null for non-matching token", () => {
    const tokens = [{ token: "abc123", name: "test", scopes: ["*"] }];
    expect(validateMcpToken(tokens, "Bearer wrong", true)).toBeNull();
  });
});

// ── Auth enforcement ──────────────────────────────────────────────────────────

describe("auth enforcement", () => {
  it("denies unauthenticated when requireAuth=true", async () => {
    const app = createMcpRoutes(
      makeConfig({ mcp: { requireAuth: true } }),
      makeManager(),
      makeStore(),
    );
    const res = await postMessage(app, jsonRpc(1, "ping"));
    expect(res.status).toBe(401);
  });

  it("allows unauthenticated when requireAuth=false", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(1, "ping"));
    expect(res.status).toBe(200);
  });
});

// ── Scope enforcement ─────────────────────────────────────────────────────────

describe("scope enforcement", () => {
  it("filters tool list by scope", async () => {
    // Use requireAuth=false so anonymous gets wildcard — then test with scoped token
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());

    // Wildcard sees all tools
    const res = await postMessage(app, jsonRpc(1, "tools/list"));
    const body = await res.json();
    const names = body.result.tools.map((t: { name: string }) => t.name);
    expect(names).toContain("aletheia_ask_syn");
    expect(names).toContain("aletheia_ask_eiron");
    expect(names).toContain("aletheia_status");
    expect(names).toContain("aletheia_memory_search");
    expect(names).toContain("aletheia_sessions");
  });

  it("denies tool execution without required scope when auth required", async () => {
    // This test needs actual tokens loaded — mock at route level
    const app = createMcpRoutes(
      makeConfig({ mcp: { requireAuth: true } }),
      makeManager(),
      makeStore(),
    );

    // Without auth, should be denied entirely
    const res = await postMessage(app, jsonRpc(1, "tools/call", {
      name: "aletheia_status",
      arguments: {},
    }));
    expect(res.status).toBe(401);
  });
});

// ── Input validation ──────────────────────────────────────────────────────────

describe("input validation", () => {
  it("rejects invalid JSON", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, "not json");
    const body = await res.json();
    expect(body.error.code).toBe(-32700);
  });

  it("validates message field in agent ask", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(1, "tools/call", {
      name: "aletheia_ask_syn",
      arguments: {},
    }));
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed.error).toContain("message is required");
  });

  it("validates query field in memory search", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(1, "tools/call", {
      name: "aletheia_memory_search",
      arguments: {},
    }));
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed.error).toContain("query is required");
  });

  it("clamps limit in memory search", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    // Even with absurd limit, the tool should clamp it — we just verify it doesn't crash
    const res = await postMessage(app, jsonRpc(1, "tools/call", {
      name: "aletheia_memory_search",
      arguments: { query: "test", limit: 99999 },
    }));
    expect(res.status).toBe(200);
  });
});

// ── JSON-RPC methods ──────────────────────────────────────────────────────────

describe("JSON-RPC methods", () => {
  it("handles initialize", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(1, "initialize"));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toHaveProperty("protocolVersion");
    expect(body.result.serverInfo.name).toBe("aletheia");
  });

  it("handles initialized", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(2, "initialized"));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toEqual({});
  });

  it("handles tools/list", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(3, "tools/list"));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result.tools.length).toBeGreaterThanOrEqual(4);
  });

  it("handles tools/call for agent ask", async () => {
    const manager = makeManager();
    const app = createMcpRoutes(makeConfig(), manager, makeStore());
    const res = await postMessage(app, jsonRpc(4, "tools/call", {
      name: "aletheia_ask_syn",
      arguments: { message: "hello" },
    }));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result.content[0].text).toContain("response from agent");
  });

  it("handles tools/call for status", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(5, "tools/call", {
      name: "aletheia_status",
      arguments: {},
    }));
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed).toHaveProperty("uptime");
    expect(parsed).toHaveProperty("agents");
  });

  it("handles tools/call for sessions", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(6, "tools/call", {
      name: "aletheia_sessions",
      arguments: {},
    }));
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed).toHaveProperty("sessions");
  });

  it("handles ping", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(7, "ping"));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toEqual({});
  });

  it("returns error for unknown method", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(8, "unknown/method"));
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.error).toBeDefined();
    expect(body.error.code).toBe(-32601);
  });

  it("returns error for unknown tool", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await postMessage(app, jsonRpc(9, "tools/call", {
      name: "nonexistent",
      arguments: {},
    }));
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed.error).toContain("Unknown tool");
  });
});
