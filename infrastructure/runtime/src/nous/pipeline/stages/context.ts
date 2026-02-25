// Context stage — bootstrap assembly, recall, broadcasts, working state, notes injection
import { createLogger, updateTurnContext } from "../../../koina/logger.js";
import { estimateTokens, estimateToolDefTokens } from "../../../hermeneus/token-counter.js";
import { assembleBootstrap } from "../../bootstrap.js";
import { detectBootstrapDiff, logBootstrapDiff } from "../../bootstrap-diff.js";
import { recallMemories } from "../../recall.js";
import { formatWorkingState } from "../../working-state.js";
import { distillSession } from "../../../distillation/pipeline.js";
import { eventBus } from "../../../koina/event-bus.js";
import { classifyDomain } from "../../interaction-signals.js";
import { indexWorkspace, loadIndexConfig, queryIndex } from "../../../organon/workspace-indexer.js";
import { loadPipelineConfig } from "../../pipeline-config.js";
import { detectPlanningIntent } from "../../../dianoia/intent.js";
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
  let preflightDistilled = false;
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
            ...(services.memoryTarget ? { memoryTarget: services.memoryTarget } : {}),
            ...(thread ? {
              onThreadSummaryUpdate: (summary, keyFacts) => {
                services.store.updateThreadSummary(thread.id, summary, keyFacts);
              },
            } : {}),
          });
          log.info(`Emergency distillation complete for ${nousId} — context cleared`);
          preflightDistilled = true;
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
  let threadSummaryText: string | undefined;
  if (msg.threadId) {
    const threadSummary = services.store.getThreadSummary(msg.threadId);
    if (threadSummary?.summary) {
      threadSummaryText = threadSummary.summary;
      const factsText = threadSummary.keyFacts.length > 0
        ? `\n\n**Key facts:**\n${threadSummary.keyFacts.slice(0, 20).map((f) => `- ${f}`).join("\n")}`
        : "";
      systemPrompt.push({
        type: "text",
        text: `## Thread Memory\n\n${threadSummary.summary}${factsText}`,
      });
    }
  }

  // Pipeline config — per-agent tuning of recall, tool expiry, note budget
  const pipelineConfig = loadPipelineConfig(workspace);

  // Pre-turn memory recall
  let recallTokens = 0;
  if (!degradedServices.includes("mem0-sidecar")) {
    const recall = await recallMemories(msg.text, nousId, {
      limit: pipelineConfig.recall.limit,
      maxTokens: pipelineConfig.recall.maxTokens,
      minScore: pipelineConfig.recall.minScore,
      sufficiencyThreshold: pipelineConfig.recall.sufficiencyThreshold,
      sufficiencyMinHits: pipelineConfig.recall.sufficiencyMinHits,
      ...(state.nous.domains ? { domains: state.nous.domains } : {}),
      ...(threadSummaryText ? { threadSummary: threadSummaryText } : {}),
    });
    if (recall.block) systemPrompt.push(recall.block);
    recallTokens = recall.tokens;
    if (recall.count > 0) {
      log.info(`Recalled ${recall.count} memories for ${nousId} (${recall.durationMs}ms, ~${recall.tokens} tokens)`);
      state.trace.setRecall(recall.count, recall.durationMs);
    }
  }

  // Domain-based proactive tool activation + skill-declared tool activation
  const domain = classifyDomain(msg.text ?? "");
  if (domain) {
    const activated = services.tools.enableToolsForDomains([domain], sessionId, state.seq ?? 0);
    if (activated.length > 0) {
      log.info(`Domain "${domain}": pre-activated tools for ${nousId}: ${activated.join(", ")}`);
    }
    if (services.skills) {
      const domainSkills = services.skills.getSkillsForDomain(domain);
      const skillTools = [...new Set(domainSkills.flatMap((s) => s.tools ?? []))];
      if (skillTools.length > 0) {
        for (const toolName of skillTools) {
          services.tools.enableTool(toolName, sessionId, state.seq ?? 0);
        }
        log.info(`Skill tools pre-activated for domain "${domain}": ${skillTools.join(", ")}`);
      }
    }
    const remaining = services.tools.getAvailableToolNamesExcluding(sessionId);
    if (remaining.length > 0) {
      systemPrompt.push({
        type: "text",
        text: `## Available Tools (not yet loaded)\n\nCall \`enable_tool\` to activate: ${remaining.join(", ")}`,
      });
    }
  }

  // Workspace index injection — pre-computed file manifest to reduce exploratory ls/find calls
  const wsConfig = pipelineConfig.workspaceIndex;
  if (workspace && wsConfig.enabled) {
    try {
      const indexConfig = await loadIndexConfig(workspace);
      const index = await indexWorkspace(workspace, indexConfig.extraPaths);
      if (index.files.length > 0) {
        const relevant = queryIndex(index, msg.text ?? "", wsConfig.highlightLimit);
        const manifestLines = index.files
          .slice(0, wsConfig.manifestLimit)
          .map((f) => `- ${f.path}`)
          .join("\n");
        const highlightLines = relevant
          .map((f) => `- ${f.path}${f.firstLine ? `: ${f.firstLine}` : ""}`)
          .join("\n");
        systemPrompt.push({
          type: "text",
          text:
            `## Workspace Index\n\n${index.files.length} files indexed.\n\n` +
            (relevant.length > 0 ? `**Relevant to this query:**\n${highlightLines}\n\n` : "") +
            `**Manifest (${Math.min(index.files.length, wsConfig.manifestLimit)} of ${index.files.length} shown):**\n${manifestLines}`,
        });
        log.debug(`Workspace index injected for ${nousId}: ${index.files.length} files, ${relevant.length} relevant`);
      }
    } catch (idxErr) {
      log.debug(`Workspace index unavailable: ${idxErr instanceof Error ? idxErr.message : idxErr}`);
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

  // Degraded services — surface watchdog status so the agent doesn't silently fail
  if (degradedServices.length > 0) {
    systemPrompt.push({
      type: "text",
      text:
        `## Degraded Services\n\n` +
        `The following services are currently unhealthy:\n` +
        `${degradedServices.map((s) => `- ${s}`).join("\n")}\n\n` +
        `Memory recall and dependent tool operations may be limited.`,
    });
  }

  // Bootstrap truncation — agent should know which workspace files were not loaded
  if (bootstrap.droppedFiles.length > 0) {
    systemPrompt.push({
      type: "text",
      text:
        `## Bootstrap Truncated\n\n` +
        `These workspace files exceeded the token budget and were not loaded:\n` +
        `${bootstrap.droppedFiles.map((f) => `- ${f}`).join("\n")}\n\n` +
        `The files exist on disk — use read_file to access them directly.`,
    });
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

  // Planning context injection — soft presence pattern (only when project is active)
  const planningOrchestrator = services.planningOrchestrator;
  if (planningOrchestrator) {
    const activeProject = planningOrchestrator.getActiveProject(nousId);
    if (activeProject) {
      const hasPending = planningOrchestrator.hasPendingConfirmation(activeProject);
      const ctx = activeProject.projectContext;
      const nextQuestion = activeProject.state === "questioning"
        ? planningOrchestrator.getNextQuestion(activeProject.id)
        : null;

      const lines: string[] = [
        `## Active Dianoia Planning Project`,
        ``,
        `Project ID: ${activeProject.id}`,
        `State: ${activeProject.state}`,
        `Goal: ${activeProject.goal || "(not yet set)"}`,
      ];

      if (ctx?.coreValue) {
        lines.push(`Core Value: ${ctx.coreValue}`);
      }
      if (ctx?.constraints && ctx.constraints.length > 0) {
        lines.push(`Constraints: ${ctx.constraints.join("; ")}`);
      }
      if (ctx?.keyDecisions && ctx.keyDecisions.length > 0) {
        lines.push(`Key Decisions: ${ctx.keyDecisions.join("; ")}`);
      }
      if (hasPending) {
        lines.push(``, `Awaiting resume confirmation from user.`);
      }
      if (nextQuestion) {
        lines.push(``, `## Planning Question`, ``, nextQuestion);
      }

      systemPrompt.push({
        type: "text",
        text: lines.join("\n"),
      });
    } else if (detectPlanningIntent(msg.text ?? "")) {
      systemPrompt.push({
        type: "text",
        text: "## Planning Offer\n\nThis message sounds like a planning task — if you'd like structured planning support, say 'yes, start planning' and I'll open a Dianoia project.",
      });
    }
  }

  // Post-distillation priming — inject extracted context from the most recent distillation.
  // This ensures the agent's first turn after distillation has full awareness of what was
  // compressed, independent of recall similarity matching. Consumed once and cleared.
  const priming = services.store.getDistillationPriming(sessionId);
  if (priming) {
    const sections: string[] = [];
    sections.push(
      preflightDistilled
        ? `Context hit the 90% ceiling — distillation ran automatically before this response (compression #${priming.distillationNumber}). Key extracted context below.`
        : `Context was distilled (compression #${priming.distillationNumber}). Key extracted context below.`,
    );
    if (priming.facts.length > 0) {
      sections.push(`**Facts:**\n${priming.facts.map(f => `- ${f}`).join("\n")}`);
    }
    if (priming.decisions.length > 0) {
      sections.push(`**Decisions:**\n${priming.decisions.map(d => `- ${d}`).join("\n")}`);
    }
    if (priming.openItems.length > 0) {
      sections.push(`**Open items:**\n${priming.openItems.map(o => `- ${o}`).join("\n")}`);
    }
    systemPrompt.push({
      type: "text",
      text: `## Post-Distillation Context\n\n${sections.join("\n\n")}`,
    });
    // Clear after injection — one-shot priming
    services.store.clearDistillationPriming(sessionId);
    log.info(`Injected post-distillation priming for ${nousId} (${priming.facts.length} facts, ${priming.decisions.length} decisions, ${priming.openItems.length} open items)`);
  }

  // Agent notes injection — explicit notes written by the agent that survive distillation
  // No count limit — token budget controls how many are injected. Most recent first.
  const notes = services.store.getNotes(sessionId, { limit: 500 });
  if (notes.length > 0) {
    const NOTE_TOKEN_CAP = pipelineConfig.notes.tokenCap;
    const header = "## Agent Notes\n\nNotes you wrote during this session. These survive context distillation.\n\n";
    let tokenCount = estimateTokens(header);
    const noteLines: string[] = [];
    let dropped = 0;

    for (const note of notes) {
      const line = `- [${note.category}] ${note.content}`;
      const lineTokens = estimateTokens(line + "\n");
      if (tokenCount + lineTokens > NOTE_TOKEN_CAP) {
        dropped++;
        continue;
      }
      noteLines.push(line);
      tokenCount += lineTokens;
    }

    if (noteLines.length > 0) {
      const suffix = dropped > 0 ? `\n\n*${dropped} older notes stored but excluded from context (token budget).*` : "";
      systemPrompt.push({
        type: "text",
        text: header + noteLines.join("\n") + suffix,
      });
    }
  }

  // Context utilization — hoisted for both pressure warning and session metrics
  const msgCount = currentSession?.messageCount ?? 0;
  const contextTokens = services.config.agents.defaults.contextTokens;
  const utilization = contextTokens > 0
    ? Math.round(((currentSession?.tokenCountEstimate ?? 0) / contextTokens) * 100)
    : 0;

  // Context pressure warning — fires every turn at ≥75% utilization.
  // Replaces the improvised "save everything" panic loop with an explicit protocol.
  if (utilization >= 75) {
    const urgent = utilization >= 85;
    systemPrompt.push({
      type: "text",
      text:
        `## Context Pressure — Turn ${msgCount}\n\n` +
        `Context at ${utilization}% of window. Distillation auto-triggers at 90%.\n\n` +
        `Preserved automatically: facts, decisions, open items → long-term memory\n` +
        `Not preserved: uncommitted filesystem changes\n\n` +
        (urgent
          ? `Commit any uncommitted work now, then stop. Do not repeat save attempts — one commit is sufficient.`
          : `No action needed unless you have uncommitted file changes.`),
    });
  }

  // Session metrics + cost — injected every 8th turn for self-awareness
  if (msgCount > 0 && msgCount % 8 === 0) {
    const elapsed = currentSession?.createdAt
      ? Math.round((Date.now() - new Date(currentSession.createdAt).getTime()) / 60000)
      : 0;

    // Cost summary from usage records
    const usageRecords = services.store.getUsageForSession(sessionId);
    const totalInput = usageRecords.reduce((s, u) => s + u.inputTokens, 0);
    const totalOutput = usageRecords.reduce((s, u) => s + u.outputTokens, 0);
    const totalCache = usageRecords.reduce((s, u) => s + u.cacheReadTokens, 0);
    const lastTurn = usageRecords[usageRecords.length - 1];
    const lastTurnTokens = lastTurn ? `${lastTurn.inputTokens}in/${lastTurn.outputTokens}out` : "n/a";
    const cacheRate = totalInput > 0 ? Math.round((totalCache / totalInput) * 100) : 0;

    systemPrompt.push({
      type: "text",
      text:
        `## Session Metrics — Turn ${msgCount}\n\n` +
        `Session duration: ${elapsed} min\n` +
        `Context utilization: ${utilization}%\n` +
        `Distillations: ${currentSession?.distillationCount ?? 0}\n` +
        `Total tokens: ${totalInput}in / ${totalOutput}out (cache hit: ${cacheRate}%)\n` +
        `Last turn: ${lastTurnTokens}`,
    });
  }

  // Pre-turn competence suggestion — if this agent scores low in the detected domain
  // and another agent scores high, suggest delegation
  if (services.competence) {
    if (domain) {
      const agentScore = services.competence.getScore(nousId, domain);
      if (agentScore < 0.3) {
        const better = services.competence.bestAgentForDomain(domain, [nousId]);
        if (better && better.score > 0.6) {
          systemPrompt.push({
            type: "text",
            text:
              `## Competence Advisory\n\n` +
              `Agent **${better.nousId}** has higher competence in **${domain}** ` +
              `(score: ${better.score.toFixed(2)} vs your ${agentScore.toFixed(2)}).\n` +
              `Consider delegating via \`sessions_ask\` if this task requires domain expertise.`,
          });
        }
      }
    }
  }

  // Tool definitions
  const nous = state.nous;
  const toolDefs = services.tools.getDefinitions({
    sessionId,
    ...(nous.tools.allow.length > 0 ? { allow: nous.tools.allow } : {}),
    ...(nous.tools.deny.length > 0 ? { deny: nous.tools.deny } : {}),
    ...(msg.toolFilter?.length ? { toolFilter: msg.toolFilter } : {}),
  });

  state.systemPrompt = systemPrompt;
  state.toolDefs = toolDefs;

  // Return recallTokens + bootstrap data for history budget calculation
  const toolDefTokens = estimateToolDefTokens(toolDefs);
  const maxOutput = services.config.agents.defaults.maxOutputTokens;
  const historyBudget = Math.max(0, contextTokens - bootstrap.totalTokens - toolDefTokens - maxOutput - recallTokens);

  // Store historyBudget on state for history stage
  (state as TurnState & { _historyBudget: number })._historyBudget = historyBudget;

  return state;
}
