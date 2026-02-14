// Hono HTTP gateway
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { timingSafeEqual } from "node:crypto";
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { CronScheduler } from "../daemon/cron.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { SkillRegistry } from "../organon/skills.js";
import { calculateCostBreakdown } from "../hermeneus/pricing.js";

const log = createLogger("pylon");

function safeCompare(a: string, b: string): boolean {
  const bufA = Buffer.from(a);
  const bufB = Buffer.from(b);
  if (bufA.length !== bufB.length) return false;
  return timingSafeEqual(bufA, bufB);
}

// Set after gateway creation — avoids circular dependency
let cronRef: CronScheduler | null = null;
let watchdogRef: Watchdog | null = null;
let skillsRef: SkillRegistry | null = null;
export function setCronRef(cron: CronScheduler): void {
  cronRef = cron;
}
export function setWatchdogRef(wd: Watchdog): void {
  watchdogRef = wd;
}
export function setSkillsRef(sr: SkillRegistry): void {
  skillsRef = sr;
}

export function createGateway(
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
): Hono {
  const app = new Hono();

  // Security headers
  app.use("*", async (c, next) => {
    c.header("X-Content-Type-Options", "nosniff");
    c.header("X-Frame-Options", "DENY");
    c.header("Referrer-Policy", "no-referrer");
    c.header("X-XSS-Protection", "0");
    return next();
  });

  // CORS
  const allowedOrigins = config.gateway.cors?.allowOrigins ?? [];
  if (allowedOrigins.length > 0) {
    app.use("*", async (c, next) => {
      const origin = c.req.header("Origin");
      if (origin && allowedOrigins.includes(origin)) {
        c.header("Access-Control-Allow-Origin", origin);
        c.header("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        c.header("Access-Control-Allow-Headers", "Content-Type, Authorization");
        c.header("Access-Control-Max-Age", "3600");
      }
      if (c.req.method === "OPTIONS") return c.body(null, 204);
      return next();
    });
  }

  // Rate limiting — sliding window per IP
  const rateLimit = config.gateway.rateLimit?.requestsPerMinute ?? 60;
  const rateBuckets = new Map<string, { count: number; resetAt: number }>();

  app.use("/mcp/*", async (c, next) => {
    const ip = c.req.header("X-Forwarded-For")?.split(",")[0]?.trim()
      ?? c.req.header("X-Real-IP")
      ?? "unknown";
    const now = Date.now();
    const bucket = rateBuckets.get(ip);

    if (bucket && bucket.resetAt > now) {
      if (bucket.count >= rateLimit) {
        c.header("Retry-After", String(Math.ceil((bucket.resetAt - now) / 1000)));
        return c.json({ error: "Rate limit exceeded" }, 429);
      }
      bucket.count++;
    } else {
      rateBuckets.set(ip, { count: 1, resetAt: now + 60_000 });
    }

    return next();
  });

  // Periodic cleanup of expired rate limit buckets
  setInterval(() => {
    const now = Date.now();
    for (const [ip, bucket] of rateBuckets) {
      if (bucket.resetAt <= now) rateBuckets.delete(ip);
    }
  }, 60_000);

  // Auth middleware — skip /health and /ui
  const authMode = config.gateway.auth.mode;
  const authToken = config.gateway.auth.token;

  app.use("*", async (c, next) => {
    if (c.req.path === "/health" || c.req.path.startsWith("/ui")) return next();

    if (authMode === "token" && authToken) {
      const header = c.req.header("Authorization");
      const token = header?.startsWith("Bearer ")
        ? header.slice(7)
        : c.req.query("token");

      if (!safeCompare(token ?? "", authToken)) {
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
      if (!safeCompare(password, authToken)) {
        return c.json({ error: "Invalid credentials" }, 401);
      }
    }

    return next();
  });

  // Global error handler — log details server-side, return generic message to client
  app.onError((err, c) => {
    log.error(`Unhandled error on ${c.req.method} ${c.req.path}: ${err.message}`);
    return c.json({ error: "Internal server error" }, 500);
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
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON body" }, 400);
    }

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
      return c.json({ error: "Internal error processing message" }, 500);
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

  // --- Admin API ---

  app.get("/api/agents", (c) => {
    const agents = config.agents.list.map((a) => ({
      id: a.id,
      name: a.name ?? a.id,
      workspace: a.workspace,
      model: a.model ?? config.agents.defaults.model.primary,
    }));
    return c.json({ agents });
  });

  app.get("/api/agents/:id", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    const sessions = store.listSessions(id).slice(0, 20);
    const metrics = store.getMetrics();
    const usage = metrics.usageByNous[id];

    return c.json({
      id: agent.id,
      name: agent.name ?? agent.id,
      workspace: agent.workspace,
      model: agent.model ?? config.agents.defaults.model.primary,
      sessions,
      usage: usage ?? null,
    });
  });

  app.get("/api/cron", (c) => {
    return c.json({ jobs: cronRef?.getStatus() ?? [] });
  });

  app.post("/api/cron/:id/trigger", async (c) => {
    if (!cronRef) return c.json({ error: "Cron not enabled" }, 400);
    const id = c.req.param("id");
    const jobs = cronRef.getStatus();
    const job = jobs.find((j) => j.id === id);
    if (!job) return c.json({ error: "Job not found" }, 404);

    try {
      await manager.handleMessage({
        text: `[Cron trigger: ${id}] Manual trigger via admin API`,
        sessionKey: `cron:${id}`,
        channel: "cron",
        ...(job.agentId ? { nousId: job.agentId } : {}),
      });
      return c.json({ ok: true, jobId: id });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Cron trigger failed: ${msg}`);
      return c.json({ error: "Failed to trigger cron job" }, 500);
    }
  });

  app.post("/api/sessions/:id/archive", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);
    store.archiveSession(id);
    return c.json({ ok: true, archived: id });
  });

  app.post("/api/sessions/:id/distill", async (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    try {
      await manager.triggerDistillation(id);
      return c.json({ ok: true, sessionId: id });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Distillation trigger failed: ${msg}`);
      return c.json({ error: "Failed to trigger distillation" }, 500);
    }
  });

  // --- Contact/Pairing API ---

  app.get("/api/contacts/pending", (c) => {
    return c.json({ requests: store.getPendingRequests() });
  });

  app.post("/api/contacts/:code/approve", (c) => {
    const code = c.req.param("code");
    const result = store.approveContactByCode(code);
    if (!result) return c.json({ error: "No pending request for that code" }, 404);
    return c.json({ ok: true, ...result });
  });

  app.post("/api/contacts/:code/deny", (c) => {
    const code = c.req.param("code");
    const denied = store.denyContactByCode(code);
    if (!denied) return c.json({ error: "No pending request for that code" }, 404);
    return c.json({ ok: true });
  });

  // --- Skills API ---

  app.get("/api/skills", (c) => {
    const list = skillsRef?.listAll() ?? [];
    return c.json({
      skills: list.map((s) => ({
        id: s.id,
        name: s.name,
        description: s.description,
      })),
    });
  });

  // --- Cost Attribution API ---

  app.get("/api/costs/summary", (c) => {
    const metrics = store.getMetrics();
    const agentCosts = config.agents.list.map((a) => {
      const usage = metrics.usageByNous[a.id];
      if (!usage) return { agentId: a.id, cost: 0, turns: 0 };
      const cost = calculateCostBreakdown({
        inputTokens: usage.inputTokens,
        outputTokens: usage.outputTokens,
        cacheReadTokens: usage.cacheReadTokens,
        cacheWriteTokens: usage.cacheWriteTokens,
        model: null, // mixed models — approximate with default
      });
      return { agentId: a.id, ...cost, turns: usage.turns };
    });
    const totalCost = agentCosts.reduce((sum, a) => sum + ("totalCost" in a ? a.totalCost : a.cost), 0);
    return c.json({ totalCost: Math.round(totalCost * 10000) / 10000, agents: agentCosts });
  });

  app.get("/api/costs/session/:id", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const turns = store.getCostsBySession(id);
    const costs = turns.map((t) => ({
      turn: t.turnSeq,
      ...calculateCostBreakdown(t),
      model: t.model,
      timestamp: t.createdAt,
    }));
    const totalCost = costs.reduce((sum, c) => sum + c.totalCost, 0);
    return c.json({
      sessionId: id,
      nousId: session.nousId,
      totalCost: Math.round(totalCost * 10000) / 10000,
      turns: costs,
    });
  });

  app.get("/api/costs/agent/:id", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    const byModel = store.getCostsByAgent(id);
    const breakdown = byModel.map((m) => ({
      model: m.model,
      ...calculateCostBreakdown(m),
      turns: m.turns,
    }));
    const totalCost = breakdown.reduce((sum, b) => sum + b.totalCost, 0);
    return c.json({
      agentId: id,
      totalCost: Math.round(totalCost * 10000) / 10000,
      byModel: breakdown,
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
