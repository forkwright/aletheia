// Blackboard API routes
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function blackboardRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { store } = deps;

  app.get("/api/blackboard", (c) => {
    return c.json({ entries: store.blackboardList() });
  });

  app.post("/api/blackboard", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }
    const key = body["key"] as string;
    const value = body["value"] as string;
    const author = (body["author"] as string) ?? "prosoche";
    const ttl = (body["ttl_seconds"] as number) ?? 3600;
    if (!key || !value) {
      return c.json({ error: "key and value required" }, 400);
    }
    const id = store.blackboardWrite(key, value, author, ttl);
    return c.json({ ok: true, id });
  });

  return app;
}
