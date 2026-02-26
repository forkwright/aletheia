// Command routes — list and execute Signal-style commands
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";
import type { SendTarget } from "../../semeion/sender.js";
import type { SignalClient } from "../../semeion/client.js";

export function commandRoutes(deps: RouteDeps, refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, manager, store } = deps;

  app.get("/api/commands", (c) => {
    const commandsRef = refs.commands();
    if (!commandsRef) return c.json({ commands: [] });
    const cmds = commandsRef.listAll().map((cmd) => ({
      name: cmd.name,
      description: cmd.description,
      aliases: cmd.aliases ?? [],
    }));
    return c.json({ commands: cmds });
  });

  app.post("/api/command", async (c) => {
    const commandsRef = refs.commands();
    if (!commandsRef) return c.json({ error: "Commands not available" }, 503);
    const body = await c.req.json() as Record<string, unknown>;
    const command = body["command"] as string;
    const sessionId = body["sessionId"] as string | undefined;
    if (!command || typeof command !== "string") {
      return c.json({ error: "Missing command" }, 400);
    }
    const match = commandsRef.match(command);
    if (!match) return c.json({ error: `Unknown command: ${command}` }, 404);
    try {
      const ctx = {
        sender: "webchat",
        senderName: "WebUI",
        isGroup: false,
        accountId: "",
        target: { account: "" } as SendTarget,
        client: {} as SignalClient,
        store,
        config,
        manager,
        watchdog: refs.watchdog(),
        skills: refs.skills(),
        sessionId,
      };
      const result = await match.handler.execute(match.args, ctx);
      return c.json({ ok: true, result });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return c.json({ error: msg }, 500);
    }
  });

  return app;
}
