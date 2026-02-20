// Context stage — bootstrap assembly, recall, broadcasts, working state, notes injection
import { createLogger, updateTurnContext } from "../../../koina/logger.js";
import { estimateTokens, estimateToolDefTokens } from "../../../hermeneus/token-counter.js";
import { assembleBootstrap } from "../../bootstrap.js";
import { detectBootstrapDiff, logBootstrapDiff } from "../../bootstrap-diff.js";
import { recallMemories } from "../../recall.js";
import { formatWorkingState } from "../../working-state.js";
import { distillSession } from "../../../distillation/pipeline.js";
import { eventBus } from "../../../koina/event-bus.js";
import type { RuntimeServices, SystemBlock, TurnState } from "../types.js";

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

  // Pre-flight context overflow guard:
  // If the last API-reported token count is ≥90% of the context window, distill
  // NOW — before building history — so this turn doesn't hit the 200K hard limit.
  {
    const preflightContextTokens = services.config.agents.defaults.contextTokens;
    if (preflightContextTokens > 0) {
      const session = services.store.findSessionById(sessionId);
      const lastActual = session?.lastInputTokens ?? 0;
      const dangerThreshold = Math.floor(preflightContextTokens * 0.90);
      if (session && lastActual >= dangerThreshold && session.messageCount >= 10) {
        log.warn(
          `Emergency distillation: ${nousId} session=${sessionId} at ${lastActual} tokens ` +
          `(${Math.round((lastActual / preflightContextTokens) * 100)}% — above 90% ceiling)`,
        );
        const compaction = services.config.agents.defaults.compaction;
        const thread = services.store.getThreadForSession(sessionId);
        try {
          await distillSession(services.store, services.router, sessionId, nousId, {
            triggerThreshold: dangerThreshold,
            minMessages: 10,
            extractionModel: compaction.distillationModel,
            summaryModel: compaction.distillationModel,
            preserveRecentMessages: compaction.preserveRecentMessages,
            preserveRecentMaxTokens: compaction.preserveRecentMaxTokens,
            ...(workspace ? { workspace } : {}),
            ...(services.plugins ? { plugins: services.plugins } : {}),
            ...(thread ? {
              onThreadSummaryUpdate: (summary, keyFacts) => {
                services.store.updateThreadSummary(thread.id, summary, keyFacts);
              },
            } : {}),
          });
          log.info(`Emergency distillation complete for ${nousId} — context cleared`);
        } catch (distillErr) {
          log.error(`Emergency distillation failed: ${distillErr instanceof Error ? distillErr.message : distillErr}`);
        }
      }
    }
  }

  // System prompt blocks
  const systemPrompt: SystemBlock[] = [
    ...bootstrap.staticBlocks,
    ...bootstrap.dynamicBlocks,
  ];

  // Thread-level relationship context (injected before recall so it primes memory search)
  if (msg.threadId) {
    const threadSummary = services.store.getThreadSummary(msg.threadId);
    if (threadSummary?.summary) {
      const factsText = threadSummary.keyFacts.length > 0
        ? `\n\n**Key facts:**\n${threadSummary.keyFacts.slice(0, 20).map((f) => `- ${f}`).join("\n")}`
        : "";
      systemPrompt.push({
        type: "text",
        text: `## Thread Memory\n\n${threadSummary.summary}${factsText}`,
      });
    }
  }

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

  // Working state injection — semantic task context from post-turn extraction
  const currentSession = services.store.findSessionById(sessionId);
  const workingState = currentSession?.workingState;
  if (workingState) {
    systemPrompt.push({
      type: "text",
      text: formatWorkingState(workingState),
    });
  }

  // Agent notes injection — explicit notes written by the agent that survive distillation
  const notes = services.store.getNotes(sessionId, { limit: 20 });
  if (notes.length > 0) {
    const NOTE_TOKEN_CAP = 2000;
    const header = "## Agent Notes\n\nNotes you wrote during this session. These survive context distillation.\n\n";
    let tokenCount = estimateTokens(header);
    const noteLines: string[] = [];

    for (const note of notes) {
      const line = `- [${note.category}] ${note.content}`;
      const lineTokens = estimateTokens(line + "\n");
      if (tokenCount + lineTokens > NOTE_TOKEN_CAP) break;
      noteLines.push(line);
      tokenCount += lineTokens;
    }

    if (noteLines.length > 0) {
      systemPrompt.push({
        type: "text",
        text: header + noteLines.join("\n"),
      });
    }
  }

  // Session metrics — injected every 8th turn for self-awareness
  const msgCount = currentSession?.messageCount ?? 0;
  if (msgCount > 0 && msgCount % 8 === 0) {
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
        `## Session Metrics — Turn ${msgCount}\n\n` +
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
