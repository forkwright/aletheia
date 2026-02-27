// System routes — health, status, update, config reload
import { Hono } from "hono";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { createLogger } from "../../koina/logger.js";
import { tryReloadConfig } from "../../taxis/loader.js";
import { eventBus } from "../../koina/event-bus.js";
import { getVersion } from "../../version.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function systemRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config, manager, store } = deps;

  app.get("/health", (c) =>
    c.json({ status: "ok", version: getVersion(), timestamp: new Date().toISOString() }),
  );

  app.get("/api/status", (c) =>
    c.json({
      status: "ok",
      version: getVersion(),
      updateChannel: config.updates?.channel ?? "stable",
      agents: config.agents.list.map((a) => a.id),
      timestamp: new Date().toISOString(),
    }),
  );

  app.get("/api/system/update-channel", (c) =>
    c.json({ channel: config.updates?.channel ?? "stable" }),
  );

  app.get("/api/system/update-status", (c) => {
    const entries = store.blackboardRead("system:update");
    if (!entries || entries.length === 0) return c.json({ available: false });
    try {
      return c.json(JSON.parse(entries[0]!.value));
    } catch {
      return c.json({ available: false });
    }
  });

  app.post("/api/system/update", async (c) => {
    const { execFileSync } = await import("node:child_process");
    const root = process.env["ALETHEIA_ROOT"] ?? join(homedir(), ".aletheia");
    try {
      const gitOutput = execFileSync("git", ["pull", "origin", "main"], {
        cwd: root, timeout: 60_000, encoding: "utf-8",
      });
      const buildOutput = execFileSync("npx", ["tsdown"], {
        cwd: join(root, "infrastructure", "runtime"), timeout: 120_000, encoding: "utf-8",
      });
      log.info(`System update completed: ${(gitOutput + buildOutput).slice(-200)}`);
      return c.json({ ok: true, message: "Update complete. Restart service to apply." });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.warn(`System update failed: ${msg}`);
      return c.json({ error: msg }, 500);
    }
  });

  // Credential labels — exposes configured credential names (no secrets)
  app.get("/api/system/credentials", (c) => {
    const credPath = join(
      process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia"),
      "credentials", "anthropic.json",
    );
    try {
      const raw = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>;
      const primary = typeof raw["label"] === "string" ? raw["label"] : "primary";
      const authType = typeof raw["token"] === "string" ? "oauth" : typeof raw["apiKey"] === "string" ? "api" : "unknown";
      const backups: Array<{ label: string; type: string }> = [];
      const backupCreds = raw["backupCredentials"];
      if (Array.isArray(backupCreds)) {
        for (let i = 0; i < backupCreds.length; i++) {
          const cred = backupCreds[i] as Record<string, unknown> | undefined;
          if (!cred || typeof cred !== "object") continue;
          const label = typeof cred["label"] === "string" ? cred["label"] : `backup-${i + 1}`;
          const type = cred["type"] === "oauth" ? "oauth" : "api";
          backups.push({ label, type });
        }
      }
      const expiresAtMs = typeof raw["expiresAt"] === "number" ? raw["expiresAt"] as number : undefined;
      const expiresAt = expiresAtMs !== undefined ? new Date(expiresAtMs).toISOString() : undefined;
      const now = Date.now();
      return c.json({
        primary: {
          label: primary,
          type: authType,
          ...(expiresAt !== undefined ? {
            expiresAt,
            isExpired: expiresAtMs! < now,
            expiresInMs: expiresAtMs! - now,
          } : {}),
        },
        backups,
      });
    } catch {
      return c.json({ primary: { label: "default", type: "unknown" }, backups: [] });
    }
  });

  // Update primary credential
  app.put("/api/system/credentials/primary", async (c) => {
    const credPath = join(
      process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia"),
      "credentials", "anthropic.json",
    );
    const body = await c.req.json<{ type: "oauth" | "api"; value: string; label?: string }>();
    if (!body.value || typeof body.value !== "string") {
      return c.json({ error: "value is required" }, 400);
    }
    const credType = body.type === "oauth" ? "oauth" : "api";
    try {
      let existing: Record<string, unknown> = {};
      try { existing = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>; } catch { /* new file */ }
      const updated: Record<string, unknown> = {
        ...existing,
        label: typeof body.label === "string" && body.label ? body.label : (existing["label"] ?? "primary"),
      };
      if (credType === "oauth") {
        updated["token"] = body.value;
        delete updated["apiKey"];
      } else {
        updated["apiKey"] = body.value;
        delete updated["token"];
        delete updated["expiresAt"];
      }
      writeFileSync(credPath, JSON.stringify(updated), "utf-8");
      log.info("Primary credential updated", { type: credType });
      return c.json({ ok: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.warn("Failed to update primary credential", { err: msg });
      return c.json({ error: msg }, 500);
    }
  });

  // Add a backup credential
  app.post("/api/system/credentials/backups", async (c) => {
    const credPath = join(
      process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia"),
      "credentials", "anthropic.json",
    );
    const body = await c.req.json<{ type: "oauth" | "api"; value: string; label: string }>();
    if (!body.value || !body.label) return c.json({ error: "value and label are required" }, 400);
    const credType = body.type === "oauth" ? "oauth" : "api";
    try {
      let existing: Record<string, unknown> = {};
      try { existing = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>; } catch { /* new file */ }
      const backups = Array.isArray(existing["backupCredentials"])
        ? (existing["backupCredentials"] as Array<Record<string, unknown>>).filter(b => b["label"] !== body.label)
        : [];
      const entry: Record<string, string> = { label: body.label, type: credType };
      if (credType === "oauth") entry["token"] = body.value;
      else entry["apiKey"] = body.value;
      backups.push(entry);
      writeFileSync(credPath, JSON.stringify({ ...existing, backupCredentials: backups }), "utf-8");
      log.info("Backup credential added", { label: body.label, type: credType });
      return c.json({ ok: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return c.json({ error: msg }, 500);
    }
  });

  // Remove a backup credential by label
  app.delete("/api/system/credentials/backups/:label", (c) => {
    const credPath = join(
      process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia"),
      "credentials", "anthropic.json",
    );
    const label = c.req.param("label");
    try {
      const existing = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>;
      const backups = Array.isArray(existing["backupCredentials"])
        ? (existing["backupCredentials"] as Array<Record<string, unknown>>).filter(b => b["label"] !== label)
        : [];
      writeFileSync(credPath, JSON.stringify({ ...existing, backupCredentials: backups }), "utf-8");
      log.info("Backup credential removed", { label });
      return c.json({ ok: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return c.json({ error: msg }, 500);
    }
  });

  app.post("/api/config/reload", (c) => {
    const newConfig = tryReloadConfig();
    if (!newConfig) {
      return c.json({ ok: false, error: "Config validation failed — check logs" }, 400);
    }
    const diff = manager.reloadConfig(newConfig);

    const bindings = newConfig.bindings.map((b) => {
      const entry: { channel: string; peerKind?: string; peerId?: string; accountId?: string; nousId: string } = {
        channel: b.match.channel,
        nousId: b.agentId,
      };
      if (b.match.peer?.kind) entry.peerKind = b.match.peer.kind;
      if (b.match.peer?.id) entry.peerId = b.match.peer.id;
      if (b.match.accountId) entry.accountId = b.match.accountId;
      return entry;
    });
    store.rebuildRoutingCache(bindings);

    eventBus.emit("config:reloaded", { added: diff.added, removed: diff.removed });
    return c.json({ ok: true, added: diff.added, removed: diff.removed, bindings: bindings.length });
  });

  return app;
}
