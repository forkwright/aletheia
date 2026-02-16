// Main orchestration — wire all modules
import { join } from "node:path";
import { createLogger } from "./koina/logger.js";
import { loadConfig } from "./taxis/loader.js";
import { paths } from "./taxis/paths.js";
import { SessionStore } from "./mneme/store.js";
import { createDefaultRouter, type ProviderRouter } from "./hermeneus/router.js";
import { ToolRegistry } from "./organon/registry.js";
import { execTool } from "./organon/built-in/exec.js";
import { readTool } from "./organon/built-in/read.js";
import { writeTool } from "./organon/built-in/write.js";
import { editTool } from "./organon/built-in/edit.js";
import { grepTool } from "./organon/built-in/grep.js";
import { findTool } from "./organon/built-in/find.js";
import { lsTool } from "./organon/built-in/ls.js";
import { webFetchTool } from "./organon/built-in/web-fetch.js";
import { webSearchTool } from "./organon/built-in/web-search.js";
import { braveSearchTool } from "./organon/built-in/brave-search.js";
import { mem0SearchTool } from "./organon/built-in/mem0-search.js";
import { factRetractTool } from "./organon/built-in/fact-retract.js";
import { browserTool, closeBrowser } from "./organon/built-in/browser.js";
import { createMessageTool } from "./organon/built-in/message.js";
import { createVoiceReplyTool } from "./organon/built-in/voice-reply.js";
import { cleanupTtsFiles } from "./semeion/tts.js";
import { createSessionsSendTool } from "./organon/built-in/sessions-send.js";
import { createSessionsAskTool } from "./organon/built-in/sessions-ask.js";
import { createSessionsSpawnTool } from "./organon/built-in/sessions-spawn.js";
import { createConfigReadTool } from "./organon/built-in/config-read.js";
import { createSessionStatusTool } from "./organon/built-in/session-status.js";
import { createPlanTools } from "./organon/built-in/plan.js";
import { NousManager } from "./nous/manager.js";
import { createGateway, startGateway, setCronRef, setWatchdogRef, setSkillsRef } from "./pylon/server.js";
import { createMcpRoutes } from "./pylon/mcp.js";
import { createUiRoutes } from "./pylon/ui.js";
import { SignalClient } from "./semeion/client.js";
import {
  spawnDaemon,
  waitForReady,
  daemonOptsFromConfig,
  type DaemonHandle,
} from "./semeion/daemon.js";
import { startListener } from "./semeion/listener.js";
import { sendMessage, parseTarget } from "./semeion/sender.js";
import { createDefaultRegistry } from "./semeion/commands.js";
import { SkillRegistry } from "./organon/skills.js";
import { loadPlugins } from "./prostheke/loader.js";
import { PluginRegistry } from "./prostheke/registry.js";
import { CronScheduler } from "./daemon/cron.js";
import { Watchdog, type ServiceProbe } from "./daemon/watchdog.js";
import type { AletheiaConfig } from "./taxis/schema.js";

const log = createLogger("aletheia");

export interface AletheiaRuntime {
  config: AletheiaConfig;
  store: SessionStore;
  router: ProviderRouter;
  tools: ToolRegistry;
  manager: NousManager;
  plugins: PluginRegistry;
  shutdown: () => void;
}

export function createRuntime(configPath?: string): AletheiaRuntime {
  log.info("Initializing Aletheia runtime");

  const config = loadConfig(configPath);
  const store = new SessionStore(paths.sessionsDb());
  const router = createDefaultRouter(config.models);

  const tools = new ToolRegistry();

  // File operations
  tools.register(execTool);
  tools.register(readTool);
  tools.register(writeTool);
  tools.register(editTool);

  // Search and listing
  tools.register(grepTool);
  tools.register(findTool);
  tools.register(lsTool);

  // Web access
  tools.register(webFetchTool);
  if (process.env["BRAVE_API_KEY"]) {
    tools.register(braveSearchTool);
    log.info("Web search: Brave (API key found)");
  } else {
    tools.register(webSearchTool);
    log.info("Web search: DuckDuckGo (no BRAVE_API_KEY)");
  }

  // Memory
  tools.register(mem0SearchTool);
  tools.register(factRetractTool);

  // Browser (requires chromium on host)
  if (process.env["CHROMIUM_PATH"] || process.env["ENABLE_BROWSER"]) {
    tools.register(browserTool);
    log.info("Browser tool registered");
  }

  // Wired tools (config + store injected)
  tools.register(createConfigReadTool(config));
  tools.register(createSessionStatusTool(store));

  // Planning tools
  for (const planTool of createPlanTools()) {
    tools.register(planTool);
  }

  log.info(`Registered ${tools.size} tools`);

  const bindings = config.bindings.map((b) => {
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

  const manager = new NousManager(config, store, router, tools);
  const plugins = new PluginRegistry(config);

  // Wire cross-agent tools (need manager + store reference for audit trail)
  const auditDispatcher = {
    handleMessage: manager.handleMessage.bind(manager),
    store,
  };
  tools.register(createSessionsSendTool(auditDispatcher));
  tools.register(createSessionsAskTool(auditDispatcher));
  tools.register(createSessionsSpawnTool(auditDispatcher));

  return {
    config,
    store,
    router,
    tools,
    manager,
    plugins,
    shutdown: () => {
      store.close();
      log.info("Runtime shutdown complete");
    },
  };
}

export async function startRuntime(configPath?: string): Promise<void> {
  const runtime = createRuntime(configPath);
  const config = runtime.config;

  // --- Plugins ---
  if (config.plugins.enabled && config.plugins.load.paths.length > 0) {
    const pluginDefs = await loadPlugins(config.plugins.load.paths);
    for (const plugin of pluginDefs) {
      const entry = config.plugins.entries[plugin.manifest.id];
      if (entry && !entry.enabled) {
        log.info(`Plugin ${plugin.manifest.id} disabled in config, skipping`);
        continue;
      }
      runtime.plugins.register(plugin, runtime.tools);
    }
    log.info(`Loaded ${runtime.plugins.size} plugins`);
    runtime.manager.setPlugins(runtime.plugins);
    await runtime.plugins.dispatchStart();
  }

  // --- Gateway ---
  const port = config.gateway.port;
  const app = createGateway(config, runtime.manager, runtime.store);

  // Mount MCP server routes
  const mcpRoutes = createMcpRoutes(config, runtime.manager, runtime.store);
  app.route("/mcp", mcpRoutes);

  // Mount Web UI
  const uiRoutes = createUiRoutes(config, runtime.manager, runtime.store);
  app.route("/", uiRoutes);

  startGateway(app, port);
  log.info(`Aletheia gateway listening on port ${port}`);

  // --- Skills ---
  const skills = new SkillRegistry();
  skills.loadFromDirectory(join(paths.shared, "skills"));
  const skillsSection = skills.toBootstrapSection();
  if (skillsSection) {
    runtime.manager.setSkillsSection(skillsSection);
  }
  setSkillsRef(skills);

  // --- Command Registry ---
  const commandRegistry = createDefaultRegistry();

  // --- Signal ---
  let watchdog: Watchdog | null = null;
  const abortController = new AbortController();
  const daemons: DaemonHandle[] = [];
  const clients = new Map<string, SignalClient>();

  // Collect bound group IDs from bindings so the listener can allow them
  const boundGroupIds = new Set<string>();
  for (const binding of config.bindings) {
    if (binding.match.peer?.kind === "group" && binding.match.peer.id) {
      boundGroupIds.add(binding.match.peer.id);
    }
  }

  if (config.channels.signal.enabled) {
    for (const [accountId, account] of Object.entries(
      config.channels.signal.accounts,
    )) {
      if (!account.enabled) continue;

      const httpUrl =
        account.httpUrl ??
        `http://${account.httpHost}:${account.httpPort}`;

      if (account.autoStart) {
        const daemonOpts = daemonOptsFromConfig(accountId, account);
        const handle = spawnDaemon(daemonOpts);
        daemons.push(handle);

        try {
          await waitForReady(handle.baseUrl);
        } catch (err) {
          log.error(
            `Signal daemon for ${accountId} failed to start: ${err instanceof Error ? err.message : err}`,
          );
          continue;
        }
      }

      const client = new SignalClient(httpUrl);
      clients.set(accountId, client);

      startListener({
        accountId,
        account,
        manager: runtime.manager,
        client,
        baseUrl: httpUrl,
        abortSignal: abortController.signal,
        boundGroupIds,
        commands: commandRegistry,
        store: runtime.store,
        config,
        get watchdog() { return watchdog; },
        skills,
        onStatusRequest: async (target) => {
          const status = formatStatusMessage(runtime.store, config, watchdog);
          await sendMessage(client, target, status, { markdown: false });
        },
      });

      log.info(`Signal account ${accountId} active at ${httpUrl}`);
    }

    if (clients.size > 0) {
      const firstClient = clients.values().next().value!;
      const firstAccountId = clients.keys().next().value!;
      const firstAccount =
        config.channels.signal.accounts[firstAccountId];
      const defaultAccount = firstAccount?.account ?? firstAccountId;

      const messageTool = createMessageTool({
        sender: {
          send: async (to: string, text: string) => {
            const target = parseTarget(to, defaultAccount);
            await sendMessage(firstClient, target, text);
          },
        },
      });
      runtime.tools.register(messageTool);
      log.info("Message tool registered with Signal sender");

      const voiceTool = createVoiceReplyTool({
        send: async (to: string, text: string, attachments: string[]) => {
          const target = parseTarget(to, defaultAccount);
          await sendMessage(firstClient, target, text, { attachments });
        },
      });
      runtime.tools.register(voiceTool);
      log.info("Voice reply tool registered");
    } else {
      runtime.tools.register(createMessageTool());
    }
  } else {
    runtime.tools.register(createMessageTool());
  }

  // --- Cron ---
  const cron = new CronScheduler(config, runtime.manager);
  setCronRef(cron);
  if (config.cron.enabled) {
    cron.start();
  }

  // --- Watchdog ---
  const wdConfig = config.watchdog;
  if (wdConfig.enabled && wdConfig.alertRecipient && clients.size > 0) {
    const alertClient = clients.values().next().value!;
    const alertAccountId = clients.keys().next().value!;
    const alertAccount = config.channels.signal.accounts[alertAccountId]!;
    const alertAccountPhone = alertAccount.account ?? alertAccountId;

    const services: ServiceProbe[] = wdConfig.services.length > 0
      ? wdConfig.services
      : [
          { name: "qdrant", url: "http://127.0.0.1:6333/healthz" },
          { name: "neo4j", url: "http://127.0.0.1:7474" },
          { name: "mem0-sidecar", url: "http://127.0.0.1:8230/health" },
          { name: "ollama", url: "http://127.0.0.1:11434/api/tags" },
        ];

    watchdog = new Watchdog({
      services,
      intervalMs: wdConfig.intervalMs,
      alertFn: async (message) => {
        await sendMessage(alertClient, {
          account: alertAccountPhone,
          recipient: wdConfig.alertRecipient!,
        }, message, { markdown: false });
      },
    });
    watchdog.start();
    setWatchdogRef(watchdog);
    runtime.manager.setWatchdog(watchdog);
  }

  // Spawn session cleanup — archive stale spawn sessions every hour
  // TTS file cleanup — remove stale audio files
  const spawnCleanupTimer = setInterval(() => {
    runtime.store.archiveStaleSpawnSessions();
    cleanupTtsFiles();
  }, 60 * 60 * 1000);

  // --- Shutdown ---
  let draining = false;
  runtime.manager.isDraining = () => draining;

  const shutdown = async () => {
    if (draining) return;
    draining = true;
    log.info("Shutting down — draining active turns (max 10s)...");
    clearInterval(spawnCleanupTimer);
    watchdog?.stop();
    cron.stop();

    // Wait up to 10s for active turns to finish
    const deadline = Date.now() + 10_000;
    while (runtime.manager.activeTurns > 0 && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 250));
    }
    if (runtime.manager.activeTurns > 0) {
      log.warn(`Forcing shutdown with ${runtime.manager.activeTurns} active turns`);
    }

    abortController.abort();
    for (const daemon of daemons) {
      daemon.stop();
    }
    await closeBrowser().catch(() => {});
    await runtime.plugins.dispatchShutdown();
    runtime.shutdown();
    process.exit(0);
  };

  process.on("SIGTERM", () => shutdown());
  process.on("SIGINT", () => shutdown());
}

function formatStatusMessage(
  store: SessionStore,
  config: AletheiaConfig,
  watchdog: Watchdog | null,
): string {
  const metrics = store.getMetrics();
  const uptime = process.uptime();
  const hours = Math.floor(uptime / 3600);
  const mins = Math.floor((uptime % 3600) / 60);

  const lines: string[] = ["Aletheia Status", ""];

  // Uptime
  lines.push(`Uptime: ${hours}h ${mins}m`);
  lines.push("");

  // Services
  if (watchdog) {
    const svcStatus = watchdog.getStatus();
    const allHealthy = svcStatus.every((s) => s.healthy);
    lines.push(`Services: ${allHealthy ? "all healthy" : "DEGRADED"}`);
    for (const svc of svcStatus) {
      lines.push(`  ${svc.healthy ? "+" : "X"} ${svc.name}`);
    }
    lines.push("");
  }

  // Per-nous activity
  lines.push("Nous:");
  for (const a of config.agents.list) {
    const m = metrics.perNous[a.id];
    const u = metrics.usageByNous[a.id];
    const name = a.name ?? a.id;
    const sessions = m?.activeSessions ?? 0;
    const msgs = m?.totalMessages ?? 0;
    const lastSeen = m?.lastActivity
      ? timeSince(new Date(m.lastActivity))
      : "never";
    const tokens = u ? `${Math.round(u.inputTokens / 1000)}k in` : "0k in";
    lines.push(`  ${name}: ${sessions} sessions, ${msgs} msgs, ${tokens}, last ${lastSeen}`);
  }
  lines.push("");

  // Usage
  const cacheHitRate = metrics.usage.totalInputTokens > 0
    ? Math.round((metrics.usage.totalCacheReadTokens / metrics.usage.totalInputTokens) * 100)
    : 0;
  lines.push(`Tokens: ${Math.round(metrics.usage.totalInputTokens / 1000)}k in, ${Math.round(metrics.usage.totalOutputTokens / 1000)}k out`);
  lines.push(`Cache: ${cacheHitRate}% hit rate`);
  lines.push(`Turns: ${metrics.usage.turnCount}`);

  return lines.join("\n");
}

function timeSince(date: Date): string {
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
