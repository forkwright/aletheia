// MCP routes tests — JSON-RPC handling, tool listing, tool execution
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createMcpRoutes } from "./mcp.js";

function makeConfig() {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn" },
        { id: "eiron", name: "Eiron", model: "claude-haiku", workspace: "/tmp/eiron" },
      ],
      default: "syn",
    },
    plugins: { enabled: false, entries: {} },
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

describe("createMcpRoutes", () => {
  it("creates a Hono app", () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    expect(app).toBeDefined();
  });

  it("POST /messages handles initialize", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toHaveProperty("protocolVersion");
    expect(body.result.serverInfo.name).toBe("aletheia");
  });

  it("POST /messages handles initialized", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 2, method: "initialized" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toEqual({});
  });

  it("POST /messages handles tools/list", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 3, method: "tools/list" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    const tools = body.result.tools;
    expect(tools.length).toBeGreaterThanOrEqual(4); // 2 agent ask + status + memory_search + sessions
    const toolNames = tools.map((t: { name: string }) => t.name);
    expect(toolNames).toContain("aletheia_ask_syn");
    expect(toolNames).toContain("aletheia_ask_eiron");
    expect(toolNames).toContain("aletheia_status");
    expect(toolNames).toContain("aletheia_memory_search");
    expect(toolNames).toContain("aletheia_sessions");
  });

  it("POST /messages handles tools/call for agent ask", async () => {
    const manager = makeManager();
    const app = createMcpRoutes(makeConfig(), manager, makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 4,
        method: "tools/call",
        params: { name: "aletheia_ask_syn", arguments: { message: "hello" } },
      }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result.content[0].text).toContain("response from agent");
  });

  it("POST /messages handles tools/call for status", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 5,
        method: "tools/call",
        params: { name: "aletheia_status", arguments: {} },
      }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed).toHaveProperty("uptime");
    expect(parsed).toHaveProperty("agents");
  });

  it("POST /messages handles tools/call for sessions", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 6,
        method: "tools/call",
        params: { name: "aletheia_sessions", arguments: {} },
      }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed).toHaveProperty("sessions");
  });

  it("POST /messages handles ping", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 7, method: "ping" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.result).toEqual({});
  });

  it("POST /messages returns error for unknown method", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 8, method: "unknown/method" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.error).toBeDefined();
    expect(body.error.code).toBe(-32601);
  });

  it("POST /messages returns parse error for invalid JSON", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "not json",
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.error.code).toBe(-32700);
  });

  it("POST /messages handles unknown tool name", async () => {
    const app = createMcpRoutes(makeConfig(), makeManager(), makeStore());
    const res = await app.request("/messages?sessionId=test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 9,
        method: "tools/call",
        params: { name: "nonexistent", arguments: {} },
      }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    const parsed = JSON.parse(body.result.content[0].text);
    expect(parsed.error).toContain("Unknown tool");
  });
});
