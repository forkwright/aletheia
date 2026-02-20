#!/usr/bin/env node
// CLI entry point
import { Command } from "commander";
import { startRuntime } from "./aletheia.js";
import { loadConfig } from "./taxis/loader.js";
import { createLogger } from "./koina/logger.js";
import { getVersion } from "./version.js";

const log = createLogger("entry");

process.on("unhandledRejection", (reason) => {
  log.error(`Unhandled rejection: ${reason instanceof Error ? reason.stack ?? reason.message : reason}`);
  process.exit(1);
});

process.on("uncaughtException", (err) => {
  log.error(`Uncaught exception: ${err.stack ?? err.message}`);
  process.exit(1);
});

const program = new Command()
  .name("aletheia")
  .description("Aletheia distributed cognition runtime")
  .version(getVersion());

const gateway = program
  .command("gateway")
  .description("Gateway management");

gateway
  .command("start")
  .description("Start the gateway")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { config?: string }) => {
    await startRuntime(opts.config);
  });

// Alias: "gateway run" → same as "gateway start" (systemd compat)
gateway
  .command("run")
  .description("Start the gateway (alias for start)")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { config?: string }) => {
    await startRuntime(opts.config);
  });

program
  .command("doctor")
  .description("Validate configuration")
  .option("-c, --config <path>", "Config file path")
  .action((opts: { config?: string }) => {
    try {
      const config = loadConfig(opts.config);
      console.log("Config valid.");
      console.log(`  Nous: ${config.agents.list.map((a) => a.id).join(", ")}`);
      console.log(`  Bindings: ${config.bindings.length}`);
      console.log(`  Gateway port: ${config.gateway.port}`);
      console.log(`  Signal accounts: ${Object.keys(config.channels.signal.accounts).length}`);
      console.log(`  Plugins: ${Object.keys(config.plugins.entries).length}`);
      console.log(`  Plugin paths: ${config.plugins.load.paths.length}`);
    } catch (error) {
      console.error(
        "Config invalid:",
        error instanceof Error ? error.message : error,
      );
      process.exit(1);
    }
  });

program
  .command("status")
  .description("System health check")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = {};
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/metrics`, { headers, signal: AbortSignal.timeout(5000) });
      if (!res.ok) {
        console.error(`Error: HTTP ${res.status}`);
        process.exit(1);
      }

      const data = await res.json() as Record<string, unknown>;
      const uptime = data["uptime"] as number;
      const hours = Math.floor(uptime / 3600);
      const mins = Math.floor((uptime % 3600) / 60);

      console.log(`Aletheia Status (${opts.url})`);
      console.log(`  Uptime: ${hours}h ${mins}m`);
      console.log();

      const nous = data["nous"] as Array<Record<string, unknown>>;
      if (nous?.length) {
        console.log("Agents:");
        for (const n of nous) {
          const tokens = n["tokens"] as Record<string, number> | null;
          const tokenInput = tokens?.["input"] as number | undefined;
          const inp = tokenInput !== null && tokenInput !== undefined ? `${Math.round(tokenInput / 1000)}k in` : "0k";
          console.log(`  ${n["name"]}: ${n["activeSessions"]} sessions, ${n["totalMessages"]} msgs, ${inp}`);
        }
        console.log();
      }

      const usage = data["usage"] as Record<string, unknown>;
      if (usage) {
        console.log(`Tokens: ${Math.round((usage["totalInputTokens"] as number) / 1000)}k in, ${Math.round((usage["totalOutputTokens"] as number) / 1000)}k out`);
        console.log(`Cache: ${usage["cacheHitRate"]}% hit rate, ${usage["turnCount"]} turns`);
      }

      const services = data["services"] as Array<Record<string, unknown>>;
      if (services?.length) {
        console.log();
        const allHealthy = services.every((s) => s["healthy"]);
        console.log(`Services: ${allHealthy ? "all healthy" : "DEGRADED"}`);
        for (const svc of services) {
          console.log(`  ${svc["healthy"] ? "+" : "X"} ${svc["name"]}`);
        }
      }

      const cron = data["cron"] as Array<Record<string, unknown>>;
      if (cron?.length) {
        console.log();
        console.log("Cron:");
        for (const job of cron) {
          console.log(`  ${job["id"]}: ${job["schedule"]} (next: ${job["nextRun"] ?? "unknown"})`);
        }
      }
    } catch (err) {
      console.error(`Failed to reach gateway: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

program
  .command("send")
  .description("Send a message to an agent")
  .requiredOption("-a, --agent <id>", "Agent ID")
  .requiredOption("-m, --message <text>", "Message text")
  .option("-s, --session <key>", "Session key", "cli")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { agent: string; message: string; session: string; url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/sessions/send`, {
        method: "POST",
        headers,
        body: JSON.stringify({ agentId: opts.agent, message: opts.message, sessionKey: opts.session }),
        signal: AbortSignal.timeout(120000),
      });

      const data = await res.json() as Record<string, unknown>;
      if (!res.ok) {
        console.error(`Error: ${data["error"] ?? res.statusText}`);
        process.exit(1);
      }

      console.log(data["response"]);
      const usage = data["usage"] as Record<string, number>;
      if (usage) {
        console.log(`\n--- ${usage["inputTokens"]} in / ${usage["outputTokens"]} out / ${data["toolCalls"]} tool calls ---`);
      }
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

program
  .command("sessions")
  .description("List sessions")
  .option("-a, --agent <id>", "Filter by agent ID")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { agent?: string; url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = {};
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const query = opts.agent ? `?nousId=${opts.agent}` : "";
      const res = await fetch(`${opts.url}/api/sessions${query}`, { headers, signal: AbortSignal.timeout(5000) });
      const data = await res.json() as { sessions: Array<Record<string, unknown>> };

      if (!data.sessions?.length) {
        console.log("No sessions found.");
        return;
      }

      console.log(`${data.sessions.length} sessions:`);
      for (const s of data.sessions.slice(0, 30)) {
        console.log(`  ${s["id"]} ${s["nousId"]} [${s["status"]}] ${s["messageCount"]} msgs (${s["updatedAt"]})`);
      }
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

const cronCmd = program.command("cron").description("Cron management");

cronCmd
  .command("list")
  .description("List cron jobs")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = {};
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/cron`, { headers, signal: AbortSignal.timeout(5000) });
      const data = await res.json() as { jobs: Array<Record<string, unknown>> };

      if (!data.jobs?.length) {
        console.log("No cron jobs configured.");
        return;
      }

      for (const job of data.jobs) {
        console.log(`  ${job["id"]}: ${job["schedule"]} agent=${job["agentId"]} last=${job["lastRun"] ?? "never"} next=${job["nextRun"] ?? "unknown"}`);
      }
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

cronCmd
  .command("trigger")
  .description("Manually trigger a cron job")
  .argument("<id>", "Cron job ID")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (id: string, opts: { url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/cron/${id}/trigger`, { method: "POST", headers, signal: AbortSignal.timeout(120000) });
      const data = await res.json() as Record<string, unknown>;

      if (!res.ok) {
        console.error(`Error: ${data["error"] ?? res.statusText}`);
        process.exit(1);
      }
      console.log(`Triggered cron job: ${id}`);
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

program
  .command("replay")
  .description("Replay a session's conversation history")
  .argument("<session-id>", "Session ID to replay")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .option("--live", "Re-send user messages through the API and compare outputs")
  .option("--stop-at <turn>", "Stop at turn N")
  .action(
    async (
      sessionId: string,
      opts: { url: string; token?: string; live?: boolean; stopAt?: string },
    ) => {
      try {
        const headers: Record<string, string> = {};
        if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

        const res = await fetch(
          `${opts.url}/api/sessions/${sessionId}/history?limit=500`,
          { headers, signal: AbortSignal.timeout(10000) },
        );
        if (!res.ok) {
          console.error(`Error: HTTP ${res.status}`);
          process.exit(1);
        }

        const data = (await res.json()) as {
          messages: Array<{
            seq: number;
            role: string;
            content: string;
            toolName: string | null;
            isDistilled: boolean;
          }>;
        };

        if (!data.messages?.length) {
          console.log("No messages in session.");
          return;
        }

        // Get session info
        const sessionRes = await fetch(`${opts.url}/api/sessions`, {
          headers,
          signal: AbortSignal.timeout(5000),
        });
        const sessionData = (await sessionRes.json()) as {
          sessions: Array<Record<string, unknown>>;
        };
        const session = sessionData.sessions?.find(
          (s) => s["id"] === sessionId,
        );
        const nousId = (session?.["nousId"] as string) ?? "unknown";
        const stopAt = opts.stopAt ? parseInt(opts.stopAt, 10) : undefined;

        console.log(
          `\n## Session ${sessionId} (agent: ${nousId}, ${data.messages.length} messages)\n`,
        );

        const userMessages: Array<{ seq: number; content: string }> = [];

        for (const msg of data.messages) {
          if (stopAt && msg.seq > stopAt) break;
          const distilled = msg.isDistilled ? " [distilled]" : "";
          const tool = msg.toolName ? ` [${msg.toolName}]` : "";
          const prefix = `[${msg.seq}] ${msg.role}${tool}${distilled}`;
          const content =
            msg.content.length > 500
              ? msg.content.slice(0, 500) + "..."
              : msg.content;
          console.log(`${prefix}:\n${content}\n`);

          if (msg.role === "user" && !msg.isDistilled) {
            userMessages.push({ seq: msg.seq, content: msg.content });
          }
        }

        // Live replay — re-send user messages and compare
        if (opts.live && userMessages.length > 0) {
          console.log(
            `\n## Live Replay (${userMessages.length} user messages → ${nousId})\n`,
          );
          const sendHeaders: Record<string, string> = {
            "Content-Type": "application/json",
            ...headers,
          };

          for (const um of userMessages) {
            if (stopAt && um.seq > stopAt) break;
            console.log(`--- Replaying turn ${um.seq} ---`);
            try {
              const replayRes = await fetch(
                `${opts.url}/api/sessions/send`,
                {
                  method: "POST",
                  headers: sendHeaders,
                  body: JSON.stringify({
                    agentId: nousId,
                    message: um.content,
                    sessionKey: `replay:${sessionId}`,
                  }),
                  signal: AbortSignal.timeout(120000),
                },
              );
              const replayData = (await replayRes.json()) as Record<
                string,
                unknown
              >;
              const response =
                (replayData["response"] as string) ?? "(no response)";
              console.log(
                `Replay response:\n${response.slice(0, 500)}\n`,
              );
            } catch (err) {
              console.error(
                `Replay failed at turn ${um.seq}: ${err instanceof Error ? err.message : err}`,
              );
              break;
            }
          }
        }
      } catch (err) {
        console.error(
          `Failed: ${err instanceof Error ? err.message : err}`,
        );
        process.exit(1);
      }
    },
  );

// --- Update ---

program
  .command("update [version]")
  .description("Update Aletheia to a release or latest main")
  .option("--edge", "Pull latest main (HEAD) instead of a release tag")
  .option("--check", "Check for updates without applying")
  .option("--rollback", "Roll back to previous version")
  .action(
    async (
      version: string | undefined,
      opts: { edge?: boolean; check?: boolean; rollback?: boolean },
    ) => {
      const { execFileSync } = await import("node:child_process");
      const { join, dirname } = await import("node:path");
      const { fileURLToPath } = await import("node:url");

      // Resolve aletheia-update script relative to the repo root
      const scriptDir = dirname(fileURLToPath(import.meta.url));
      const repoRoot = join(scriptDir, "..", "..");
      const script = join(repoRoot, "shared", "bin", "aletheia-update");

      const args: string[] = [];
      if (opts.edge) args.push("--edge");
      if (opts.check) args.push("--check");
      if (opts.rollback) args.push("--rollback");
      if (version) args.push(version);

      try {
        execFileSync(script, args, { stdio: "inherit" });
      } catch (err) {
        const code = (err as { status?: number }).status ?? 1;
        process.exit(code);
      }
    },
  );

// --- Auth Migration ---

program
  .command("migrate-auth")
  .description("Migrate from token auth to session-based auth")
  .option("-u, --username <name>", "Admin username")
  .option("-p, --password <pass>", "Admin password")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { username?: string; password?: string; config?: string }) => {
    const { readFileSync, writeFileSync } = await import("node:fs");
    const { paths } = await import("./taxis/paths.js");
    const { hashPassword } = await import("./auth/passwords.js");
    const { createInterface } = await import("node:readline");

    const configPath = opts.config ?? paths.configFile();
    let raw: string;
    try {
      raw = readFileSync(configPath, "utf-8");
    } catch {
      console.error(`Cannot read config: ${configPath}`);
      process.exit(1);
    }

    let config: Record<string, unknown>;
    try {
      config = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      console.error("Invalid JSON in config file");
      process.exit(1);
    }

    const gateway = (config["gateway"] ?? {}) as Record<string, unknown>;
    const auth = (gateway["auth"] ?? {}) as Record<string, unknown>;
    const currentMode = (auth["mode"] as string) ?? "token";

    if (currentMode === "session") {
      const users = auth["users"] as Array<unknown> | undefined;
      console.log(`Auth mode is already 'session' with ${users?.length ?? 0} user(s).`);
      process.exit(0);
    }

    console.log(`Current auth mode: ${currentMode}`);
    console.log("Migrating to session-based auth.\n");

    // Get username and password
    let username = opts.username;
    let password = opts.password;

    if (!username || !password) {
      const rl = createInterface({ input: process.stdin, output: process.stdout });
      const ask = (q: string): Promise<string> =>
        new Promise((resolve) => rl.question(q, resolve));

      if (!username) username = await ask("Username: ");
      if (!password) {
        process.stdout.write("Password: ");
        password = await ask("");
      }
      rl.close();
    }

    if (!username?.trim() || !password) {
      console.error("Username and password are required.");
      process.exit(1);
    }

    const passwordHash = hashPassword(password);

    // Build the new auth config
    auth["mode"] = "session";
    auth["users"] = [
      { username: username.trim(), passwordHash, role: "admin" },
    ];
    if (!auth["session"]) {
      auth["session"] = {
        accessTokenTtl: 900,
        refreshTokenTtl: 2592000,
        maxSessionsPerUser: 10,
        secureCookies: true,
      };
    }
    gateway["auth"] = auth;
    config["gateway"] = gateway;

    // Write config
    try {
      writeFileSync(configPath, JSON.stringify(config, null, 2) + "\n", "utf-8");
    } catch (err) {
      console.error(`Failed to write config: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }

    console.log(`\nAuth migrated to session mode.`);
    console.log(`  User: ${username.trim()} (admin)`);
    if (auth["token"]) {
      console.log(`  Old token preserved for API access.`);
    }
    console.log(`\nRestart the gateway to apply: systemctl restart aletheia`);
  });

program.parse();
