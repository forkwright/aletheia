// Thread routes — list, history, summary
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function threadRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { store } = deps;

  app.get("/api/threads", (c) => {
    const nousId = c.req.query("nousId");
    const threads = store.listThreads(nousId ?? undefined);
    return c.json({ threads });
  });

  app.get("/api/threads/:id/history", (c) => {
    const id = c.req.param("id");
    const before = c.req.query("before");
    const limit = Math.min(parseInt(c.req.query("limit") ?? "50", 10), 200);
    const messages = store.getThreadHistory(id, {
      ...(before ? { before } : {}),
      limit,
    });
    return c.json({
      messages: messages.map((m) => ({
        id: m.id,
        sessionId: m.sessionId,
        seq: m.seq,
        role: m.role,
        content: m.content,
        toolCallId: m.toolCallId,
        toolName: m.toolName,
        createdAt: m.createdAt,
      })),
    });
  });

  app.get("/api/threads/:id/summary", (c) => {
    const id = c.req.param("id");
    const summary = store.getThreadSummary(id);
    if (!summary) return c.json({ error: "No summary for this thread" }, 404);
    return c.json(summary);
  });

  return app;
}
