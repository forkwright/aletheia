// Setup routes — pre-auth endpoints for initial onboarding wizard
import { Hono } from "hono";
import { existsSync, writeFileSync, readFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { createLogger } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon.setup");

const configDir = (): string => process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia");
const setupFlagFile = (): string => join(configDir(), ".setup-complete");
const credentialFile = (): string => join(configDir(), "credentials", "anthropic.json");
const claudeJsonPath = (): string => join(homedir(), ".claude.json");

export function setupRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config } = deps;

  app.get("/api/setup/status", (c) => {
    return c.json({
      credentialFound: existsSync(credentialFile()),
      agentCount: config.agents.list.length,
      setupComplete: existsSync(setupFlagFile()),
    });
  });

  app.post("/api/setup/credentials", async (c) => {
    let apiKey: string | undefined;

    // Check request body first
    try {
      const body = await c.req.json() as Record<string, unknown>;
      if (typeof body["apiKey"] === "string" && body["apiKey"].trim().length > 0) {
        apiKey = body["apiKey"].trim();
      }
    } catch { /* no body or not JSON */ }

    // Auto-detect from Claude Code's credential store
    if (!apiKey) {
      try {
        const raw = JSON.parse(readFileSync(claudeJsonPath(), "utf-8")) as Record<string, unknown>;
        const pk = raw["primaryApiKey"];
        if (typeof pk === "string" && pk.length > 0) {
          apiKey = pk;
          log.info("Auto-detected API key from ~/.claude.json");
        } else {
          // File exists but no API key — likely OAuth-only Claude Code install
          const hasOAuth = typeof raw["oauthAccount"] === "object" || typeof raw["token"] === "string";
          if (hasOAuth) {
            return c.json({
              success: false,
              error: "Claude Code is using OAuth login, which doesn't include an API key. Get a separate API key at https://console.anthropic.com/keys",
            }, 400);
          }
        }
      } catch {
        log.debug("~/.claude.json not found or unreadable");
      }
    }

    if (!apiKey) {
      return c.json({ success: false, error: "No API key found. Provide one manually or get one at https://console.anthropic.com/keys" }, 400);
    }

    // Validate key format and minimum viable length (sk-ant- prefix + at least 10 chars)
    if (!apiKey.startsWith("sk-ant-") || apiKey.length < 20) {
      return c.json({ success: false, error: "Invalid key format — expected sk-ant-... (full key required)" }, 400);
    }

    try {
      const credDir = join(configDir(), "credentials");
      mkdirSync(credDir, { recursive: true });

      // Preserve existing backupKeys if the file already exists
      let existing: Record<string, unknown> = {};
      try {
        existing = JSON.parse(readFileSync(credentialFile(), "utf-8")) as Record<string, unknown>;
      } catch { /* no existing file */ }

      writeFileSync(credentialFile(), JSON.stringify({ ...existing, apiKey }, null, 2), { mode: 0o600 });
      return c.json({ success: true });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Failed to write credentials: ${msg}`);
      return c.json({ success: false, error: msg }, 500);
    }
  });

  app.post("/api/setup/complete", (c) => {
    try {
      writeFileSync(setupFlagFile(), new Date().toISOString(), "utf-8");
      return c.json({ success: true });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Failed to write setup flag: ${msg}`);
      return c.json({ success: false, error: msg }, 500);
    }
  });

  return app;
}
