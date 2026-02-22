// Skills API route
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function skillRoutes(_deps: RouteDeps, refs: RouteRefs): Hono {
  const app = new Hono();

  app.get("/api/skills", (c) => {
    const list = refs.skills()?.listAll() ?? [];
    return c.json({
      skills: list.map((s) => ({
        id: s.id,
        name: s.name,
        description: s.description,
      })),
    });
  });

  return app;
}
