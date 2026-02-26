// MCP server management routes
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";
import type { McpServerConfig } from "../../organon/mcp-client.js";

export function mcpRoutes(deps: RouteDeps, refs: RouteRefs): Hono {
  const app = new Hono();
  const { config } = deps;

  app.get("/api/mcp/servers", (c) => {
    return c.json({ servers: refs.mcp()?.getStatus() ?? [] });
  });

  app.post("/api/mcp/servers/:name/reconnect", async (c) => {
    const mcpRef = refs.mcp();
    if (!mcpRef) return c.json({ error: "MCP not enabled" }, 400);
    const name = c.req.param("name");
    const mcpConfig = (config as Record<string, unknown>)["mcp"] as { servers?: Record<string, unknown> } | undefined;
    const serverConfig = mcpConfig?.servers?.[name];
    if (!serverConfig) return c.json({ error: "Server not found in config" }, 404);
    try {
      await mcpRef.connect(name, serverConfig as McpServerConfig);
      return c.json({ ok: true, name });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return c.json({ error: `Reconnect failed: ${msg}` }, 500);
    }
  });

  return app;
}
