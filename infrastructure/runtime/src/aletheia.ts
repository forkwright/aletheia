// Main orchestration — wire all modules
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
import { mem0SearchTool } from "./organon/built-in/mem0-search.js";
import { createMessageTool } from "./organon/built-in/message.js";
import { createSessionsSendTool } from "./organon/built-in/sessions-send.js";
import { createSessionsAskTool } from "./organon/built-in/sessions-ask.js";
import { createSessionsSpawnTool } from "./organon/built-in/sessions-spawn.js";
import { createConfigReadTool } from "./organon/built-in/config-read.js";
import { createSessionStatusTool } from "./organon/built-in/session-status.js";
import { NousManager } from "./nous/manager.js";
import { createGateway, startGateway } from "./pylon/server.js";
import { SignalClient } from "./semeion/client.js";
import {
  spawnDaemon,
  waitForReady,
  daemonOptsFromConfig,
  type DaemonHandle,
} from "./semeion/daemon.js";
import { startListener } from "./semeion/listener.js";
import { sendMessage, parseTarget } from "./semeion/sender.js";
import { loadPlugins } from "./prostheke/loader.js";
import { PluginRegistry } from "./prostheke/registry.js";
import { CronScheduler } from "./daemon/cron.js";
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
  const router = createDefaultRouter();

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
  tools.register(webSearchTool);

  // Memory
  tools.register(mem0SearchTool);

  // Wired tools (config + store injected)
  tools.register(createConfigReadTool(config));
  tools.register(createSessionStatusTool(store));

  log.info(`Registered ${tools.size} tools`);

  const bindings = config.bindings.map((b) => ({
    channel: b.match.channel,
    peerKind: b.match.peer?.kind,
    peerId: b.match.peer?.id,
    accountId: b.match.accountId,
    nousId: b.agentId,
  }));
  store.rebuildRoutingCache(bindings);

  const manager = new NousManager(config, store, router, tools);
  const plugins = new PluginRegistry(config);

  // Wire cross-agent tools (need manager reference)
  tools.register(createSessionsSendTool(manager));
  tools.register(createSessionsAskTool(manager));
  tools.register(createSessionsSpawnTool(manager));

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
  startGateway(app, port);
  log.info(`Aletheia gateway listening on port ${port}`);

  // --- Signal ---
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
      });

      log.info(`Signal account ${accountId} active at ${httpUrl}`);
    }

    if (clients.size > 0) {
      const firstClient = clients.values().next().value!;
      const firstAccountId = clients.keys().next().value!;
      const firstAccount =
        config.channels.signal.accounts[firstAccountId];
      const defaultAccount = firstAccount.account ?? firstAccountId;

      const messageTool = createMessageTool({
        send: async (to: string, text: string) => {
          const target = parseTarget(to, defaultAccount);
          await sendMessage(firstClient, target, text);
        },
      });
      runtime.tools.register(messageTool);
      log.info("Message tool registered with Signal sender");
    } else {
      runtime.tools.register(createMessageTool());
    }
  } else {
    runtime.tools.register(createMessageTool());
  }

  // --- Cron ---
  const cron = new CronScheduler(config, runtime.manager);
  if (config.cron.enabled) {
    cron.start();
  }

  // --- Shutdown ---
  const shutdown = async () => {
    log.info("Shutting down...");
    cron.stop();
    abortController.abort();
    for (const daemon of daemons) {
      daemon.stop();
    }
    await runtime.plugins.dispatchShutdown();
    runtime.shutdown();
    process.exit(0);
  };

  process.on("SIGTERM", () => shutdown());
  process.on("SIGINT", () => shutdown());
}
