// Memory sidecar proxy routes
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

const memoryUrl = process.env["MEMORY_SIDECAR_URL"] ?? "http://127.0.0.1:8230";

function memorySidecarHeaders(extra?: Record<string, string>): Record<string, string> {
  const headers: Record<string, string> = { ...extra };
  const key = process.env["ALETHEIA_MEMORY_KEY"];
  if (key) headers["Authorization"] = `Bearer ${key}`;
  return headers;
}

export function memoryRoutes(_deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();

  app.get("/api/memory/graph/export", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/export${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/search", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/search${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph_stats", async (c) => {
    const res = await fetch(`${memoryUrl}/graph_stats`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.post("/api/memory/graph/analyze", async (c) => {
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/graph/analyze`, {
      method: "POST",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/entity/:name", async (c) => {
    const name = c.req.param("name");
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.delete("/api/memory/entity/:name", async (c) => {
    const name = c.req.param("name");
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}`, {
      method: "DELETE",
      headers: memorySidecarHeaders(),
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.patch("/api/memory/entity/:name/flag", async (c) => {
    const name = c.req.param("name");
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}/flag`, {
      method: "PATCH",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.post("/api/memory/entity/merge", async (c) => {
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/entity/merge`, {
      method: "POST",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/health", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/memory/health${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/timeline", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/timeline${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/agent-overlay", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/agent-overlay${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/drift", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/drift${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  return app;
}
