// Cost attribution routes
import { Hono } from "hono";
import { calculateCostBreakdown } from "../../hermeneus/pricing.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function costRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, store } = deps;

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
        model: null,
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

  app.get("/api/costs/daily", (c) => {
    const days = Math.min(Number(c.req.query("days")) || 30, 90);
    const rows = store.getDailyCosts(days);
    const daily = rows.map((r) => {
      const cost = calculateCostBreakdown({
        inputTokens: r.inputTokens,
        outputTokens: r.outputTokens,
        cacheReadTokens: r.cacheReadTokens,
        cacheWriteTokens: r.cacheWriteTokens,
        model: null,
      });
      return {
        date: r.date,
        cost: Math.round(cost.totalCost * 10000) / 10000,
        tokens: r.inputTokens + r.outputTokens + r.cacheReadTokens + r.cacheWriteTokens,
        turns: r.turns,
      };
    });
    return c.json({ daily });
  });

  return app;
}
