// Audit log route
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function auditRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { auditLog } = deps;

  app.get("/api/audit", (c) => {
    if (!auditLog) return c.json({ entries: [] });
    const actor = c.req.query("actor");
    const since = c.req.query("since");
    const limit = Math.min(parseInt(c.req.query("limit") ?? "100", 10), 500);
    const entries = auditLog.query({
      ...(actor ? { actor } : {}),
      ...(since ? { since } : {}),
      limit,
    });
    return c.json({ entries });
  });

  return app;
}
