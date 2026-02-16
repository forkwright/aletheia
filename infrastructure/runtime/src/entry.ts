#!/usr/bin/env node
// CLI entry point
import { Command } from "commander";
import { startRuntime } from "./aletheia.js";
import { loadConfig } from "./taxis/loader.js";
import { createLogger } from "./koina/logger.js";

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
  .version("0.1.0");

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
      if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/metrics`, { headers, signal: AbortSignal.timeout(5000) });
      if (!res.ok) {
        console.error(`Error: HTTP ${res.status}`);
        process.exit(1);
      }

      const data = await res.json() as Record<string, unknown>;
      const uptime = data.uptime as number;
      const hours = Math.floor(uptime / 3600);
      const mins = Math.floor((uptime % 3600) / 60);

      console.log(`Aletheia Status (${opts.url})`);
      console.log(`  Uptime: ${hours}h ${mins}m`);
      console.log();

      const nous = data.nous as Array<Record<string, unknown>>;
      if (nous?.length) {
        console.log("Agents:");
        for (const n of nous) {
          const tokens = n.tokens as Record<string, number> | null;
          const inp = tokens ? `${Math.round(tokens.input / 1000)}k in` : "0k";
          console.log(`  ${n.name}: ${n.activeSessions} sessions, ${n.totalMessages} msgs, ${inp}`);
        }
        console.log();
      }

      const usage = data.usage as Record<string, unknown>;
      if (usage) {
        console.log(`Tokens: ${Math.round((usage.totalInputTokens as number) / 1000)}k in, ${Math.round((usage.totalOutputTokens as number) / 1000)}k out`);
        console.log(`Cache: ${usage.cacheHitRate}% hit rate, ${usage.turnCount} turns`);
      }

      const services = data.services as Array<Record<string, unknown>>;
      if (services?.length) {
        console.log();
        const allHealthy = services.every((s) => s.healthy);
        console.log(`Services: ${allHealthy ? "all healthy" : "DEGRADED"}`);
        for (const svc of services) {
          console.log(`  ${svc.healthy ? "+" : "X"} ${svc.name}`);
        }
      }

      const cron = data.cron as Array<Record<string, unknown>>;
      if (cron?.length) {
        console.log();
        console.log("Cron:");
        for (const job of cron) {
          console.log(`  ${job.id}: ${job.schedule} (next: ${job.nextRun ?? "unknown"})`);
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
      if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/sessions/send`, {
        method: "POST",
        headers,
        body: JSON.stringify({ agentId: opts.agent, message: opts.message, sessionKey: opts.session }),
        signal: AbortSignal.timeout(120000),
      });

      const data = await res.json() as Record<string, unknown>;
      if (!res.ok) {
        console.error(`Error: ${data.error ?? res.statusText}`);
        process.exit(1);
      }

      console.log(data.response);
      const usage = data.usage as Record<string, number>;
      if (usage) {
        console.log(`\n--- ${usage.inputTokens} in / ${usage.outputTokens} out / ${data.toolCalls} tool calls ---`);
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
      if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

      const query = opts.agent ? `?nousId=${opts.agent}` : "";
      const res = await fetch(`${opts.url}/api/sessions${query}`, { headers, signal: AbortSignal.timeout(5000) });
      const data = await res.json() as { sessions: Array<Record<string, unknown>> };

      if (!data.sessions?.length) {
        console.log("No sessions found.");
        return;
      }

      console.log(`${data.sessions.length} sessions:`);
      for (const s of data.sessions.slice(0, 30)) {
        console.log(`  ${s.id} ${s.nousId} [${s.status}] ${s.messageCount} msgs (${s.updatedAt})`);
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
      if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/cron`, { headers, signal: AbortSignal.timeout(5000) });
      const data = await res.json() as { jobs: Array<Record<string, unknown>> };

      if (!data.jobs?.length) {
        console.log("No cron jobs configured.");
        return;
      }

      for (const job of data.jobs) {
        console.log(`  ${job.id}: ${job.schedule} agent=${job.agentId} last=${job.lastRun ?? "never"} next=${job.nextRun ?? "unknown"}`);
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
      if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

      const res = await fetch(`${opts.url}/api/cron/${id}/trigger`, { method: "POST", headers, signal: AbortSignal.timeout(120000) });
      const data = await res.json() as Record<string, unknown>;

      if (!res.ok) {
        console.error(`Error: ${data.error ?? res.statusText}`);
        process.exit(1);
      }
      console.log(`Triggered cron job: ${id}`);
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });

program.parse();
