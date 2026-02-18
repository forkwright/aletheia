// Context stage — bootstrap assembly, recall, broadcasts, working state injection
import { createLogger, updateTurnContext } from "../../../koina/logger.js";
import { estimateToolDefTokens } from "../../../hermeneus/token-counter.js";
import { assembleBootstrap } from "../../bootstrap.js";
import { detectBootstrapDiff, logBootstrapDiff } from "../../bootstrap-diff.js";
import { recallMemories } from "../../recall.js";
import { eventBus } from "../../../koina/event-bus.js";
import type { TurnState, RuntimeServices, SystemBlock } from "../types.js";

const log = createLogger("pipeline:context");

export async function buildContext(
  state: TurnState,
  services: RuntimeServices,
): Promise<TurnState> {
  const { nousId, sessionId, sessionKey, msg, workspace } = state;

  updateTurnContext({ nousId, sessionId, sessionKey });
  eventBus.emit("turn:before", { nousId, sessionId, sessionKey, channel: msg.channel });
  log.info(`Preparing context for ${nousId}:${sessionKey} (session ${sessionId})`);

  // Degraded services
  const degradedServices: string[] = [];
  if (services.watchdog) {
    for (const svc of services.watchdog.getStatus()) {
      if (!svc.healthy) degradedServices.push(svc.name);
    }
  }

  // Bootstrap
  const bootstrap = assembleBootstrap(workspace, {
    maxTokens: services.config.agents.defaults.bootstrapMaxTokens,
    ...(services.skillsSection ? { skillsSection: services.skillsSection } : {}),
    ...(degradedServices.length > 0 ? { degradedServices } : {}),
  });

  services.store.updateBootstrapHash(sessionId, bootstrap.contentHash);
  const diff = detectBootstrapDiff(nousId, bootstrap.fileHashes, workspace);
  if (diff) logBootstrapDiff(diff, workspace);
  if (bootstrap.droppedFiles.length > 0) {
    log.warn(`Bootstrap for ${nousId} dropped files due to budget: ${bootstrap.droppedFiles.join(", ")}`);
  }

  state.trace.setBootstrap(Object.keys(bootstrap.fileHashes), bootstrap.totalTokens);
  if (degradedServices.length > 0) state.trace.setDegradedServices(degradedServices);

  // System prompt blocks
  const systemPrompt: SystemBlock[] = [
    ...bootstrap.staticBlocks,
    ...bootstrap.dynamicBlocks,
  ];

  // Pre-turn memory recall
  let recallTokens = 0;
  if (!degradedServices.includes("mem0-sidecar")) {
    const recall = await recallMemories(msg.text, nousId);
    if (recall.block) systemPrompt.push(recall.block);
    recallTokens = recall.tokens;
    if (recall.count > 0) {
      log.info(`Recalled ${recall.count} memories for ${nousId} (${recall.durationMs}ms, ~${recall.tokens} tokens)`);
      state.trace.setRecall(recall.count, recall.durationMs);
    }
  }

  // Broadcasts
  const broadcasts = services.store.blackboardReadPrefix("broadcast:");
  if (broadcasts.length > 0) {
    const broadcastLines = broadcasts
      .slice(0, 5)
      .map((b) => `- **[${b.key.replace("broadcast:", "")}]** ${b.value.slice(0, 300)}`)
      .join("\n");
    systemPrompt.push({ type: "text", text: `## Broadcasts\n\n${broadcastLines}` });
  }

  // Working state injection
  const currentSession = services.store.findSessionById(sessionId);
  const msgCount = currentSession?.messageCount ?? 0;
  if (msgCount > 0 && msgCount % 8 === 0) {
    const recentTools = services.store.getRecentToolCalls(sessionId, 6);
    const elapsed = currentSession?.createdAt
      ? Math.round((Date.now() - new Date(currentSession.createdAt).getTime()) / 60000)
      : 0;
    const contextTokens = services.config.agents.defaults.contextTokens;
    const utilization = contextTokens > 0
      ? Math.round(((currentSession?.tokenCountEstimate ?? 0) / contextTokens) * 100)
      : 0;
    systemPrompt.push({
      type: "text",
      text:
        `## Working State — Turn ${msgCount}\n\n` +
        `Recent tools: ${recentTools.length > 0 ? recentTools.join(", ") : "none"}\n` +
        `Session duration: ${elapsed} min\n` +
        `Context utilization: ${utilization}%\n` +
        `Distillations: ${currentSession?.distillationCount ?? 0}`,
    });
  }

  // Tool definitions
  const nous = state.nous;
  const toolDefs = services.tools.getDefinitions({
    sessionId,
    ...(nous.tools.allow.length > 0 ? { allow: nous.tools.allow } : {}),
    ...(nous.tools.deny.length > 0 ? { deny: nous.tools.deny } : {}),
  });

  state.systemPrompt = systemPrompt;
  state.toolDefs = toolDefs;

  // Return recallTokens + bootstrap data for history budget calculation
  const toolDefTokens = estimateToolDefTokens(toolDefs);
  const contextTokens = services.config.agents.defaults.contextTokens;
  const maxOutput = services.config.agents.defaults.maxOutputTokens;
  const historyBudget = Math.max(0, contextTokens - bootstrap.totalTokens - toolDefTokens - maxOutput - recallTokens);

  // Store historyBudget on state for history stage
  (state as TurnState & { _historyBudget: number })._historyBudget = historyBudget;

  return state;
}
