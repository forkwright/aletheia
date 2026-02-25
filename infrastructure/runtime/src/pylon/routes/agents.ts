// Agent routes — identity, branding, config, CRUD
import { Hono } from "hono";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../../koina/logger.js";
import { tryReloadConfig } from "../../taxis/loader.js";
import { eventBus } from "../../koina/event-bus.js";
import { paths } from "../../taxis/paths.js";
import { scaffoldAgent, type UserProfile } from "../../taxis/scaffold.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function agentRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, manager, store } = deps;

  app.get("/api/agents/:id/identity", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    try {
      const workspace = agent.workspace;
      const identityPath = join(workspace, "IDENTITY.md");
      const raw = readFileSync(identityPath, "utf-8");
      const emojiMatch = raw.match(/emoji:\s*(.+)/i);
      const nameMatch = raw.match(/name:\s*(.+)/i);
      let parsedName = nameMatch?.[1]?.replace(/\*+/g, "").trim() || "";
      if (!parsedName) parsedName = agent.name ?? agent.id;
      let parsedEmoji: string | null = null;
      if (emojiMatch?.[1]) {
        const cleaned = emojiMatch[1].replace(/\*+/g, "").trim();
        const emojiOnly = cleaned.match(/^(\p{Emoji_Presentation}|\p{Emoji}\uFE0F)+/u);
        parsedEmoji = emojiOnly?.[0] || null;
      }
      return c.json({
        id: agent.id,
        name: parsedName,
        emoji: parsedEmoji,
      });
    } catch (err) {
      log.debug(`IDENTITY.md read failed for ${id}: ${err instanceof Error ? err.message : err}`);
      return c.json({
        id: agent.id,
        name: agent.identity?.name ?? agent.name ?? agent.id,
        emoji: agent.identity?.emoji ?? null,
      });
    }
  });

  app.get("/api/branding", (c) => {
    return c.json(config.branding ?? { name: "Aletheia" });
  });

  app.get("/api/config", (c) => {
    return c.json({
      agents: config.agents.list.map((a) => ({
        id: a.id,
        name: a.name ?? a.id,
        workspace: a.workspace,
      })),
      bindings: config.bindings.length,
      plugins: Object.keys(config.plugins.entries).length,
      branding: config.branding,
    });
  });

  app.get("/api/agents", (c) => {
    const agents = config.agents.list.map((a) => ({
      id: a.id,
      name: a.name ?? a.id,
      workspace: a.workspace,
      model: a.model ?? config.agents.defaults.model.primary,
    }));
    return c.json({ agents });
  });

  app.post("/api/agents", async (c) => {
    try {
      const body = await c.req.json<{ id: string; name: string; emoji?: string; userProfile?: UserProfile }>();
      if (!body.id || !body.name) {
        return c.json({ error: "id and name are required" }, 400);
      }
      const scaffoldOpts = {
        id: body.id,
        name: body.name,
        nousDir: paths.nous,
        configPath: paths.configFile(),
        templateDir: join(paths.nous, "_example"),
        ...(body.emoji ? { emoji: body.emoji } : {}),
        ...(body.userProfile ? { userProfile: body.userProfile } : {}),
      };
      const result = scaffoldAgent(scaffoldOpts);

      const newConfig = tryReloadConfig();
      if (newConfig) {
        const diff = manager.reloadConfig(newConfig);
        const bindings = newConfig.bindings.map((b) => {
          const entry: { channel: string; peerKind?: string; peerId?: string; accountId?: string; nousId: string } = {
            channel: b.match.channel, nousId: b.agentId,
          };
          if (b.match.peer?.kind) entry.peerKind = b.match.peer.kind;
          if (b.match.peer?.id) entry.peerId = b.match.peer.id;
          if (b.match.accountId) entry.accountId = b.match.accountId;
          return entry;
        });
        store.rebuildRoutingCache(bindings);
        eventBus.emit("config:reloaded", { added: diff.added, removed: diff.removed });
      }

      return c.json({ ok: true, id: body.id, workspace: result.workspace, filesCreated: result.filesCreated });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      const status = msg.includes("already exists") ? 409 : 400;
      return c.json({ error: msg }, status);
    }
  });

  app.get("/api/agents/:id", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    const sessions = store.listSessions(id).slice(0, 20);
    const metrics = store.getMetrics();
    const usage = metrics.usageByNous[id];

    return c.json({
      id: agent.id,
      name: agent.name ?? agent.id,
      workspace: agent.workspace,
      model: agent.model ?? config.agents.defaults.model.primary,
      sessions,
      usage: usage ?? null,
    });
  });

  return app;
}
