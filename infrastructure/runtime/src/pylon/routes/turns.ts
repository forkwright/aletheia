// Turn management and tool approval routes
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function turnRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, manager } = deps;

  app.get("/api/turns/active", (c) => {
    return c.json({ turns: manager.getActiveTurnDetails() });
  });

  app.post("/api/turns/:id/abort", (c) => {
    const id = c.req.param("id");
    const aborted = manager.abortTurn(id);
    if (!aborted) return c.json({ error: "Turn not found or already completed" }, 404);
    return c.json({ ok: true, turnId: id });
  });

  app.post("/api/turns/:turnId/tools/:toolId/approve", async (c) => {
    const turnId = c.req.param("turnId");
    const toolId = c.req.param("toolId");
    let alwaysAllow = false;
    try {
      const body = await c.req.json() as Record<string, unknown>;
      alwaysAllow = body["alwaysAllow"] === true;
    } catch {
      // No body is fine
    }
    const resolved = manager.approvalGate.resolveApproval(turnId, toolId, {
      decision: "approve",
      alwaysAllow,
    });
    if (!resolved) return c.json({ error: "No pending approval for this tool" }, 404);
    return c.json({ ok: true });
  });

  app.post("/api/turns/:turnId/tools/:toolId/deny", (c) => {
    const turnId = c.req.param("turnId");
    const toolId = c.req.param("toolId");
    const resolved = manager.approvalGate.resolveApproval(turnId, toolId, {
      decision: "deny",
    });
    if (!resolved) return c.json({ error: "No pending approval for this tool" }, 404);
    return c.json({ ok: true });
  });

  app.get("/api/approval/mode", (c) => {
    const approval = (config.agents.defaults as Record<string, unknown>)["approval"] as { mode?: string } | undefined;
    return c.json({ mode: approval?.mode ?? "autonomous" });
  });

  return app;
}
