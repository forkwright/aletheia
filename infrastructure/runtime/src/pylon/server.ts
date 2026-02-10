// Hono HTTP gateway
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";

const log = createLogger("pylon");

export function createGateway(
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
): Hono {
  const app = new Hono();

  // Auth middleware — skip /health
  const authMode = config.gateway.auth.mode;
  const authToken = config.gateway.auth.token;

  app.use("*", async (c, next) => {
    if (c.req.path === "/health") return next();

    if (authMode === "token" && authToken) {
      const header = c.req.header("Authorization");
      const token = header?.startsWith("Bearer ")
        ? header.slice(7)
        : c.req.query("token");

      if (token !== authToken) {
        return c.json({ error: "Unauthorized" }, 401);
      }
    } else if (authMode === "password" && authToken) {
      const header = c.req.header("Authorization");
      if (!header?.startsWith("Basic ")) {
        c.header("WWW-Authenticate", 'Basic realm="Aletheia"');
        return c.json({ error: "Unauthorized" }, 401);
      }
      const decoded = Buffer.from(header.slice(6), "base64").toString();
      const password = decoded.includes(":") ? decoded.split(":").slice(1).join(":") : decoded;
      if (password !== authToken) {
        return c.json({ error: "Invalid credentials" }, 401);
      }
    }

    return next();
  });

  app.get("/health", (c) =>
    c.json({ status: "ok", timestamp: new Date().toISOString() }),
  );

  app.get("/api/status", (c) =>
    c.json({
      status: "ok",
      agents: config.agents.list.map((a) => a.id),
      timestamp: new Date().toISOString(),
    }),
  );

  app.get("/api/sessions", (c) => {
    const nousId = c.req.query("nousId");
    const sessions = store.listSessions(nousId);
    return c.json({ sessions });
  });

  app.get("/api/sessions/:id/history", (c) => {
    const id = c.req.param("id");
    const limit = parseInt(c.req.query("limit") ?? "100", 10);
    const history = store.getHistory(id, { limit });
    return c.json({ messages: history });
  });

  app.post("/api/sessions/send", async (c) => {
    const body = await c.req.json();
    const { agentId, message, sessionKey } = body as {
      agentId: string;
      message: string;
      sessionKey?: string;
    };

    if (!agentId || !message) {
      return c.json({ error: "agentId and message required" }, 400);
    }

    try {
      const result = await manager.handleMessage({
        text: message,
        nousId: agentId,
        sessionKey: sessionKey ?? "main",
      });
      return c.json({
        response: result.text,
        sessionId: result.sessionId,
        toolCalls: result.toolCalls,
        usage: {
          inputTokens: result.inputTokens,
          outputTokens: result.outputTokens,
        },
      });
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      log.error(`Session send failed: ${msg}`);
      return c.json({ error: msg }, 500);
    }
  });

  app.get("/api/config", (c) => {
    return c.json({
      agents: config.agents.list.map((a) => ({
        id: a.id,
        name: a.name ?? a.id,
        workspace: a.workspace,
      })),
      bindings: config.bindings.length,
      plugins: Object.keys(config.plugins.entries).length,
    });
  });

  return app;
}

export function startGateway(
  app: Hono,
  port: number,
): ReturnType<typeof serve> {
  log.info(`Starting gateway on port ${port}`);
  return serve({
    fetch: app.fetch,
    port,
  });
}
