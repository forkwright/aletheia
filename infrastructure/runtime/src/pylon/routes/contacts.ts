// Contact/pairing routes
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function contactRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { store } = deps;

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

  return app;
}
