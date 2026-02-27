// Main orchestration — wire all modules
import { join } from "node:path";
import { createLogger } from "./koina/logger.js";
import { trySafe } from "./koina/safe.js";
import { applyEnv, loadConfig, watchConfig } from "./taxis/loader.js";
import { loadBootstrapAnchor } from "./taxis/bootstrap-loader.js";
import { mergeGitignore, scaffoldNousShared } from "./taxis/nous-scaffold.js";
import { initPaths, nousSharedDir, paths } from "./taxis/paths.js";
import { resolveSecretRefs } from "./taxis/secret-resolver.js";
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
import { memoryCorrectTool } from "./organon/built-in/memory-correct.js";
import { memoryForgetTool } from "./organon/built-in/memory-forget.js";
import { mem0RetractTool } from "./organon/built-in/mem0-retract.js";
import { mem0AuditTool } from "./organon/built-in/mem0-audit.js";
import { browserTool, closeBrowser } from "./organon/built-in/browser.js";
import { createMessageTool } from "./organon/built-in/message.js";
import { createVoiceReplyTool } from "./organon/built-in/voice-reply.js";
import { cleanupTtsFiles } from "./semeion/tts.js";
import { createSessionsSendTool } from "./organon/built-in/sessions-send.js";
import { createSessionsAskTool } from "./organon/built-in/sessions-ask.js";
import { createSessionsSpawnTool } from "./organon/built-in/sessions-spawn.js";
import { createSessionsDispatchTool } from "./organon/built-in/sessions-dispatch.js";
import { createConfigReadTool } from "./organon/built-in/config-read.js";
import { createSessionStatusTool } from "./organon/built-in/session-status.js";
import { createPlanTools } from "./organon/built-in/plan.js";
// plan-propose removed — deprecated in favor of Dianoia orchestrator
import { traceLookupTool } from "./organon/built-in/trace-lookup.js";
import { createCheckCalibrationTool } from "./organon/built-in/check-calibration.js";
import { createSelfEvaluateTool } from "./organon/built-in/self-evaluate.js";
import { createWhatDoIKnowTool } from "./organon/built-in/what-do-i-know.js";
import { createRecentCorrectionsTool } from "./organon/built-in/recent-corrections.js";
import { createBlackboardTool } from "./organon/built-in/blackboard.js";
import { createNoteTool } from "./organon/built-in/note.js";
import { createContextCheckTool } from "./organon/built-in/context-check.js";
import { createStatusReportTool } from "./organon/built-in/status-report.js";
import { createResearchTool } from "./organon/built-in/research.js";
import { createDeliberateTool } from "./organon/built-in/deliberate.js";
import { createSelfAuthorTools, loadAuthoredTools } from "./organon/self-author.js";
import { createPatchTools } from "./organon/built-in/propose-patch.js";
import { createPipelineConfigTool } from "./organon/built-in/pipeline-config.js";
import { createWorkspaceIndexTool } from "./organon/built-in/workspace-index.js";
import { loadCustomCommands, registerCustomCommands } from "./organon/custom-commands.js";
import { NousManager } from "./nous/manager.js";
import { DianoiaOrchestrator } from "./dianoia/orchestrator.js";
import { FileSyncDaemon } from "./dianoia/file-sync.js";
import { openPlansDb } from "./dianoia/plans-db.js";
import { CheckpointSystem, createPlanCreateTool, createPlanDiscussTool, createPlanExecuteTool, createPlanRequirementsTool, createPlanResearchTool, createPlanRoadmapTool, createPlanVerifyTool, ExecutionOrchestrator, GoalBackwardVerifier, PlanningStore, RequirementsOrchestrator, ResearchOrchestrator, RoadmapOrchestrator } from "./dianoia/index.js";
import { McpClientManager } from "./organon/mcp-client.js";
import { createGateway, type GatewayAuthDeps, setCommandsRef, setCronRef, setMcpRef, setSkillsRef, setWatchdogRef, startGateway } from "./pylon/server.js";
import { AuthSessionStore } from "./symbolon/sessions.js";
import { AuditLog } from "./symbolon/audit.js";
import { generateSecret } from "./symbolon/tokens.js";
import { createMcpRoutes } from "./pylon/mcp.js";
import { broadcastEvent, createUiRoutes } from "./pylon/ui.js";
import { AgoraRegistry } from "./agora/registry.js";
import { SignalChannelProvider } from "./semeion/provider.js";
import { SlackChannelProvider } from "./agora/channels/slack/provider.js";
import { sendMessage } from "./semeion/sender.js";
import { createDefaultRegistry } from "./semeion/commands.js";
import { SkillRegistry } from "./organon/skills.js";
import { discoverPlugins, loadPlugins } from "./prostheke/loader.js";
import { PluginRegistry } from "./prostheke/registry.js";
import { CronScheduler } from "./daemon/cron.js";
import { runNightlyReflection, runWeeklyReflection } from "./daemon/reflection-cron.js";
import { runEvolutionCycle } from "./daemon/evolution-cron.js";
import { runRetention } from "./daemon/retention.js";
import { type ServiceProbe, Watchdog } from "./daemon/watchdog.js";
import { startUpdateChecker } from "./daemon/update-check.js";
import { getVersion } from "./version.js";
import { CompetenceModel } from "./nous/competence.js";
import { UncertaintyTracker } from "./nous/uncertainty.js";
import type { AletheiaConfig } from "./taxis/schema.js";
import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { getKeySalt, initEncryption } from "./koina/encryption.js";
import { eventBus } from "./koina/event-bus.js";
import { type HookRegistry, registerHooks } from "./koina/hooks.js";
import { getSidecarUrl, getUserId } from "./koina/memory-client.js";

const log = createLogger("aletheia");

let _memoryHealthDegraded = false;
let _memoryDegradedMetrics: string[] = [];

type RoutingEntry = { channel: string; peerKind?: string; peerId?: string; accountId?: string; nousId: string };

function extractBindings(config: AletheiaConfig): RoutingEntry[] {
  return config.bindings.map((b) => {
    const entry: RoutingEntry = { channel: b.match.channel, nousId: b.agentId };
    if (b.match.peer?.kind) entry.peerKind = b.match.peer.kind;
    if (b.match.peer?.id) entry.peerId = b.match.peer.id;
    if (b.match.accountId) entry.accountId = b.match.accountId;
    return entry;
  });
}

export interface AletheiaRuntime {
  config: AletheiaConfig;
  store: SessionStore;
  router: ProviderRouter;
  tools: ToolRegistry;
  manager: NousManager;
  plugins: PluginRegistry;
  memoryTarget: import("./melete/hooks.js").MemoryFlushTarget;
  shutdown: () => void;
}

export function createRuntime(configPath?: string): AletheiaRuntime {
  const { anchor } = loadBootstrapAnchor();
  initPaths(anchor);

  // Best-effort gap-fill — non-blocking, warns on newly created dirs
  trySafe("nous:scaffold", () => {
    const nousDir = nousSharedDir(); // called inside callback — MUST be after initPaths()
    const created = scaffoldNousShared(nousDir);
    if (created.length > 0) {
      log.warn("nous scaffold dirs created at startup (run 'aletheia init' for authoritative setup)", { created });
    }
    mergeGitignore(nousDir); // idempotent — safe every startup
  }, undefined);

  const plansDb = openPlansDb(paths.sessionsDb());
  log.info("Plans DB opened", { path: plansDb.name });

  eventBus.emit("boot:start", {});
  log.info("Initializing Aletheia runtime");

  const config = loadConfig(configPath);

  applyEnv(config);
  resolveSecretRefs(config); // must run after applyEnv — env vars from config.env.vars must be set first

  // Initialize encryption before store (store uses encryptIfEnabled/decryptIfNeeded)
  if (config.encryption.enabled) {
    const passphrase = process.env[config.encryption.keyEnvVar];
    if (!passphrase) {
      log.warn(`Encryption enabled but ${config.encryption.keyEnvVar} not set — messages will NOT be encrypted`);
    } else {
      const saltPath = join(paths.configDir(), "encryption.salt");
      let salt: string | undefined;
      try {
        salt = readFileSync(saltPath, "utf-8").trim();
      } catch { /* salt file doesn't exist yet */ }
      initEncryption(passphrase, salt);
      if (!salt) {
        writeFileSync(saltPath, getKeySalt()!, { mode: 0o600 });
      }
      try { chmodSync(saltPath, 0o600); } catch { /* may already be correct */ }
      log.info("Message encryption active");
    }
  }

  const store = new SessionStore(paths.sessionsDb());

  // Harden file permissions on sensitive files at startup
  if (config.privacy.hardenFilePermissions) {
    const dbPath = paths.sessionsDb();
    const cfgPath = paths.configFile();
    for (const p of [dbPath, cfgPath]) {
      try {
        if (existsSync(p)) chmodSync(p, 0o600);
      } catch { /* salt file creation failed — non-fatal */
        log.warn(`Could not harden permissions on ${p}`);
      }
    }
  }

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

  // Web access (available on-demand)
  tools.register({ ...webFetchTool, category: "available" as const, domains: ["research", "writing"] });
  if (process.env["BRAVE_API_KEY"]) {
    tools.register({ ...braveSearchTool, category: "available" as const, domains: ["research", "writing"] });
    log.info("Web search: Brave (API key found)");
  } else {
    tools.register({ ...webSearchTool, category: "available" as const, domains: ["research", "writing"] });
    log.info("Web search: DuckDuckGo (no BRAVE_API_KEY)");
  }

  // Memory
  tools.register(mem0SearchTool);
  tools.register({ ...factRetractTool, category: "available" as const });
  tools.register({ ...memoryCorrectTool, category: "available" as const });
  tools.register({ ...memoryForgetTool, category: "available" as const });
  tools.register({ ...mem0RetractTool, category: "available" as const });
  tools.register({ ...mem0AuditTool, category: "available" as const });
  tools.register({ ...traceLookupTool, category: "available" as const });

  // Browser (requires chromium on host)
  if (process.env["CHROMIUM_PATH"] || process.env["ENABLE_BROWSER"]) {
    tools.register({ ...browserTool, category: "available" as const, domains: ["research"] });
    log.info("Browser tool registered");
  }

  // Wired tools (config + store injected — available on-demand)
  const configReadTool = createConfigReadTool(config);
  configReadTool.category = "available";
  tools.register(configReadTool);
  const sessionStatusTool = createSessionStatusTool(store);
  sessionStatusTool.category = "available";
  tools.register(sessionStatusTool);

  // Legacy planning tools — deprecated, available on-demand
  for (const planTool of createPlanTools(store)) {
    if (planTool.definition.name === "plan_create") continue; // replaced by Dianoia create-tool
    planTool.category = "available";
    tools.register(planTool);
  }

  // plan_propose removed — Dianoia orchestrator replaces it

  // Self-authoring tools (available on-demand)
  const defaultWorkspace = config.agents.list[0]?.workspace ?? "/tmp";
  for (const authorTool of createSelfAuthorTools(defaultWorkspace, tools)) {
    authorTool.category = "available";
    tools.register(authorTool);
  }
  const authoredCount = loadAuthoredTools(defaultWorkspace, tools);
  if (authoredCount > 0) log.info(`Loaded ${authoredCount} authored tools`);

  // Runtime code patching tools (available on-demand)
  for (const patchTool of createPatchTools()) {
    patchTool.category = "available";
    tools.register(patchTool);
  }

  // enable_tool meta-tool — lets agents activate available tools on demand
  const enableToolHandler: import("./organon/registry.js").ToolHandler = {
    definition: {
      name: "enable_tool",
      description:
        "Activate an available tool for this session. Tools auto-expire after 5 unused turns. " +
        "Available tools: " + tools.getAvailableToolNames().join(", "),
      input_schema: {
        type: "object",
        properties: {
          name: { type: "string", description: "Tool name to enable" },
        },
        required: ["name"],
      },
    },
    execute(input: Record<string, unknown>, context: import("./organon/registry.js").ToolContext): Promise<string> {
      const name = input["name"] as string;
      const ok = tools.enableTool(name, context.sessionId, 0);
      if (ok) return Promise.resolve(JSON.stringify({ enabled: true, tool: name }));
      return Promise.resolve(JSON.stringify({ enabled: false, error: `Tool "${name}" not found` }));
    },
  };
  tools.register(enableToolHandler);

  log.info(`Registered ${tools.size} tools`);

  store.rebuildRoutingCache(extractBindings(config));
  store.migrateSessionsToThreads();

  const manager = new NousManager(config, store, router, tools);

  const planningConfig = config.planning ?? {
    depth: "standard" as const,
    parallelization: true,
    research: true,
    plan_check: true,
    verifier: true,
    mode: "interactive" as const,
  };
  const planningStore = new PlanningStore(plansDb);
  const planningOrchestrator = new DianoiaOrchestrator(plansDb, planningConfig);
  manager.setPlanningOrchestrator(planningOrchestrator);

  // File sync daemon — writes markdown files alongside every DB mutation (co-primary)
  const fileSyncDaemon = new FileSyncDaemon(plansDb);
  fileSyncDaemon.start();

  log.info("Dianoia planning orchestrator initialized", { workspace: defaultWorkspace });

  const plugins = new PluginRegistry(config);

  // Memory flush target — connects distillation/reflection extraction to memory sidecar
  const memoryTarget: import("./melete/hooks.js").MemoryFlushTarget = {
    async addMemories(agentId: string, memories: string[], sessionId: string): Promise<{ added: number; errors: number }> {
      try {
        const res = await fetch(`${getSidecarUrl()}/add_batch`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            texts: memories,
            user_id: getUserId(),
            agent_id: agentId,
            source: "distillation",
            session_id: sessionId,
            confidence: 0.8,
          }),
          signal: AbortSignal.timeout(30_000),
        });
        if (!res.ok) {
          log.warn(`Memory flush HTTP ${res.status}: ${await res.text().catch(() => "")}`);
          return { added: 0, errors: memories.length };
        }
        const data = await res.json() as { added?: number; errors?: number; skipped?: number };
        const receipt = {
          origin: "distillation" as const,
          agentId,
          sessionId,
          timestamp: new Date().toISOString(),
          factCount: memories.length,
          added: data.added ?? 0,
          skipped: data.skipped ?? 0,
          errors: data.errors ?? 0,
        };
        log.info("Memory write receipt", receipt);
        return { added: data.added ?? 0, errors: data.errors ?? 0 };
      } catch (error) {
        log.warn(`Memory flush failed: ${error instanceof Error ? error.message : error}`);
        return { added: 0, errors: memories.length };
      }
    },
  };
  manager.setMemoryTarget(memoryTarget);
  manager.setSidecarUrl(getSidecarUrl());
  log.info("Memory flush target configured (sidecar /add_batch)");

  // Competence model + uncertainty tracker — wired into manager for runtime use
  const sharedRoot = paths.root;
  const competence = new CompetenceModel(sharedRoot);
  const uncertainty = new UncertaintyTracker(sharedRoot);
  manager.setCompetence(competence);
  manager.setUncertainty(uncertainty);
  log.info("Competence model and uncertainty tracker initialized");

  // Self-observation tools — query competence model, calibration, and interaction signals
  const calibrationTool = createCheckCalibrationTool(competence, uncertainty);
  calibrationTool.category = "available";
  tools.register(calibrationTool);
  const knowTool = createWhatDoIKnowTool(competence, store);
  knowTool.category = "available";
  tools.register(knowTool);
  const correctionsTool = createRecentCorrectionsTool(store);
  correctionsTool.category = "available";
  tools.register(correctionsTool);
  const selfEvalTool = createSelfEvaluateTool(store, competence, uncertainty);
  selfEvalTool.category = "available";
  tools.register(selfEvalTool);

  // Pipeline self-configuration — agents tune recall, tool expiry, note budget
  const pipelineCfgTool = createPipelineConfigTool();
  pipelineCfgTool.category = "available";
  tools.register(pipelineCfgTool);

  // Workspace index — file manifest for reducing exploratory ls/find calls
  tools.register(createWorkspaceIndexTool());

  // Cross-agent blackboard — persistent shared state with auto-expiry
  tools.register(createBlackboardTool(store));
  tools.register(createNoteTool(store));

  // Meta-tools — composed pipelines
  const ctxCheckTool = createContextCheckTool(tools);
  ctxCheckTool.category = "available";
  tools.register(ctxCheckTool);
  const statusTool = createStatusReportTool(store, competence);
  statusTool.category = "available";
  tools.register(statusTool);
  const researchTool = createResearchTool(tools);
  researchTool.category = "available";
  tools.register(researchTool);

  // Wire cross-agent tools (need manager + store reference for audit trail)
  const auditDispatcher = {
    handleMessage: manager.handleMessage.bind(manager),
    store,
  };
  tools.register(createSessionsSendTool(auditDispatcher));
  tools.register(createSessionsAskTool(auditDispatcher));
  const spawnTool = createSessionsSpawnTool(auditDispatcher, sharedRoot);
  spawnTool.category = "available";
  tools.register(spawnTool);
  const dispatchTool = createSessionsDispatchTool(auditDispatcher, sharedRoot);
  dispatchTool.category = "available";
  tools.register(dispatchTool);
  tools.register(createDeliberateTool(auditDispatcher));

  // Dianoia project creation — replaces old plan_create
  const planCreateTool = createPlanCreateTool(planningOrchestrator);
  tools.register(planCreateTool);

  // Planning research orchestrator — wired after dispatchTool is available
  const researchOrchestrator = new ResearchOrchestrator(plansDb, dispatchTool, defaultWorkspace);
  const planResearchTool = createPlanResearchTool(planningOrchestrator, researchOrchestrator);
  tools.register(planResearchTool);

  // Planning requirements orchestrator — wired after research orchestrator
  const requirementsOrchestrator = new RequirementsOrchestrator(plansDb, defaultWorkspace);
  const planRequirementsTool = createPlanRequirementsTool(planningOrchestrator, requirementsOrchestrator);
  tools.register(planRequirementsTool);

  // Planning roadmap orchestrator — wired after dispatchTool is available
  const roadmapOrchestrator = new RoadmapOrchestrator(plansDb, dispatchTool);
  const planRoadmapTool = createPlanRoadmapTool(planningOrchestrator, roadmapOrchestrator);
  tools.register(planRoadmapTool);

  // Planning discussion tool — bridges 'discussing' state between roadmap and phase-planning
  const planDiscussTool = createPlanDiscussTool(planningOrchestrator, plansDb, dispatchTool);
  tools.register(planDiscussTool);

  // Planning execution orchestrator — wired after dispatchTool is available
  const executionOrchestrator = new ExecutionOrchestrator(plansDb, dispatchTool);
  // Planning verifier — must be created before execution tool so it can be injected
  const verifierOrchestrator = new GoalBackwardVerifier(plansDb, dispatchTool);

  const planExecuteTool = createPlanExecuteTool(planningOrchestrator, executionOrchestrator, verifierOrchestrator);
  tools.register(planExecuteTool);
  manager.setExecutionOrchestrator(executionOrchestrator);
  const checkpointSystem = new CheckpointSystem(planningStore, planningConfig);
  const planVerifyTool = createPlanVerifyTool(
    planningOrchestrator,
    verifierOrchestrator,
    checkpointSystem,
    planningStore,
  );
  tools.register(planVerifyTool);
  log.info("Dianoia verifier and checkpoint system initialized");

  return {
    config,
    store,
    router,
    tools,
    manager,
    plugins,
    memoryTarget,
    shutdown: () => {
      fileSyncDaemon.stop();
      plansDb.close();
      store.close();
      log.info("Runtime shutdown complete");
    },
  };
}

export async function startRuntime(configPath?: string): Promise<void> {
  const runtime = createRuntime(configPath);
  const config = runtime.config;

  // --- Plugins ---
  if (config.plugins.enabled) {
    const pluginDefs = config.plugins.load.paths.length > 0
      ? await loadPlugins(config.plugins.load.paths)
      : [];

    // Auto-discover plugins from plugin root directory
    const discovered = await discoverPlugins(paths.pluginRoot);
    const loadedIds = new Set(pluginDefs.map((p) => p.manifest.id));
    for (const dp of discovered) {
      if (!loadedIds.has(dp.manifest.id)) {
        pluginDefs.push(dp);
      }
    }

    for (const plugin of pluginDefs) {
      const entry = config.plugins.entries[plugin.manifest.id];
      if (entry && !entry.enabled) {
        log.info(`Plugin ${plugin.manifest.id} disabled in config, skipping`);
        continue;
      }
      runtime.plugins.register(plugin, runtime.tools);
    }
    if (runtime.plugins.size > 0) {
      log.info(`Loaded ${runtime.plugins.size} plugins`);
      runtime.manager.setPlugins(runtime.plugins);
      await runtime.plugins.dispatchStart();
    }
  }

  // --- Declarative Hooks ---
  // Load YAML hook definitions from shared/hooks/ and wire to event bus.
  // These run shell commands at lifecycle points — no TypeScript required.
  const hooksDir = join(paths.shared, "hooks");
  const hookRegistry = registerHooks(hooksDir);
  // Also load per-nous hooks from each agent workspace (nous/<id>/hooks/)
  const perNousHookRegistries: HookRegistry[] = [];
  for (const agent of config.agents.list) {
    const agentHooksDir = join(paths.nous, agent.id, "hooks");
    const agentHooks = registerHooks(agentHooksDir);
    if (agentHooks.hooks.length > 0) {
      perNousHookRegistries.push(agentHooks);
    }
  }

  // --- Auth ---
  let gatewayAuth: GatewayAuthDeps | undefined;
  {
    // Reuse the SessionStore's database connection — both operate on disjoint
    // tables in sessions.db, so sharing avoids a redundant WAL reader (#346).
    const authDb = runtime.store.getDb();

    const auditLog = new AuditLog(authDb);
    let authSessionStore: AuthSessionStore | null = null;
    let sessionSecret: string | null = null;

    if (config.gateway.auth.mode === "session") {
      authSessionStore = new AuthSessionStore(authDb);
      const secretPath = join(paths.configDir(), "session.key");
      if (existsSync(secretPath)) {
        sessionSecret = readFileSync(secretPath, "utf-8").trim();
      } else {
        sessionSecret = generateSecret();
        writeFileSync(secretPath, sessionSecret, { mode: 0o600 });
        log.info("Generated new session signing key");
      }

      if (config.gateway.auth.users.length === 0) {
        log.warn("Auth mode is 'session' but no users configured — run 'aletheia migrate-auth' to create an admin account");
      }
    }

    gatewayAuth = { sessionStore: authSessionStore, auditLog, secret: sessionSecret };
  }

  // --- Gateway ---
  const port = config.gateway.port;
  const app = createGateway(config, runtime.manager, runtime.store, gatewayAuth);

  // Mount MCP server routes
  const mcpRoutes = createMcpRoutes(config, runtime.manager, runtime.store);
  app.route("/mcp", mcpRoutes);

  // Mount Web UI
  const uiRoutes = createUiRoutes(config, runtime.manager, runtime.store);
  app.route("/", uiRoutes);

  // Wire event bus → SSE push for real-time UI updates
  for (const eventName of [
    "turn:before", "turn:after", "tool:called", "tool:failed", "status:update",
    "session:created", "session:archived", "config:reloaded",
  ] as const) {
    eventBus.on(eventName, (payload) => broadcastEvent(eventName, payload));
  }

  // Record tool stats for usage analytics
  for (const eventName of ["tool:called", "tool:failed"] as const) {
    eventBus.on(eventName, (payload: Record<string, unknown>) => {
      try {
        const errMsg = eventName === "tool:failed" ? (payload["error"] as string)?.slice(0, 500) : undefined;
        const durMs = payload["durationMs"] as number | undefined;
        runtime.store.recordToolStat({
          nousId: (payload["nousId"] as string) ?? "unknown",
          toolName: (payload["tool"] as string) ?? "unknown",
          success: eventName === "tool:called",
          ...(errMsg ? { errorMessage: errMsg } : {}),
          ...(durMs !== null && durMs !== undefined ? { durationMs: durMs } : {}),
        });
      } catch { /* non-fatal */ }
    });
  }

  // Memory health event subscribers — track degraded state and log structured warnings
  eventBus.on("memory:health_degraded", (payload: Record<string, unknown>) => {
    const metrics = (payload["metrics"] as string[] | undefined) ?? [];
    _memoryHealthDegraded = true;
    _memoryDegradedMetrics = metrics;
    log.warn("Memory health degraded", { metrics, reason: payload["status"] });
  });

  eventBus.on("memory:health_recovered", (_payload: Record<string, unknown>) => {
    if (_memoryHealthDegraded) {
      log.info("Memory health recovered", { previousMetrics: _memoryDegradedMetrics });
      _memoryHealthDegraded = false;
      _memoryDegradedMetrics = [];
    }
  });

  startGateway(app, port);
  eventBus.emit("boot:ready", { port, tools: runtime.tools.size, plugins: runtime.plugins.size });
  log.info(`Aletheia gateway listening on port ${port}`);

  // --- Skills ---
  const skills = new SkillRegistry();
  skills.loadFromDirectory(join(paths.shared, "skills"));
  const skillsSection = skills.toBootstrapSection();
  if (skillsSection) {
    runtime.manager.setSkillsSection(skillsSection);
  }
  runtime.manager.setSkills(skills);
  setSkillsRef(skills);

  // --- MCP Client ---
  let mcpManager: McpClientManager | null = null;
  if (config.mcp.enabled && Object.keys(config.mcp.servers).length > 0) {
    mcpManager = new McpClientManager(runtime.tools);
    try {
      await mcpManager.connectAll(config.mcp.servers as Record<string, import("./organon/mcp-client.js").McpServerConfig>);
      log.info(`MCP client: ${mcpManager.getToolCount()} tools from ${mcpManager.getStatus().filter(s => s.status === "connected").length} server(s)`);
    } catch (error) {
      log.error(`MCP client initialization error: ${error instanceof Error ? error.message : error}`);
    }
    setMcpRef(mcpManager);
  }

  // --- Command Registry ---
  const commandRegistry = createDefaultRegistry();
  const customCmds = loadCustomCommands(join(paths.shared, "commands"));
  if (customCmds.length > 0) {
    const registered = registerCustomCommands(customCmds, commandRegistry, runtime.manager);
    log.info(`Loaded ${registered} custom commands from shared/commands/`);
  }
  setCommandsRef(commandRegistry);

  // /plan and !plan — route to DianoiaOrchestrator with slug intake routing
  const planOrch = runtime.manager.getPlanningOrchestrator();
  if (planOrch) {
    commandRegistry.register({
      name: "plan",
      description: "Start or resume a Dianoia planning project",
      execute(args, ctx) {
        const session = ctx.sessionId ? ctx.store.findSessionById(ctx.sessionId) : undefined;
        const nousId = session?.nousId ?? ctx.config.agents.list[0]?.id ?? "syn";
        const sessionId = ctx.sessionId ?? "";
        const userInput = args.trim();
        // Routing note: DianoiaOrchestrator intake state is checked here in the /plan command
        // handler, not in nous/manager.ts. The /plan command is the exclusive entry point to
        // planOrch — no pipeline stage or tool invokes planOrch.handle() directly — so the
        // consumer boundary (here) is the correct and complete routing location.
        // Migration response routing — highest priority (before slug intake)
        if (planOrch.hasPendingMigration()) {
          return Promise.resolve(planOrch.handleMigrationResponse(userInput, nousId, sessionId));
        }
        // Route to slug confirmation if confirmation is pending
        if (planOrch.hasPendingSlugConfirmation()) {
          return Promise.resolve(planOrch.receiveSlugConfirmation(userInput, nousId, sessionId));
        }
        // Route to name intake if we're waiting for a project name
        if (planOrch.hasPendingNameIntake()) {
          return Promise.resolve(planOrch.receiveProjectName(userInput, nousId, sessionId));
        }
        // Default: start or resume flow
        return Promise.resolve(planOrch.handle(nousId, sessionId));
      },
    });
    log.debug("Registered /plan command");
  }

  // --- Channels (Agora) ---
  let watchdog: Watchdog | null = null;
  const abortController = new AbortController();
  const agora = new AgoraRegistry();

  // Signal channel provider
  const signalProvider = new SignalChannelProvider({
    config,
    commands: commandRegistry,
    skills,
    onStatusRequest: async (client, target) => {
      const status = formatStatusMessage(runtime.store, config, watchdog);
      await sendMessage(client, target, status, { markdown: false });
    },
  });
  agora.register(signalProvider);

  // Slack channel provider (if configured)
  if (config.channels.slack?.enabled) {
    const slackProvider = new SlackChannelProvider(config);
    agora.register(slackProvider);
  }

  // Start all registered channels
  await agora.startAll({
    dispatch: (msg) => runtime.manager.handleMessage(msg),
    config,
    store: runtime.store,
    manager: runtime.manager,
    abortSignal: abortController.signal,
    commands: commandRegistry,
    get watchdog() { return watchdog; },
  });

  // Wire message tool to agora registry for multi-channel routing (Spec 34, Phase 4)
  if (agora.size > 0) {
    const messageTool = createMessageTool({ registry: agora });
    runtime.tools.register(messageTool);
    log.info(`Message tool registered via agora (channels: ${agora.list().join(", ")})`);
  } else {
    runtime.tools.register(createMessageTool());
    log.warn("Message tool registered without channels — sends will fail");
  }

  // Voice reply tool — Signal-only (requires TTS + audio file delivery)
  if (signalProvider.hasClients) {
    const voiceTool = createVoiceReplyTool({
      send: async (to: string, text: string, attachments: string[]) => {
        await agora.send("signal", { to, text, attachments });
      },
    });
    runtime.tools.register(voiceTool);
    log.info("Voice reply tool registered (Signal only)");
  }

  // --- Cron ---
  const cron = new CronScheduler(config, runtime.manager);
  setCronRef(cron);

  // Register built-in reflection command for cron
  cron.registerCommand("reflection:nightly", async () => {
    const result = await runNightlyReflection(
      runtime.store,
      runtime.router,
      config,
      {
        model: config.agents.defaults.compaction.distillationModel,
        minHumanMessages: 10,
        lookbackHours: 24,
        memoryTarget: runtime.memoryTarget,
      },
    );
    return `Reflected: ${result.agentsReflected} agents, ${result.totalFindings} findings, ${result.totalMemoriesStored} memories stored` +
      (result.errors.length > 0 ? ` (${result.errors.length} errors)` : "");
  });

  cron.registerCommand("reflection:weekly", async () => {
    const result = await runWeeklyReflection(
      runtime.store,
      runtime.router,
      config,
      {
        model: config.agents.defaults.compaction.distillationModel,
        lookbackDays: 7,
      },
    );
    return `Weekly reflection: ${result.agentsReflected} agents, ${result.totalFindings} findings`;
  });

  // Backup cron command — exports all agents to JSON files with retention
  cron.registerCommand("backup:all-agents", async () => {
    const { exportAgent, agentFileToJson } = await import("./portability/export.js");
    const dest = config.backup.destination;
    mkdirSync(dest, { recursive: true });

    const date = new Date().toISOString().split("T")[0];
    let count = 0;

    for (const agent of config.agents.list) {
      try {
        const agentFile = await exportAgent(agent.id, agent as unknown as Record<string, unknown>, runtime.store);
        const filename = `${agent.id}-${date}.agent.json`;
        writeFileSync(join(dest, filename), agentFileToJson(agentFile, false));
        count++;
      } catch (error) {
        log.warn(`Backup failed for ${agent.id}: ${error instanceof Error ? error.message : error}`);
      }
    }

    // Retention — delete old .agent.json files
    const cutoff = Date.now() - config.backup.retentionDays * 24 * 60 * 60 * 1000;
    try {
      for (const file of readdirSync(dest)) {
        if (!file.endsWith(".agent.json")) continue;
        const filePath = join(dest, file);
        const stat = statSync(filePath);
        if (stat.mtimeMs < cutoff) {
          unlinkSync(filePath);
          log.debug(`Deleted old backup: ${file}`);
        }
      }
    } catch { /* retention cleanup is best-effort */ }

    return `Backed up ${count} agents to ${dest}`;
  });

  // Evolutionary config search — mutate pipeline configs, benchmark, promote winners
  cron.registerCommand("evolution:nightly", async () => {
    const opts: Parameters<typeof runEvolutionCycle>[3] = {};
    if (signalProvider.hasClients && config.watchdog?.alertRecipient) {
      const alertRecipient = config.watchdog.alertRecipient;
      opts.sendNotification = async (_nousId, message) => {
        await agora.send("signal", { to: alertRecipient, text: message, markdown: false });
      };
    }
    const result = await runEvolutionCycle(runtime.store, runtime.router, config, opts);
    return `Evolution: ${result.agentsProcessed} agents, ${result.variantsCreated} variants, ${result.promotions} promotions`;
  });

  if (config.cron.enabled) {
    cron.start();
  }

  // --- Watchdog ---
  // Always start for dashboard service health, even without Signal/alertRecipient.
  // Alert function is optional — only wired when Signal is available + alertRecipient set.
  const wdConfig = config.watchdog;
  if (wdConfig.enabled) {
    const services: ServiceProbe[] = wdConfig.services.length > 0
      ? wdConfig.services
      : [
          { name: "qdrant", url: "http://127.0.0.1:6333/healthz" },
          { name: "neo4j", url: "http://127.0.0.1:7474" },
          { name: "mem0-sidecar", url: "http://127.0.0.1:8230/health" },
          { name: "ollama", url: "http://127.0.0.1:11434/api/tags" },
        ];

    // Wire alert function only when Signal + alertRecipient are configured
    let alertFn: ((message: string) => Promise<void>) | undefined;
    if (wdConfig.alertRecipient && signalProvider.hasClients) {
      alertFn = async (message) => {
        await agora.send("signal", { to: wdConfig.alertRecipient!, text: message, markdown: false });
      };
    }

    watchdog = new Watchdog({ services, intervalMs: wdConfig.intervalMs, ...(alertFn ? { alertFn } : {}) });
    watchdog.start();
    setWatchdogRef(watchdog);
    runtime.manager.setWatchdog(watchdog);
    log.info(`Watchdog started: ${services.length} services${alertFn ? ", alerts via agora" : ", no alert channel"}`);
  }

  // Spawn session cleanup — archive stale spawn sessions every hour
  // TTS file cleanup — remove stale audio files
  // Retention — enforce data lifecycle policy every 24h (with immediate first run)
  const spawnCleanupTimer = setInterval(() => {
    runtime.store.archiveStaleSpawnSessions();
    runtime.store.deleteEphemeralSessions(24 * 60 * 60 * 1000); // 24h
    cleanupTtsFiles();
    runtime.store.blackboardExpire();
  }, 60 * 60 * 1000);

  const retentionTimer = setInterval(() => {
    runRetention(runtime.store, config.privacy);
  }, 24 * 60 * 60 * 1000);
  // Run once shortly after startup so stale data is cleared without waiting 24h
  setTimeout(() => runRetention(runtime.store, config.privacy), 60_000);

  // --- Update checker ---
  const updateCheckTimer = startUpdateChecker(runtime.store, getVersion());

  // --- Config hot-reload ---
  const configWatcher = watchConfig(configPath, (newConfig) => {
    applyEnv(newConfig);
    resolveSecretRefs(newConfig); // resolve refs on hot-reload too
    const diff = runtime.manager.reloadConfig(newConfig);

    // Rebuild routing cache with new bindings
    const newBindings = extractBindings(newConfig);
    runtime.store.rebuildRoutingCache(newBindings);

    eventBus.emit("config:reloaded", { added: diff.added, removed: diff.removed });
    log.info(`Config reloaded: +${diff.added.length} -${diff.removed.length} agents, ${newBindings.length} bindings`);
  });

  // --- Shutdown ---
  let draining = false;
  runtime.manager.isDraining = () => draining;

  const shutdown = async () => {
    if (draining) return;
    draining = true;
    log.info("Shutting down — draining active turns (max 10s)...");
    clearInterval(spawnCleanupTimer);
    clearInterval(retentionTimer);
    clearInterval(updateCheckTimer);
    configWatcher?.close();
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

    if (mcpManager) await mcpManager.disconnectAll().catch(() => { /* disconnect errors suppressed on shutdown */ });
    abortController.abort();
    await agora.stopAll().catch(() => { /* channel stop errors suppressed on shutdown */ });
    await closeBrowser().catch(() => { /* browser close errors suppressed on shutdown */ });
    await runtime.plugins.dispatchShutdown();
    hookRegistry.teardown();
    for (const reg of perNousHookRegistries) reg.teardown();
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
