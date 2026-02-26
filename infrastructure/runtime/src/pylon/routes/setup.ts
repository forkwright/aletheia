/* eslint-disable no-console */
// Setup routes — pre-auth endpoints for initial onboarding wizard
import { Hono } from "hono";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { createLogger } from "../../koina/logger.js";
import { hashPassword } from "../../auth/passwords.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon.setup");

const configDir = (): string => process.env["ALETHEIA_CONFIG_DIR"] ?? join(homedir(), ".aletheia");
const setupFlagFile = (): string => join(configDir(), ".setup-complete");
const credentialFile = (): string => join(configDir(), "credentials", "anthropic.json");
const aletheiaConfigFile = (): string => join(configDir(), "aletheia.json");
const claudeJsonPath = (): string => join(homedir(), ".claude.json");

async function validateAnthropicKey(key: string): Promise<{ valid: boolean; error?: string }> {
  try {
    const res = await fetch("https://api.anthropic.com/v1/models", {
      headers: { "x-api-key": key, "anthropic-version": "2023-06-01" },
      signal: AbortSignal.timeout(10_000),
    });
    if (res.ok || res.status === 404) return { valid: true };
    if (res.status === 401) return { valid: false, error: "Invalid API key — authentication rejected" };
    return { valid: false, error: `Anthropic API returned ${res.status}` };
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    return { valid: false, error: `Could not reach Anthropic API: ${msg}` };
  }
}

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
    let credValue: string | undefined;
    let credType: "apiKey" | "token" = "apiKey";

    // Check request body first
    try {
      const body = await c.req.json() as Record<string, unknown>;
      const raw = typeof body["apiKey"] === "string" ? body["apiKey"].trim() : "";
      if (raw.length > 0) {
        credValue = raw;
        // OAuth tokens use a different field name in the credentials file
        credType = raw.startsWith("sk-ant-oat01-") ? "token" : "apiKey";
      }
    } catch { /* no body or not JSON */ }

    // Auto-detect from Claude Code's credential store
    if (!credValue) {
      try {
        const raw = JSON.parse(readFileSync(claudeJsonPath(), "utf-8")) as Record<string, unknown>;
        const pk = raw["primaryApiKey"];
        if (typeof pk === "string" && pk.length > 0) {
          credValue = pk;
          credType = "apiKey";
          log.info("Auto-detected API key from ~/.claude.json");
        } else {
          // OAuth-only Claude Code install — extract OAuth token if present
          const oauthToken = typeof raw["token"] === "string" ? raw["token"] : undefined;
          if (oauthToken) {
            credValue = oauthToken;
            credType = "token";
            log.info("Auto-detected OAuth token from ~/.claude.json");
          } else if (typeof raw["oauthAccount"] === "object") {
            return c.json({
              success: false,
              error: "Claude Code is using OAuth but no token is accessible. Get a separate API key at https://console.anthropic.com/keys",
            }, 400);
          }
        }
      } catch {
        log.debug("~/.claude.json not found or unreadable");
      }
    }

    if (!credValue) {
      return c.json({ success: false, error: "No API key found. Provide one manually or get one at https://console.anthropic.com/keys" }, 400);
    }

    // Validate key format
    if (!credValue.startsWith("sk-ant-") || credValue.length < 20) {
      return c.json({ success: false, error: "Invalid key format — expected sk-ant-... (full key required)" }, 400);
    }

    // Validate against Anthropic API (only for API keys, not OAuth tokens)
    if (credType === "apiKey") {
      const validation = await validateAnthropicKey(credValue);
      if (!validation.valid) {
        return c.json({ success: false, error: validation.error }, 400);
      }
    }

    try {
      const credDir = join(configDir(), "credentials");
      mkdirSync(credDir, { recursive: true });

      // Preserve existing backupCredentials/backupKeys if the file already exists
      let existing: Record<string, unknown> = {};
      try {
        existing = JSON.parse(readFileSync(credentialFile(), "utf-8")) as Record<string, unknown>;
      } catch { /* no existing file */ }

      writeFileSync(
        credentialFile(),
        JSON.stringify({ ...existing, [credType]: credValue }, null, 2),
        { mode: 0o600 },
      );
      return c.json({ success: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Failed to write credentials: ${msg}`);
      return c.json({ success: false, error: msg }, 500);
    }
  });

  app.post("/api/setup/account", async (c) => {
    try {
      const body = await c.req.json() as Record<string, unknown>;
      const username = typeof body["username"] === "string" ? body["username"].trim() : "";
      const password = typeof body["password"] === "string" ? body["password"] : "";

      if (!username || !password) {
        return c.json({ success: false, error: "username and password are required" }, 400);
      }
      if (username.length < 2) {
        return c.json({ success: false, error: "Username must be at least 2 characters" }, 400);
      }
      if (password.length < 8) {
        return c.json({ success: false, error: "Password must be at least 8 characters" }, 400);
      }

      const passwordHash = hashPassword(password);

      // Read and patch aletheia.json
      let cfg: Record<string, unknown> = {};
      try {
        cfg = JSON.parse(readFileSync(aletheiaConfigFile(), "utf-8")) as Record<string, unknown>;
      } catch { /* new install — start from empty */ }

      const gateway = (typeof cfg["gateway"] === "object" && cfg["gateway"] !== null
        ? cfg["gateway"]
        : {}) as Record<string, unknown>;

      gateway["auth"] = {
        mode: "session",
        users: [{ username, passwordHash, role: "admin" }],
        session: { secureCookies: false },
      };
      cfg["gateway"] = gateway;

      mkdirSync(configDir(), { recursive: true });
      writeFileSync(aletheiaConfigFile(), JSON.stringify(cfg, null, 2), { mode: 0o600 });
      log.info(`Account created: ${username}`);
      return c.json({ success: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Failed to create account: ${msg}`);
      return c.json({ success: false, error: msg }, 500);
    }
  });

  app.post("/api/setup/complete", (c) => {
    try {
      writeFileSync(setupFlagFile(), new Date().toISOString(), "utf-8");
      return c.json({ success: true });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Failed to write setup flag: ${msg}`);
      return c.json({ success: false, error: msg }, 500);
    }
  });

  return app;
}
