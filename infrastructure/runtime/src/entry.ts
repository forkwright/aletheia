#!/usr/bin/env node
/* eslint-disable no-console */
// CLI entry point
import { Command } from "commander";
import { startRuntime } from "./aletheia.js";
import { createLogger } from "./koina/logger.js";
import { getVersion } from "./version.js";
import { formatDoctorOutput, formatResults, runBootPersistenceChecks, runConnectivityChecks, runDependencyChecks, runDiagnostics } from "./koina/diagnostics.js";
import { readJson } from "./koina/fs.js";

const log = createLogger("entry");

const avgNums = (arr: number[]) => arr.reduce((s, v) => s + v, 0) / arr.length;
const fmtNum = (n: number) => n.toFixed(2);
const padEnd = (s: string, w: number) => s.padEnd(w);

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

// --- Init ---

program
  .command("init")
  .description("First-run setup wizard — configure credentials, create first agent")
  .action(async () => {
    const { existsSync, mkdirSync, readFileSync, writeFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const { randomBytes } = await import("node:crypto");
    const { paths } = await import("./taxis/paths.js");
    const { writeJson } = await import("./koina/fs.js");
    const { scaffoldAgent } = await import("./taxis/scaffold.js");
    const readline = await import("node:readline");

    const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
    const ask = (q: string): Promise<string> =>
      new Promise((resolve) => rl.question(q, resolve));

    try {
      const configPath = paths.configFile();

      let profileOnlyMode = false;
      if (existsSync(configPath)) {
        console.log(`\nConfig exists at ${configPath}.`);
        const answer = (await ask("  [A]ll fields, [P]rofile only, [C]ancel? ")).trim().toLowerCase();
        if (answer === "c" || answer === "") {
          console.log("Aborted.");
          return;
        }
        if (answer === "p") {
          profileOnlyMode = true;
        }
        // "a" or anything else = full re-run (fall through)
      }

      let apiKey = "";
      let port = 18789;
      let authMode: "none" | "token" | "session" = "none";
      let authBlock: Record<string, unknown> = { mode: "none" };
      let nousDir = "";
      let deployDir = "";

      if (!profileOnlyMode) {
        // API key — auto-detect from ~/.claude.json (same source as web wizard)
        const { homedir } = await import("node:os");
        const claudeJsonPath = join(homedir(), ".claude.json");
        let detectedKey = "";
        try {
          const raw = JSON.parse(readFileSync(claudeJsonPath, "utf-8")) as Record<string, unknown>;
          const pk = raw["primaryApiKey"];
          if (typeof pk === "string" && pk.length > 0) detectedKey = pk;
        } catch { /* not found or unreadable — proceed to manual entry */ }

        if (detectedKey) {
          const masked = `${detectedKey.slice(0, 12)}...`;
          const answer = (await ask(`Found API key in ~/.claude.json (${masked}) — use it? [Y/n] `)).trim().toLowerCase();
          apiKey = (answer === "" || answer === "y") ? detectedKey : (await ask("Anthropic API key (sk-ant-...): ")).trim();
        } else {
          apiKey = (await ask("Anthropic API key (sk-ant-...): ")).trim();
        }

        if (!apiKey) {
          console.error("API key is required.");
          return;
        }

        // Gateway port
        const portStr = (await ask("Gateway port [18789]: ")).trim();
        port = portStr ? parseInt(portStr, 10) : 18789;
        if (isNaN(port) || port < 1 || port > 65535) {
          console.error("Invalid port number.");
          return;
        }

        // Auth mode
        const authInput = (await ask("Auth mode — none (local only), token (API key), session (multi-user) [none]: ")).trim().toLowerCase();
        authMode = (authInput === "token" || authInput === "session") ? authInput : "none";
        authBlock = { mode: authMode };
        if (authMode === "token") {
          const token = randomBytes(24).toString("hex");
          authBlock["token"] = token;
          console.log(`\n  Auth token: ${token}`);
          console.log(`  Save this — you'll need it to access the UI and API.\n`);
        }

        // Anchor.json — bootstrap path configuration
        const { homedir: homedirFn } = await import("node:os");
        const { writeBootstrapAnchor, anchorPath } = await import("./taxis/bootstrap-loader.js");
        const anchorFilePath = anchorPath();
        const defaultNousDir = join(homedirFn(), ".aletheia", "nous");
        const defaultDeployDir = join(homedirFn(), ".aletheia", "deploy");
        nousDir = defaultNousDir;
        deployDir = defaultDeployDir;

        const existingAnchor = readJson(anchorFilePath);
        if (existingAnchor && typeof existingAnchor === "object") {
          const a = existingAnchor as { nousDir?: string; deployDir?: string };
          if (a.nousDir) {
            const keepNous = (await ask(`nous.dir is currently ${a.nousDir}. Keep it? [Y/n] `)).trim().toLowerCase();
            if (keepNous === "" || keepNous === "y") {
              nousDir = a.nousDir;
            } else {
              const input = (await ask(`nous.dir [${defaultNousDir}]: `)).trim();
              if (input) nousDir = input;
            }
          } else {
            const input = (await ask(`nous.dir [${defaultNousDir}]: `)).trim();
            if (input) nousDir = input;
          }
          if (a.deployDir) {
            const keepDeploy = (await ask(`deploy.dir is currently ${a.deployDir}. Keep it? [Y/n] `)).trim().toLowerCase();
            if (keepDeploy === "" || keepDeploy === "y") {
              deployDir = a.deployDir;
            } else {
              const input = (await ask(`deploy.dir [${defaultDeployDir}]: `)).trim();
              if (input) deployDir = input;
            }
          } else {
            const input = (await ask(`deploy.dir [${defaultDeployDir}]: `)).trim();
            if (input) deployDir = input;
          }
        } else {
          const nousInput = (await ask(`nous.dir [${defaultNousDir}]: `)).trim();
          if (nousInput) nousDir = nousInput;
          const deployInput = (await ask(`deploy.dir [${defaultDeployDir}]: `)).trim();
          if (deployInput) deployDir = deployInput;
        }

        mkdirSync(nousDir, { recursive: true });
        mkdirSync(deployDir, { recursive: true });
        writeBootstrapAnchor(nousDir, deployDir);
        console.log(`  Anchor: ${anchorFilePath}`);

        // Scaffold _shared/ workspace dirs — authoritative (throws on failure)
        const { scaffoldNousShared: doScaffold, mergeGitignore: doMerge } = await import("./taxis/nous-scaffold.js");
        const sharedCreated = doScaffold(nousDir);
        doMerge(nousDir);
        if (sharedCreated.length > 0) {
          console.log(`  Scaffold: ${sharedCreated.join(", ")}`);
        }
        // If sharedCreated.length === 0, all dirs already existed — report nothing (silence = nothing new)
      }

      // First agent
      const agentName = (await ask("First agent name (e.g. Atlas): ")).trim();
      if (!agentName) {
        console.error("Agent name is required.");
        return;
      }
      const agentId = agentName.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
      const agentEmoji = (await ask("Agent emoji [🤖]: ")).trim() || "🤖";

      // Profile step — collect name, timezone, optional role
      console.log("\nTell your agent about you (this personalizes how it works with you):");
      const userName = (await ask("  Your name: ")).trim();
      const detectedTz = Intl.DateTimeFormat().resolvedOptions().timeZone;
      const tzInput = (await ask(`  Timezone [${detectedTz}]: `)).trim();
      const userTimezone = tzInput || detectedTz;
      const userRole = (await ask("  Role/title (optional, press Enter to skip): ")).trim();

      const userProfile: import("./taxis/scaffold.js").UserProfile = {
        name: userName || agentName, // fallback to agent name if user skips
        role: userRole || "user",
        style: "balanced", // communication style not asked upfront — emerges through use
        timezone: userTimezone,
      };

      if (!profileOnlyMode) {
        // Write credentials
        const credDir = join(paths.configDir(), "credentials");
        mkdirSync(credDir, { recursive: true });
        const credPath = join(credDir, "anthropic.json");
        writeFileSync(credPath, JSON.stringify({ apiKey }, null, 2) + "\n", { mode: 0o600 });

        // Write base config (scaffoldAgent will append agent + binding)
        mkdirSync(paths.configDir(), { recursive: true });
        writeJson(configPath, {
          agents: { defaults: {}, list: [] },
          bindings: [],
          gateway: { port, auth: authBlock },
        });
      }

      // Scaffold agent — resolve nousDir from anchor.json or use value set in anchor step above
      let scaffoldNousDir: string;
      if (profileOnlyMode) {
        const { anchorPath: getAnchorPath } = await import("./taxis/bootstrap-loader.js");
        const { homedir: homedirFn2 } = await import("node:os");
        const existingAnchorForProfile = readJson(getAnchorPath()) as { nousDir?: string } | null;
        scaffoldNousDir = existingAnchorForProfile?.nousDir ?? join(homedirFn2(), ".aletheia", "nous");
      } else {
        scaffoldNousDir = nousDir;
      }
      mkdirSync(scaffoldNousDir, { recursive: true });
      const templateDir = join(scaffoldNousDir, "_example");
      if (!existsSync(templateDir)) {
        mkdirSync(templateDir, { recursive: true });
      }

      const result = scaffoldAgent({
        id: agentId,
        name: agentName,
        emoji: agentEmoji,
        nousDir: scaffoldNousDir,
        configPath,
        templateDir,
        userProfile,
      });

      console.log(`\nSetup complete.`);
      if (!profileOnlyMode) {
        const authLabel = authMode === "none"
          ? "none (no token required)"
          : authMode === "token"
            ? "token (saved to config)"
            : "session (configure users with migrate-auth)";
        console.log(`  Config:  ${configPath}`);
        console.log(`  Auth:    ${authLabel}`);
      }
      console.log(`  Agent:   ${agentName} (${agentId}) → ${result.workspace}`);
      if (userName) console.log(`  Profile: ${userName}${userRole ? ` — ${userRole}` : ""} (${userTimezone})`);
      console.log(`\nNext step:`);
      console.log(`  aletheia start`);
    } finally {
      rl.close();
    }
  });

// --- Gateway ---

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
  .command("refresh-token")
  .description("Check OAuth token expiry and refresh if needed")
  .action(async () => {
    const { readCredentials, isTokenExpired, refreshOAuthToken } = await import("./hermeneus/oauth-refresh.js");
    const creds = readCredentials();
    if (!creds) {
      console.log("❌ No OAuth credentials found (not using OAuth authentication)");
      process.exit(0);
    }

    const remaining = creds.expiresAt - Date.now();
    const hoursLeft = (remaining / 3_600_000).toFixed(1);

    if (!isTokenExpired(creds.expiresAt)) {
      console.log(`✅ Token valid — ${hoursLeft}h remaining (expires ${new Date(creds.expiresAt).toISOString()})`);
      process.exit(0);
    }

    console.log(`⚠️  Token expired or expiring soon (${hoursLeft}h) — attempting refresh...`);
    const result = await refreshOAuthToken();
    if (result.success) {
      console.log(`✅ Token refreshed — new expiry: ${new Date(result.newExpiresAt!).toISOString()}`);
    } else {
      console.log(`❌ Refresh failed: ${result.error}`);
      process.exit(1);
    }
  });

program
  .command("doctor")
  .description("Check system health — connectivity, dependencies, and boot persistence")
  .action(async () => {
    const [connectivity, bootPersistence] = await Promise.all([
      runConnectivityChecks(),
      Promise.resolve(runBootPersistenceChecks()),
    ]);
    const dependencies = runDependencyChecks();

    // Surface bootstrap_anchor check from runDiagnostics (re-reads anchor.json each run)
    let anchorSection = "";
    try {
      const { results: diagResults } = runDiagnostics();
      const anchorResult = diagResults.find((r) => r.name === "bootstrap_anchor");
      if (anchorResult) {
        anchorSection = `\n── Configuration ──\n${formatResults([anchorResult])}\n`;
      }
    } catch { /* runDiagnostics errors are non-fatal for doctor output */ }

    console.log(anchorSection + formatDoctorOutput(connectivity, dependencies, bootPersistence));
    // Exit 0 always — doctor is informational, not assertion-based
  });

program
  .command("status")
  .description("System health check")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = {};
      // Auto-read token from config if not provided
      let token = opts.token;
      if (!token) {
        try {
          const { loadConfig } = await import("./taxis/loader.js");
          const cfg = loadConfig();
          const raw = cfg.gateway?.auth?.token;
          if (typeof raw === "string") token = raw;
        } catch { /* config unavailable */ }
      }
      if (token) headers["Authorization"] = `Bearer ${token}`;

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
    } catch (error) {
      console.error(`Failed to reach gateway: ${error instanceof Error ? error.message : error}`);
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
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }
  });

program
  .command("plan")
  .description("Start or resume a Dianoia planning project")
  .option("-a, --agent <id>", "Agent ID to plan for")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { agent?: string; url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/sessions/send`, {
        method: "POST",
        headers,
        body: JSON.stringify({
          agentId: opts.agent ?? "syn",
          message: "/plan",
          sessionKey: "cli:plan",
        }),
        signal: AbortSignal.timeout(120000),
      });

      const data = await res.json() as Record<string, unknown>;
      if (!res.ok) {
        console.error(`Error: ${(data["error"] as string | undefined) ?? res.statusText}`);
        process.exit(1);
      }
      console.log((data["response"] as string | undefined) ?? "(no response)");
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : String(error)}`);
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
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
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
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
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
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
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
            } catch (error) {
              console.error(
                `Replay failed at turn ${um.seq}: ${error instanceof Error ? error.message : error}`,
              );
              break;
            }
          }
        }
      } catch (error) {
        console.error(
          `Failed: ${error instanceof Error ? error.message : error}`,
        );
        process.exit(1);
      }
    },
  );

// --- Session Forking ---

program
  .command("fork")
  .description("Fork a session from a historical distillation checkpoint")
  .argument("<session-id>", "Session ID to fork")
  .requiredOption("--at <number>", "Distillation checkpoint number to fork from")
  .action(async (sessionId: string, opts: { at: string }) => {
    const distillationNumber = parseInt(opts.at, 10);
    if (isNaN(distillationNumber) || distillationNumber < 1) {
      console.error("--at must be a positive integer (distillation number)");
      process.exit(1);
    }

    const { SessionStore } = await import("./mneme/store.js");
    const { paths } = await import("./taxis/paths.js");
    const store = new SessionStore(paths.sessionsDb());
    try {
      const result = store.forkSession(sessionId, distillationNumber);
      console.log(`Forked session: ${result.newSessionId}`);
      console.log(`  Source: ${sessionId} @ distillation #${distillationNumber}`);
      console.log(`  Messages copied: ${result.messagesCopied}`);
    } catch (error) {
      console.error(`Fork failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    } finally {
      store.close();
    }
  });

// --- Agent Management ---

const agentCmd = program.command("agent").description("Agent management");

agentCmd
  .command("create")
  .description("Scaffold a new agent workspace with onboarding")
  .option("--id <id>", "Agent ID (lowercase, alphanumeric, hyphens)")
  .option("--name <name>", "Agent display name")
  .option("--emoji <emoji>", "Agent emoji")
  .action(async (opts: { id?: string; name?: string; emoji?: string }) => {
    const { paths } = await import("./taxis/paths.js");
    const { scaffoldAgent, validateAgentId } = await import("./taxis/scaffold.js");
    const { join } = await import("node:path");

    let id = opts.id;
    let name = opts.name;

    if (!id || !name) {
      const readline = await import("node:readline");
      const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
      const ask = (q: string): Promise<string> =>
        new Promise((resolve) => rl.question(q, resolve));

      if (!id) {
        id = (await ask("Agent ID (e.g. atlas): ")).trim();
        const check = validateAgentId(id);
        if (!check.valid) {
          console.error(`Invalid ID: ${check.reason}`);
          rl.close();
          process.exit(1);
        }
      }
      if (!name) {
        name = (await ask("Display name: ")).trim();
        if (!name) { name = id.charAt(0).toUpperCase() + id.slice(1); }
      }
      rl.close();
    }

    try {
      const scaffoldOpts = {
        id,
        name,
        nousDir: paths.nous,
        configPath: paths.configFile(),
        templateDir: join(paths.nous, "_example"),
        ...(opts.emoji ? { emoji: opts.emoji } : {}),
      };
      const result = scaffoldAgent(scaffoldOpts);
      console.log(`Created agent "${name}" (${id})`);
      console.log(`  Workspace: ${result.workspace}`);
      console.log(`  Files: ${result.filesCreated.join(", ")}`);
      console.log(`\nStart onboarding: open the web UI and select ${name}.`);
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }
  });

// --- Plugins ---

const pluginsCmd = program.command("plugins").description("Plugin management");

pluginsCmd
  .command("list")
  .description("List discovered and configured plugins")
  .action(async () => {
    const { existsSync } = await import("node:fs");
    const { join } = await import("node:path");
    const { paths } = await import("./taxis/paths.js");
    const { loadConfig } = await import("./taxis/loader.js");

    const config = loadConfig();
    const rootDir = paths.pluginRoot;

    console.log(`Plugin root: ${rootDir}\n`);

    // Discover plugins from root directory
    const discovered: Array<{ id: string; version: string; path: string; source: string }> = [];

    if (existsSync(rootDir)) {
      const { discoverPlugins } = await import("./prostheke/loader.js");
      const found = await discoverPlugins(rootDir);
      for (const p of found) {
        discovered.push({ id: p.manifest.id, version: p.manifest.version, path: rootDir, source: "discovered" });
      }
    }

    // Configured explicit paths
    for (const p of config.plugins.load.paths) {
      const exists = existsSync(p);
      if (exists) {
        const manifestPath = join(p, "manifest.json");
        const manifest = readJson(manifestPath) as { id?: string; version?: string } | null;
        if (manifest?.id) {
          discovered.push({ id: manifest.id, version: manifest.version ?? "?", path: p, source: "config" });
        }
      } else {
        discovered.push({ id: "(missing)", version: "-", path: p, source: "config" });
      }
    }

    if (discovered.length === 0) {
      console.log("No plugins found.");
      return;
    }

    for (const p of discovered) {
      const enabled = config.plugins.entries[p.id]?.enabled !== false;
      const status = p.id === "(missing)" ? "MISSING" : enabled ? "enabled" : "disabled";
      console.log(`  ${p.id} v${p.version} [${status}] (${p.source}: ${p.path})`);
    }
  });

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
      } catch (error) {
        const code = (error as { status?: number }).status ?? 1;
        process.exit(code);
      }
    },
  );

// --- Import ---

program
  .command("import")
  .description("Import an agent from an AgentFile JSON export")
  .argument("<file>", "Path to .agent.json file")
  .option("--nous-id <id>", "Override agent ID (for cloning)")
  .option("--skip-sessions", "Skip session/message import")
  .option("--skip-workspace", "Skip workspace file restoration")
  .action(async (file: string, opts: { nousId?: string; skipSessions?: boolean; skipWorkspace?: boolean }) => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const { SessionStore } = await import("./mneme/store.js");
    const { paths } = await import("./taxis/paths.js");
    const { importAgent } = await import("./portability/import.js");

    const filePath = resolve(file);
    let raw: string;
    try {
      raw = readFileSync(filePath, "utf-8");
    } catch (error) {
      console.error(`Cannot read file: ${filePath}`);
      console.error(error instanceof Error ? error.message : error);
      process.exit(1);
    }

    let agentFile: import("./portability/export.js").AgentFile;
    try {
      agentFile = JSON.parse(raw);
    } catch { /* invalid JSON in agent file */
      console.error("Invalid JSON in agent file");
      process.exit(1);
    }

    const store = new SessionStore(paths.sessionsDb());
    try {
      console.log(`Importing agent from: ${filePath}`);
      const importOpts: import("./portability/import.js").ImportOptions = {};
      if (opts.nousId) importOpts.targetNousId = opts.nousId;
      if (opts.skipSessions) importOpts.skipSessions = opts.skipSessions;
      if (opts.skipWorkspace) importOpts.skipWorkspace = opts.skipWorkspace;
      const result = await importAgent(agentFile, store, importOpts);

      console.log(`\nImport complete:`);
      console.log(`  Agent: ${result.nousId}`);
      console.log(`  Files: ${result.filesRestored}`);
      console.log(`  Sessions: ${result.sessionsImported}`);
      console.log(`  Messages: ${result.messagesImported}`);
      console.log(`  Notes: ${result.notesImported}`);
    } finally {
      store.close();
    }
  });

// --- Token Audit ---

program
  .command("audit-tokens [agent-id]")
  .description("Show per-section bootstrap token breakdown for an agent")
  .action(async (agentId: string | undefined) => {
    const { auditTokens } = await import("./nous/audit.js");
    const id = agentId ?? "syn";
    await auditTokens(id);
  });

// --- Audit Chain ---

const auditCmd = program.command("audit").description("Audit log management");

auditCmd
  .command("verify")
  .description("Verify audit trail hash chain integrity")
  .action(async () => {
    const Database = (await import("better-sqlite3")).default;
    const { paths } = await import("./taxis/paths.js");
    const { verifyAuditChain } = await import("./symbolon/audit-verify.js");

    const dbPath = paths.sessionsDb();
    let db: InstanceType<typeof Database>;
    try {
      db = new Database(dbPath, { readonly: true });
    } catch (error) {
      console.error(`Cannot open database: ${dbPath}`);
      console.error(error instanceof Error ? error.message : error);
      process.exit(1);
    }

    try {
      const result = verifyAuditChain(db);

      if (result.totalEntries === 0) {
        console.log("Audit log is empty — nothing to verify.");
        process.exit(0);
      }

      console.log(`Audit chain verification:`);
      console.log(`  Entries: ${result.totalEntries} total, ${result.checkedEntries} with checksums`);
      console.log(`  Range: ${result.firstEntry} → ${result.lastEntry}`);

      if (result.valid) {
        console.log(`  Status: VALID`);
        process.exit(0);
      } else {
        console.log(`  Status: TAMPERED`);
        console.log(`  Detail: ${result.tamperDetails}`);
        process.exit(1);
      }
    } finally {
      db.close();
    }
  });

// --- Memory ---

const memoryCmd = program.command("memory").description("Memory management and diagnostics");

memoryCmd
  .command("audit")
  .description("Run recall precision/recall audit against ground-truth corpus")
  .option("-u, --url <url>", "Sidecar URL", "http://localhost:8230")
  .option("-c, --corpus <path>", "Corpus JSONL path", "~/.aletheia/corpus/recall.jsonl")
  .option("--save-baseline", "Save current scores as new baseline")
  .option("--agent <id>", "Filter to a specific agent")
  .action(async (opts: { url: string; corpus: string; saveBaseline?: boolean; agent?: string }) => {
    const { readFileSync, writeFileSync, existsSync, mkdirSync } = await import("node:fs");
    const { join, dirname } = await import("node:path");

    // Resolve corpus path (expand ~)
    const home = process.env["HOME"] ?? "/root";
    const corpusPath = opts.corpus.replace(/^~/, home);
    const baselinePath = join(home, ".aletheia", "corpus", "recall-baseline.json");

    if (!existsSync(corpusPath)) {
      console.log(`No corpus file found at ${corpusPath}. Create a JSONL file with {query, expected_ids, domain} entries.`);
      return;
    }

    // Load corpus
    type CorpusEntry = { query: string; expected_ids: string[]; domain: string };
    let entries: CorpusEntry[];
    try {
      const raw = readFileSync(corpusPath, "utf-8");
      entries = raw
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0)
        .map((line) => JSON.parse(line) as CorpusEntry);
    } catch (error) {
      console.error(`Failed to parse corpus file: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }

    // Filter by agent if requested
    if (opts.agent) {
      entries = entries.filter((e) => e.domain === opts.agent);
      if (entries.length === 0) {
        console.log(`No corpus entries found for agent: ${opts.agent}`);
        return;
      }
    }

    // Run queries against sidecar /search
    type EntryResult = { domain: string; precision: number; recall: number; f1: number };
    const results: EntryResult[] = [];

    for (const entry of entries) {
      let returnedIds: string[] = [];
      try {
        const res = await fetch(`${opts.url}/search`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: entry.query, user_id: "aletheia", agent_id: entry.domain, top_k: 20 }),
          signal: AbortSignal.timeout(15_000),
        });
        if (res.ok) {
          const data = await res.json() as { results?: Array<{ id?: string }> };
          returnedIds = (data.results ?? []).map((r) => r.id ?? "").filter((id) => id.length > 0);
        }
      } catch { /* query failed — empty results */ }

      const expectedSet = new Set(entry.expected_ids);
      const returnedSet = new Set(returnedIds);
      const intersection = returnedIds.filter((id) => expectedSet.has(id));

      const precision = returnedSet.size === 0 ? 1 : intersection.length / returnedSet.size;
      const recall = expectedSet.size === 0 ? 1 : intersection.length / expectedSet.size;
      const f1 = precision + recall === 0 ? 0 : (2 * precision * recall) / (precision + recall);

      results.push({ domain: entry.domain, precision, recall, f1 });
    }

    // Aggregate per-domain
    const domainMap = new Map<string, { precision: number[]; recall: number[]; f1: number[] }>();
    for (const r of results) {
      const existing = domainMap.get(r.domain) ?? { precision: [], recall: [], f1: [] };
      existing.precision.push(r.precision);
      existing.recall.push(r.recall);
      existing.f1.push(r.f1);
      domainMap.set(r.domain, existing);
    }

    type DomainSummary = { precision: number; recall: number; f1: number; query_count: number };
    const byDomain: Record<string, DomainSummary> = {};
    for (const [domain, vals] of domainMap) {
      byDomain[domain] = {
        precision: avgNums(vals.precision),
        recall: avgNums(vals.recall),
        f1: avgNums(vals.f1),
        query_count: vals.precision.length,
      };
    }

    const overallPrecision = avgNums(results.map((r) => r.precision));
    const overallRecall = avgNums(results.map((r) => r.recall));
    const overallF1 = avgNums(results.map((r) => r.f1));

    // Baseline comparison
    let baselineMsg = "";
    let hasRegression = false;
    if (existsSync(baselinePath) && !opts.saveBaseline) {
      try {
        const baseline = JSON.parse(readFileSync(baselinePath, "utf-8")) as {
          overall: { precision: number; recall: number; f1: number };
        };
        const precDiff = overallPrecision - baseline.overall.precision;
        const recDiff = overallRecall - baseline.overall.recall;
        const THRESHOLD = 0.05;
        const precStatus = precDiff < -THRESHOLD ? "REGRESSION" : "within threshold";
        const recStatus = recDiff < -THRESHOLD ? "REGRESSION" : "within threshold";
        if (precDiff < -THRESHOLD || recDiff < -THRESHOLD) {
          hasRegression = true;
        }
        const precSign = precDiff >= 0 ? "+" : "";
        const recSign = recDiff >= 0 ? "+" : "";
        baselineMsg = `  Baseline: precision ${precSign}${fmtNum(precDiff)} (${precStatus}), recall ${recSign}${fmtNum(recDiff)} (${recStatus})`;
      } catch { /* baseline parse failed — skip comparison */ }
    }

    // Save baseline if requested
    if (opts.saveBaseline) {
      const baselineData = {
        timestamp: new Date().toISOString(),
        overall: { precision: overallPrecision, recall: overallRecall, f1: overallF1 },
        by_domain: byDomain,
      };
      mkdirSync(dirname(baselinePath), { recursive: true });
      writeFileSync(baselinePath, JSON.stringify(baselineData, null, 2) + "\n", "utf-8");
      console.log(`Baseline saved to ${baselinePath}`);
    }

    // Print results table
    const domains = Object.keys(byDomain).toSorted();
    const colW = Math.max(8, ...domains.map((d) => d.length));
    const sep = "─".repeat(colW);

    console.log(`\nRecall Audit — ${results.length} queries, ${domains.length} domains\n`);
    console.log(`  ${padEnd("Domain", colW)}  Queries  Precision  Recall   F1`);
    console.log(`  ${sep}  ───────  ─────────  ──────   ──`);
    for (const domain of domains) {
      const d = byDomain[domain]!;
      console.log(`  ${padEnd(domain, colW)}  ${String(d.query_count).padStart(7)}  ${fmtNum(d.precision).padStart(9)}  ${fmtNum(d.recall).padStart(6)}   ${fmtNum(d.f1)}`);
    }
    console.log(`  ${sep}  ───────  ─────────  ──────   ──`);
    console.log(`  ${padEnd("OVERALL", colW)}  ${String(results.length).padStart(7)}  ${fmtNum(overallPrecision).padStart(9)}  ${fmtNum(overallRecall).padStart(6)}   ${fmtNum(overallF1)}`);

    if (baselineMsg) {
      console.log(`\n${baselineMsg}`);
    }
    if (hasRegression) {
      console.log(`\n  REGRESSION DETECTED — precision or recall dropped more than 5% from baseline`);
      process.exit(1);
    }
  });

memoryCmd
  .command("health")
  .description("Check memory system health against configured thresholds")
  .option("-u, --url <url>", "Sidecar URL", "http://localhost:8230")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { url: string; config?: string }) => {
    const { paths } = await import("./taxis/paths.js");
    const { AletheiaConfigSchema } = await import("./taxis/schema.js");
    const { eventBus } = await import("./koina/event-bus.js");

    const configPath = opts.config ?? paths.configFile();
    let thresholds = {
      noiseRateMax: 0.05,
      orphanCountMax: 50,
      relatesToRateMax: 0.30,
      recallLatencyP95Ms: 1000,
      flushSuccessRateMin: 0.95,
    };

    try {
      const raw = await readJson(configPath);
      const config = AletheiaConfigSchema.safeParse(raw);
      if (config.success) {
        thresholds = { ...thresholds, ...config.data.memoryHealth };
      }
    } catch { /* config unreadable — use defaults */ }

    let healthData: Record<string, unknown>;
    try {
      const thresholdsParam = encodeURIComponent(JSON.stringify(thresholds));
      const res = await fetch(`${opts.url}/health?thresholds=${thresholdsParam}`, {
        signal: AbortSignal.timeout(10_000),
      });
      if (!res.ok) {
        console.error(`Sidecar returned ${res.status}: ${res.statusText}`);
        process.exit(1);
      }
      healthData = await res.json() as Record<string, unknown>;
    } catch (error) {
      console.error(`Cannot reach sidecar at ${opts.url}: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }

    const status = String(healthData["status"] ?? "unknown").toUpperCase();
    const qdrant = (healthData["qdrant"] ?? {}) as Record<string, unknown>;
    const neo4j = (healthData["neo4j"] ?? {}) as Record<string, unknown>;
    const recall = (healthData["recall"] ?? {}) as Record<string, unknown>;
    const thresholdInfo = (healthData["thresholds"] ?? {}) as Record<string, unknown>;
    const exceeded = (thresholdInfo["exceeded"] ?? []) as string[];

    const colW = 22;

    console.log(`\nMemory Health: ${status}\n`);
    console.log(`  ${padEnd("Metric", colW)}  ${padEnd("Value", 10)}  ${padEnd("Threshold", 10)}  Status`);
    console.log(`  ${padEnd("──────", colW)}  ${padEnd("─────", 10)}  ${padEnd("─────────", 10)}  ──────`);

    const noiseRate = recall["noise_rate"] !== null && recall["noise_rate"] !== undefined ? `${(Number(recall["noise_rate"]) * 100).toFixed(1)}%` : "N/A";
    const noiseStatus = exceeded.includes("noise_rate") ? "EXCEEDED" : (recall["noise_rate"] !== null && recall["noise_rate"] !== undefined ? "OK" : "N/A");
    console.log(`  ${padEnd("Noise rate", colW)}  ${padEnd(noiseRate, 10)}  ${padEnd(`< ${(thresholds.noiseRateMax * 100).toFixed(1)}%`, 10)}  ${noiseStatus}`);

    const orphanCount = qdrant["orphan_count"] !== null && qdrant["orphan_count"] !== undefined ? String(qdrant["orphan_count"]) : "N/A";
    const orphanStatus = exceeded.includes("orphan_count") ? "EXCEEDED" : (qdrant["orphan_count"] !== null && qdrant["orphan_count"] !== undefined ? "OK" : "N/A");
    console.log(`  ${padEnd("Orphan count", colW)}  ${padEnd(orphanCount, 10)}  ${padEnd(`< ${thresholds.orphanCountMax}`, 10)}  ${orphanStatus}`);

    const relatesToRate = neo4j["relates_to_rate"] !== null && neo4j["relates_to_rate"] !== undefined ? `${(Number(neo4j["relates_to_rate"]) * 100).toFixed(1)}%` : "N/A";
    const relatesToStatus = exceeded.includes("relates_to_rate") ? "EXCEEDED" : (neo4j["relates_to_rate"] !== null && neo4j["relates_to_rate"] !== undefined ? "OK" : "N/A");
    console.log(`  ${padEnd("RELATES_TO rate", colW)}  ${padEnd(relatesToRate, 10)}  ${padEnd(`< ${(thresholds.relatesToRateMax * 100).toFixed(1)}%`, 10)}  ${relatesToStatus}`);

    const latencyP95 = recall["latency_p95_ms"] !== null && recall["latency_p95_ms"] !== undefined ? `${Math.round(Number(recall["latency_p95_ms"]))}ms` : "N/A";
    const latencyStatus = exceeded.includes("recall_latency_p95_ms") ? "EXCEEDED" : (recall["latency_p95_ms"] !== null && recall["latency_p95_ms"] !== undefined ? "OK" : "N/A");
    console.log(`  ${padEnd("Recall P95", colW)}  ${padEnd(latencyP95, 10)}  ${padEnd(`< ${thresholds.recallLatencyP95Ms}ms`, 10)}  ${latencyStatus}`);

    const flushData = (healthData["flush"] ?? null) as Record<string, unknown> | null;
    const flushRate = flushData !== null && flushData["success_rate"] !== undefined
      ? `${(Number(flushData["success_rate"]) * 100).toFixed(1)}%`
      : "N/A";
    const flushLabel = flushData !== null && flushData["sample_count"] !== undefined
      ? `Flush success (24h, n=${flushData["sample_count"]})`
      : "Flush success rate";
    const flushStatus = exceeded.includes("flush_success_rate")
      ? "EXCEEDED"
      : (flushData !== null && flushData["success_rate"] !== undefined ? "OK" : "N/A");
    console.log(`  ${padEnd(flushLabel, colW)}  ${padEnd(flushRate, 10)}  ${padEnd(`> ${(thresholds.flushSuccessRateMin * 100).toFixed(1)}%`, 10)}  ${flushStatus}`);

    if (exceeded.length > 0) {
      const values = (thresholdInfo["values"] ?? {}) as Record<string, number>;
      eventBus.emit("memory:health_degraded", {
        metrics: exceeded,
        values: values as Record<string, unknown>,
        status: healthData["status"] as string,
      });
    } else {
      eventBus.emit("memory:health_recovered", {
        metrics: [],
        status: healthData["status"] as string,
      });
    }

    if (status === "DEGRADED" || status === "CRITICAL") {
      process.exit(1);
    }
  });

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
    const { hashPassword } = await import("./symbolon/passwords.js");
    const { createInterface } = await import("node:readline");

    const configPath = opts.config ?? paths.configFile();
    let raw: string;
    try {
      raw = readFileSync(configPath, "utf-8");
    } catch { /* config file unreadable */
      console.error(`Cannot read config: ${configPath}`);
      process.exit(1);
    }

    let config: Record<string, unknown>;
    try {
      config = JSON.parse(raw) as Record<string, unknown>;
    } catch { /* invalid JSON in config */
      console.error("Invalid JSON in config file");
      process.exit(1);
    }

    const gatewayConfig = (config["gateway"] ?? {}) as Record<string, unknown>;
    const auth = (gatewayConfig["auth"] ?? {}) as Record<string, unknown>;
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
    gatewayConfig["auth"] = auth;
    config["gateway"] = gatewayConfig;

    // Write config
    try {
      writeFileSync(configPath, JSON.stringify(config, null, 2) + "\n", "utf-8");
    } catch (error) {
      console.error(`Failed to write config: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }

    console.log(`\nAuth migrated to session mode.`);
    console.log(`  User: ${username.trim()} (admin)`);
    if (auth["token"]) {
      console.log(`  Old token preserved for API access.`);
    }
    console.log(`\nRestart the gateway to apply: systemctl restart aletheia`);
  });

// --- Channel management (Agora) ---
const channelCmd = program.command("channel").description("Channel management (Agora)");

channelCmd
  .command("list")
  .description("Show configured channels and their status")
  .action(async () => {
    const { channelList } = await import("./agora/cli.js");
    channelList();
  });

channelCmd
  .command("add <channel>")
  .description("Add and configure a channel (e.g., slack)")
  .action(async (channel: string) => {
    const { isSupportedChannel, channelAddSlack, listSupportedChannels } = await import("./agora/cli.js");
    if (!isSupportedChannel(channel)) {
      console.error(`Unknown channel: "${channel}". Supported: ${listSupportedChannels().join(", ")}`);
      process.exit(1);
    }
    if (channel === "slack") {
      await channelAddSlack();
    }
  });

channelCmd
  .command("remove <channel>")
  .description("Remove a channel configuration")
  .action(async (channel: string) => {
    const { channelRemove } = await import("./agora/cli.js");
    channelRemove(channel);
  });

// --- Plan (Dianoia) ---
const planCmd = program.command("plan").description("Dianoia planning projects");

async function planApi(path: string, opts: { url: string; token?: string }, method = "GET", body?: unknown): Promise<unknown> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  let token = opts.token;
  if (!token) {
    try {
      const { loadConfig: lc } = await import("./taxis/loader.js");
      const cfg = lc();
      const raw = cfg.gateway?.auth?.token;
      if (typeof raw === "string") token = raw;
    } catch { /* config unavailable */ }
  }
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(`${opts.url}${path}`, {
    method,
    headers,
    ...(body ? { body: JSON.stringify(body) } : {}),
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`HTTP ${res.status}: ${text}`);
  }
  return res.json();
}

planCmd
  .command("list")
  .description("List all planning projects")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { url: string; token?: string }) => {
    try {
      const data = await planApi("/api/planning/projects", opts) as { projects: Array<Record<string, unknown>> };
      const projects = data.projects ?? [];
      if (projects.length === 0) {
        console.log("No planning projects.");
        return;
      }
      console.log(`${"ID".padEnd(30)}  ${"State".padEnd(12)}  Goal`);
      console.log(`${"─".repeat(30)}  ${"─".repeat(12)}  ${"─".repeat(40)}`);
      for (const p of projects) {
        const id = String(p["id"] ?? "").slice(0, 28);
        const state = String(p["state"] ?? "unknown").padEnd(12);
        const goal = String(p["goal"] ?? "").slice(0, 60);
        console.log(`${id.padEnd(30)}  ${state}  ${goal}`);
      }
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }
  });

planCmd
  .command("show <id>")
  .description("Show project details and roadmap")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (id: string, opts: { url: string; token?: string }) => {
    try {
      const data = await planApi(`/api/planning/projects/${id}`, opts) as { project: Record<string, unknown> };
      const p = data.project;
      if (!p) { console.error("Project not found."); process.exit(1); }

      console.log(`Project: ${p["goal"]}`);
      console.log(`  ID:    ${p["id"]}`);
      console.log(`  State: ${p["state"]}`);
      if (p["constraints"]) console.log(`  Scope: ${p["constraints"]}`);
      console.log();

      // Phases
      const phases = (p["phases"] ?? []) as Array<Record<string, unknown>>;
      if (phases.length > 0) {
        console.log("Phases:");
        for (const ph of phases) {
          const icon = ph["state"] === "complete" ? "✅" : ph["state"] === "executing" ? "🔄" : "⬜";
          const reqs = (ph["requirements"] ?? []) as string[];
          console.log(`  ${icon} ${ph["name"]}${reqs.length > 0 ? ` (${reqs.length} reqs)` : ""}`);
          if (ph["goal"]) console.log(`     ${ph["goal"]}`);
        }
      }

      // Requirements summary
      const reqs = (p["requirements"] ?? []) as Array<Record<string, unknown>>;
      if (reqs.length > 0) {
        const v1 = reqs.filter(r => r["tier"] === "v1").length;
        const v2 = reqs.filter(r => r["tier"] === "v2").length;
        const oos = reqs.filter(r => r["tier"] === "out-of-scope").length;
        console.log(`\nRequirements: ${v1} v1, ${v2} v2, ${oos} out-of-scope`);
      }
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }
  });

planCmd
  .command("abandon <id>")
  .description("Abandon a planning project")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (id: string, opts: { url: string; token?: string }) => {
    try {
      await planApi(`/api/planning/projects/${id}`, opts, "DELETE");
      console.log(`Project ${id} abandoned.`);
    } catch (error) {
      console.error(`Failed: ${error instanceof Error ? error.message : error}`);
      process.exit(1);
    }
  });

// --- Update ---
program
  .command("update")
  .description("Build and deploy Aletheia from current source")
  .option("--dry-run", "Show what would be done without executing")
  .option("--skip-ui", "Skip UI build (runtime-only update)")
  .option("--no-restart", "Build and copy but don't restart the daemon")
  .action(async (opts: { dryRun?: boolean; skipUi?: boolean; restart?: boolean }) => {
    const { existsSync: exists } = await import("node:fs");
    const { execSync } = await import("node:child_process");
    const { join } = await import("node:path");
    const { loadBootstrapAnchor } = await import("./taxis/bootstrap-loader.js");

    const dryRun = opts.dryRun ?? false;
    const skipUi = opts.skipUi ?? false;
    const restart = opts.restart !== false; // default true unless --no-restart

    const run = (cmd: string, label: string, cwd?: string): boolean => {
      if (dryRun) {
        console.log(`  [dry-run] ${label}: ${cmd}${cwd ? ` (in ${cwd})` : ""}`);
        return true;
      }
      try {
        console.log(`  ${label}...`);
        execSync(cmd, { cwd, stdio: "pipe", timeout: 120_000 });
        console.log(`    ✅ done`);
        return true;
      } catch (error) {
        const msg = error instanceof Error ? error.message : String(error);
        console.error(`    ❌ ${msg.split("\n")[0]}`);
        return false;
      }
    };

    // Find source root — look for infrastructure/runtime/package.json
    let sourceRoot: string;
    try {
      const { anchor } = loadBootstrapAnchor();
      // Anchor gives us deploy dir; source root is typically the repo root
      // Walk up from anchor's nousDir to find the repo
      const repoMarkers = ["infrastructure/runtime/package.json", ".git"];
      let candidate = join(anchor.nousDir, "..");
      let found = false;
      for (let i = 0; i < 5; i++) {
        if (repoMarkers.every(m => exists(join(candidate, m)))) {
          sourceRoot = candidate;
          found = true;
          break;
        }
        candidate = join(candidate, "..");
      }
      if (!found) {
        // Fallback: common location
        if (exists("/mnt/ssd/aletheia/infrastructure/runtime/package.json")) {
          sourceRoot = "/mnt/ssd/aletheia";
        } else {
          console.error("Cannot locate Aletheia source root. Run from the repo or set anchor.json.");
          process.exit(1);
          return;
        }
      }
    } catch {
      if (exists("/mnt/ssd/aletheia/infrastructure/runtime/package.json")) {
        sourceRoot = "/mnt/ssd/aletheia";
      } else {
        console.error("Cannot locate Aletheia source root.");
        process.exit(1);
        return;
      }
    }

    const runtimeDir = join(sourceRoot!, "infrastructure", "runtime");
    const uiDir = join(sourceRoot!, "ui");

    // Determine deploy target
    let deployDir: string;
    try {
      const { anchor } = loadBootstrapAnchor();
      deployDir = anchor.deployDir;
    } catch {
      deployDir = join(sourceRoot!, "deploy");
    }

    console.log(`\nAletheia Update${dryRun ? " (dry run)" : ""}`);
    console.log(`  Source:  ${sourceRoot!}`);
    console.log(`  Deploy:  ${deployDir}`);
    console.log();

    // Step 1: Git pull (with dirty check)
    try {
      if (!dryRun) {
        const status = execSync("git status --porcelain", { cwd: sourceRoot!, encoding: "utf-8" }).trim();
        if (status) {
          console.log(`  ⚠️  Working tree has uncommitted changes (${status.split("\n").length} files)`);
        }
      }
    } catch { /* git status failed — proceed anyway */ }

    const sha1 = dryRun ? "abc1234" : execSync("git rev-parse --short HEAD", { cwd: sourceRoot!, encoding: "utf-8" }).trim();
    if (!run("git pull origin main --ff-only", "Git pull", sourceRoot!)) {
      console.error("\nGit pull failed. Resolve conflicts first.");
      process.exit(1);
    }
    const sha2 = dryRun ? "def5678" : execSync("git rev-parse --short HEAD", { cwd: sourceRoot!, encoding: "utf-8" }).trim();
    if (sha1 === sha2 && !dryRun) {
      console.log("  Already up to date.");
    }

    // Step 2: Build runtime
    if (!run("npx tsdown", "Build runtime", runtimeDir)) {
      process.exit(1);
    }

    // Step 3: Build UI (unless --skip-ui)
    if (!skipUi) {
      if (exists(join(uiDir, "package.json"))) {
        if (!run("npm run build", "Build UI", uiDir)) {
          process.exit(1);
        }
      } else {
        console.log("  [skip] UI directory not found");
      }
    } else {
      console.log("  [skip] UI build (--skip-ui)");
    }

    // Step 4: Copy artifacts
    const { mkdirSync } = await import("node:fs");
    if (!dryRun) mkdirSync(deployDir, { recursive: true });

    run(`cp -r ${join(runtimeDir, "dist", "entry.mjs")} ${join(deployDir, "entry.mjs")}`, "Copy runtime artifact");

    if (!skipUi && exists(join(uiDir, "dist"))) {
      run(`rm -rf ${join(deployDir, "ui")} && cp -r ${join(uiDir, "dist")} ${join(deployDir, "ui")}`, "Copy UI build");
    }

    // Copy shared assets
    for (const dir of ["shared/bin", "shared/config", "shared/templates"]) {
      const src = join(sourceRoot!, dir);
      const dest = join(deployDir, dir);
      if (exists(src)) {
        run(`mkdir -p ${dest} && rsync -a --delete ${src}/ ${dest}/`, `Sync ${dir}`);
      }
    }

    // Step 5: Restart
    if (restart) {
      const finalSha = dryRun ? "def5678" : execSync("git rev-parse --short HEAD", { cwd: sourceRoot!, encoding: "utf-8" }).trim();
      run("systemctl --user restart aletheia", "Restart daemon");

      if (!dryRun) {
        // Wait for startup, check health
        console.log("  Waiting for startup...");
        await new Promise(r => setTimeout(r, 3000));
        try {
          const res = await fetch("http://localhost:18789/api/metrics", { signal: AbortSignal.timeout(5000) });
          if (res.ok) {
            console.log(`  ✅ Gateway responding`);
          } else {
            console.log(`  ⚠️  Gateway returned ${res.status}`);
          }
        } catch {
          console.log("  ⚠️  Gateway not responding — check logs: journalctl --user -u aletheia -n 50");
        }
      }

      console.log(`\n✅ Updated to ${dryRun ? sha2 : finalSha}`);
    } else {
      console.log("\n✅ Build complete (restart skipped — use systemctl --user restart aletheia)");
    }
  });

program.parse();
