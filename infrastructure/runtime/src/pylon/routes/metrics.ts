// Metrics, tool stats, cron, and reflection routes
import { Hono } from "hono";
import { createLogger } from "../../koina/logger.js";
import { computeSelfAssessment } from "../../distillation/reflect.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function metricRoutes(deps: RouteDeps, refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, manager, store } = deps;

  app.get("/api/tool-stats", (c) => {
    const agentId = c.req.query("agentId");
    const window = c.req.query("window");
    let windowHours = 168;
    if (window) {
      const match = window.match(/^(\d+)(h|d)$/);
      if (match) {
        windowHours = match[2] === "d" ? parseInt(match[1]!, 10) * 24 : parseInt(match[1]!, 10);
      }
    }
    const stats = store.getToolStats({
      ...(agentId ? { nousId: agentId } : {}),
      windowHours,
    });
    return c.json({ ok: true, window: `${windowHours}h`, stats });
  });

  app.get("/api/reflection/:nousId", (c) => {
    const nousId = c.req.param("nousId");
    const limit = parseInt(c.req.query("limit") ?? "10", 10);
    const logs = store.getReflectionLog(nousId, { limit });
    return c.json({ nousId, reflections: logs });
  });

  app.get("/api/reflection/:nousId/assessment", (c) => {
    const nousId = c.req.param("nousId");
    const assessment = computeSelfAssessment(store, nousId);
    return c.json({ nousId, assessment });
  });

  app.get("/api/reflection/:nousId/latest", (c) => {
    const nousId = c.req.param("nousId");
    const last = store.getLastReflection(nousId);
    if (!last) return c.json({ nousId, reflection: null });
    return c.json({ nousId, reflection: last });
  });

  app.get("/api/cron", (c) => {
    return c.json({ jobs: refs.cron()?.getStatus() ?? [] });
  });

  app.post("/api/cron/:id/trigger", async (c) => {
    const cronRef = refs.cron();
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
      (metrics.usage.totalInputTokens + metrics.usage.totalCacheReadTokens) > 0
        ? Math.round(
            (metrics.usage.totalCacheReadTokens /
              (metrics.usage.totalInputTokens + metrics.usage.totalCacheReadTokens)) *
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
      cron: refs.cron()?.getStatus() ?? [],
      services: refs.watchdog()?.getStatus() ?? [],
      mcp: refs.mcp()?.getStatus() ?? [],
    });
  });

  return app;
}
