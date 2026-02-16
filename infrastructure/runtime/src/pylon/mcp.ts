// MCP (Model Context Protocol) server — exposes Aletheia agents as MCP tools
import { Hono } from "hono";
import { readFileSync, existsSync } from "node:fs";
import { createLogger } from "../koina/logger.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";

const log = createLogger("pylon.mcp");

const MCP_VERSION = "2024-11-05";

interface McpToken {
  token: string;
  name: string;
  scopes: string[];
}

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: string | number;
  method: string;
  params?: Record<string, unknown>;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: string | number | null;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

function loadMcpTokens(credPath: string): McpToken[] {
  const tokensPath = `${credPath}/mcp-tokens.json`;
  if (!existsSync(tokensPath)) return [];
  try {
    const raw = readFileSync(tokensPath, "utf-8");
    return JSON.parse(raw) as McpToken[];
  } catch {
    log.warn("Failed to load MCP tokens");
    return [];
  }
}

function validateMcpToken(tokens: McpToken[], authHeader: string | undefined): McpToken | null {
  if (tokens.length === 0) return { token: "", name: "default", scopes: ["*"] };
  if (!authHeader?.startsWith("Bearer ")) return null;
  const token = authHeader.slice(7);
  return tokens.find((t) => t.token === token) ?? null;
}

export function createMcpRoutes(
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
): Hono {
  const app = new Hono();
  const credPath = process.env["ALETHEIA_HOME"]
    ? `${process.env["ALETHEIA_HOME"]}/credentials`
    : `${process.env["HOME"]}/.aletheia/credentials`;
  const tokens = loadMcpTokens(credPath);

  // SSE endpoint for MCP transport
  app.get("/sse", async (c) => {
    const client = validateMcpToken(tokens, c.req.header("Authorization"));
    if (!client) return c.json({ error: "Unauthorized" }, 401);

    const sessionId = `mcp_${Date.now().toString(36)}`;

    c.header("Content-Type", "text/event-stream");
    c.header("Cache-Control", "no-cache");
    c.header("Connection", "keep-alive");

    // Send the endpoint URL for the client to POST JSON-RPC messages
    const postUrl = `/mcp/messages?sessionId=${sessionId}`;

    return c.body(
      new ReadableStream({
        start(controller) {
          const encoder = new TextEncoder();
          const send = (event: string, data: string) => {
            controller.enqueue(encoder.encode(`event: ${event}\ndata: ${data}\n\n`));
          };

          // Send endpoint event per MCP spec
          send("endpoint", postUrl);

          // Keep-alive ping every 30s
          const keepAlive = setInterval(() => {
            try {
              controller.enqueue(encoder.encode(": ping\n\n"));
            } catch {
              clearInterval(keepAlive);
            }
          }, 30000);

          // Store the controller for sending responses back
          mcpSessions.set(sessionId, { send, controller, keepAlive });
        },
        cancel() {
          const session = mcpSessions.get(sessionId);
          if (session) {
            clearInterval(session.keepAlive);
            mcpSessions.delete(sessionId);
          }
        },
      }),
    );
  });

  // JSON-RPC message endpoint
  app.post("/messages", async (c) => {
    const client = validateMcpToken(tokens, c.req.header("Authorization"));
    if (!client) return c.json({ error: "Unauthorized" }, 401);

    const sessionId = c.req.query("sessionId");
    const session = sessionId ? mcpSessions.get(sessionId) : null;

    let request: JsonRpcRequest;
    try {
      request = (await c.req.json()) as JsonRpcRequest;
    } catch {
      return c.json({ jsonrpc: "2.0", id: null, error: { code: -32700, message: "Parse error" } });
    }

    const response = await handleJsonRpc(request, config, manager, store, client);

    // If SSE session exists, send response via SSE
    if (session) {
      session.send("message", JSON.stringify(response));
    }

    return c.json(response);
  });

  log.info(`MCP routes registered (${tokens.length} tokens loaded)`);
  return app;
}

const mcpSessions = new Map<string, {
  send: (event: string, data: string) => void;
  controller: ReadableStreamDefaultController;
  keepAlive: ReturnType<typeof setInterval>;
}>();

async function handleJsonRpc(
  request: JsonRpcRequest,
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
  client: McpToken,
): Promise<JsonRpcResponse> {
  const { id, method, params } = request;

  switch (method) {
    case "initialize":
      return {
        jsonrpc: "2.0",
        id,
        result: {
          protocolVersion: MCP_VERSION,
          capabilities: {
            tools: { listChanged: false },
          },
          serverInfo: {
            name: "aletheia",
            version: "0.2.0",
          },
        },
      };

    case "initialized":
      return { jsonrpc: "2.0", id, result: {} };

    case "tools/list":
      return {
        jsonrpc: "2.0",
        id,
        result: {
          tools: buildMcpToolList(config, client),
        },
      };

    case "tools/call": {
      const toolName = (params?.["name"] ?? "") as string;
      const toolArgs = (params?.["arguments"] ?? {}) as Record<string, unknown>;
      const result = await executeMcpTool(toolName, toolArgs, config, manager, store);
      return {
        jsonrpc: "2.0",
        id,
        result: {
          content: [{ type: "text", text: typeof result === "string" ? result : JSON.stringify(result) }],
        },
      };
    }

    case "ping":
      return { jsonrpc: "2.0", id, result: {} };

    default:
      return {
        jsonrpc: "2.0",
        id,
        error: { code: -32601, message: `Method not found: ${method}` },
      };
  }
}

function buildMcpToolList(config: AletheiaConfig, client: McpToken): Array<{
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}> {
  const tools: Array<{ name: string; description: string; inputSchema: Record<string, unknown> }> = [];

  // Per-agent ask tools
  for (const agent of config.agents.list) {
    if (client.scopes[0] !== "*" && !client.scopes.includes(`agent:${agent.id}`)) continue;

    tools.push({
      name: `aletheia_ask_${agent.id}`,
      description: `Ask ${agent.name ?? agent.id} a question and wait for their response.`,
      inputSchema: {
        type: "object",
        properties: {
          message: { type: "string", description: "Question or request to send" },
          sessionKey: { type: "string", description: "Session key (default: 'mcp')" },
        },
        required: ["message"],
      },
    });
  }

  // System tools
  tools.push({
    name: "aletheia_status",
    description: "Get Aletheia system status including agents, services, and usage.",
    inputSchema: { type: "object", properties: {} },
  });

  tools.push({
    name: "aletheia_memory_search",
    description: "Search Aletheia's memory system for facts and knowledge.",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string", description: "Search query" },
        agentId: { type: "string", description: "Filter by agent ID" },
        limit: { type: "number", description: "Max results (default: 10)" },
      },
      required: ["query"],
    },
  });

  tools.push({
    name: "aletheia_sessions",
    description: "List active sessions, optionally filtered by agent.",
    inputSchema: {
      type: "object",
      properties: {
        agentId: { type: "string", description: "Filter by agent ID" },
      },
    },
  });

  return tools;
}

async function executeMcpTool(
  toolName: string,
  args: Record<string, unknown>,
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
): Promise<unknown> {
  // Agent ask tools
  const askMatch = toolName.match(/^aletheia_ask_(.+)$/);
  if (askMatch) {
    const agentId = askMatch[1]!;
    const message = args["message"] as string;
    const sessionKey = (args["sessionKey"] as string) ?? "mcp";

    if (!message) return { error: "message is required" };

    const agent = config.agents.list.find((a) => a.id === agentId);
    if (!agent) return { error: `Unknown agent: ${agentId}` };

    const result = await manager.handleMessage({
      text: message,
      nousId: agentId,
      sessionKey,
      channel: "mcp",
      peerKind: "external",
    });

    return {
      response: result.text,
      sessionId: result.sessionId,
      toolCalls: result.toolCalls,
      tokens: {
        input: result.inputTokens,
        output: result.outputTokens,
      },
    };
  }

  // System status
  if (toolName === "aletheia_status") {
    const metrics = store.getMetrics();
    return {
      uptime: Math.round(process.uptime()),
      agents: config.agents.list.map((a) => ({
        id: a.id,
        name: a.name ?? a.id,
        sessions: metrics.perNous[a.id]?.activeSessions ?? 0,
        messages: metrics.perNous[a.id]?.totalMessages ?? 0,
      })),
      usage: metrics.usage,
    };
  }

  // Memory search (proxied to sidecar)
  if (toolName === "aletheia_memory_search") {
    const query = args["query"] as string;
    const agentId = args["agentId"] as string | undefined;
    const limit = (args["limit"] as number) ?? 10;

    try {
      const body: Record<string, unknown> = { query, limit };
      if (agentId) body["agent_id"] = agentId;

      const res = await fetch("http://127.0.0.1:8230/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(10000),
      });
      return await res.json();
    } catch (err) {
      return { error: `Memory search failed: ${err instanceof Error ? err.message : err}` };
    }
  }

  // Sessions list
  if (toolName === "aletheia_sessions") {
    const agentId = args["agentId"] as string | undefined;
    const sessions = store.listSessions(agentId).slice(0, 20);
    return {
      sessions: sessions.map((s) => ({
        id: s.id,
        nousId: s.nousId,
        status: s.status,
        messages: s.messageCount,
        updated: s.updatedAt,
      })),
    };
  }

  return { error: `Unknown tool: ${toolName}` };
}
