// Hono HTTP gateway
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { CronScheduler } from "../daemon/cron.js";
import type { Watchdog } from "../daemon/watchdog.js";

const log = createLogger("pylon");

// Set after gateway creation — avoids circular dependency
let cronRef: CronScheduler | null = null;
let watchdogRef: Watchdog | null = null;
export function setCronRef(cron: CronScheduler): void {
  cronRef = cron;
}
export function setWatchdogRef(wd: Watchdog): void {
  watchdogRef = wd;
}

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
          cacheReadTokens: result.cacheReadTokens,
          cacheWriteTokens: result.cacheWriteTokens,
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

  app.get("/api/metrics", (c) => {
    const metrics = store.getMetrics();
    const uptime = process.uptime();

    const nous = config.agents.list.map((a) => {
      const nousMetrics = metrics.perNous[a.id];
      const nousUsage = metrics.usageByNous[a.id];
      return {
        id: a.id,
        name: a.name ?? a.id,
        activeSessions: nousMetrics?.activeSessions ?? 0,
        totalMessages: nousMetrics?.totalMessages ?? 0,
        lastActivity: nousMetrics?.lastActivity ?? null,
        tokens: nousUsage
          ? {
              input: nousUsage.inputTokens,
              output: nousUsage.outputTokens,
              cacheRead: nousUsage.cacheReadTokens,
              cacheWrite: nousUsage.cacheWriteTokens,
              turns: nousUsage.turns,
            }
          : null,
      };
    });

    const cacheHitRate =
      metrics.usage.totalInputTokens > 0
        ? Math.round(
            (metrics.usage.totalCacheReadTokens /
              metrics.usage.totalInputTokens) *
              100,
          )
        : 0;

    return c.json({
      status: "ok",
      uptime: Math.round(uptime),
      timestamp: new Date().toISOString(),
      nous,
      usage: {
        ...metrics.usage,
        cacheHitRate,
      },
      cron: cronRef?.getStatus() ?? [],
      services: watchdogRef?.getStatus() ?? [],
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
