// Nous manager — lifecycle, routing, agent turn execution
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { SessionStore, type Message } from "../mneme/store.js";
import { ProviderRouter } from "../hermeneus/router.js";
import { estimateTokens, estimateToolDefTokens } from "../hermeneus/token-counter.js";
import { ToolRegistry, type ToolContext } from "../organon/registry.js";
import { assembleBootstrap } from "./bootstrap.js";
import { detectBootstrapDiff, logBootstrapDiff } from "./bootstrap-diff.js";
import { distillSession } from "../distillation/pipeline.js";
import { scoreComplexity, selectModel, selectTemperature, type ComplexityTier } from "../hermeneus/complexity.js";
import { paths } from "../taxis/paths.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import {
  resolveNous,
  resolveModel,
  resolveWorkspace,
  resolveDefaultNous,
} from "../taxis/loader.js";
import type {
  ContentBlock,
  ImageBlock,
  MessageParam,
  ToolUseBlock,
  UserContentBlock,
} from "../hermeneus/anthropic.js";
import type { PluginRegistry } from "../prostheke/registry.js";
import type { Watchdog } from "../daemon/watchdog.js";
import { TraceBuilder } from "./trace.js";
import { checkInputCircuitBreakers, checkResponseQuality } from "./circuit-breaker.js";
import { getReversibility, requiresSimulation } from "../organon/reversibility.js";
import type { CompetenceModel } from "./competence.js";
import type { UncertaintyTracker } from "./uncertainty.js";
import { eventBus } from "../koina/event-bus.js";
import { classifyInteraction } from "./interaction-signals.js";
import { extractSkillCandidate, saveLearnedSkill, type ToolCallRecord } from "../organon/skill-learner.js";

const log = createLogger("nous");

export interface MediaAttachment {
  contentType: string;
  data: string;
  filename?: string;
}

export interface InboundMessage {
  text: string;
  nousId?: string;
  sessionKey?: string;
  parentSessionId?: string;
  channel?: string;
  peerId?: string;
  peerKind?: string;
  accountId?: string;
  media?: MediaAttachment[];
  model?: string;
  depth?: number;
}

export interface TurnOutcome {
  text: string;
  nousId: string;
  sessionId: string;
  toolCalls: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

export type TurnStreamEvent =
  | { type: "turn_start"; sessionId: string; nousId: string }
  | { type: "text_delta"; text: string }
  | { type: "tool_start"; toolName: string; toolId: string }
  | { type: "tool_result"; toolName: string; result: string; isError: boolean; durationMs: number }
  | { type: "turn_complete"; outcome: TurnOutcome }
  | { type: "error"; message: string };

// Per-session mutex to prevent concurrent turns from corrupting context
const sessionLocks = new Map<string, Promise<unknown>>();

function withSessionLock<T>(sessionId: string, fn: () => Promise<T>): Promise<T> {
  const previous = sessionLocks.get(sessionId) ?? Promise.resolve();
  const current = previous.then(fn, fn);
  sessionLocks.set(sessionId, current);
  // Suppress unhandled rejection from the .finally() chain — the caller handles the original
  current.finally(() => {
    if (sessionLocks.get(sessionId) === current) {
      sessionLocks.delete(sessionId);
    }
  }).catch(() => {});
  return current;
}

// Ephemeral timestamp formatting — absolute times in operator timezone
// Injected at API-call time only, never stored. Uses absolute format because
// relative time ("yesterday") becomes inaccurate as conversations age.
function formatEphemeralTimestamp(isoString: string, tz: string = "UTC"): string | null {
  try {
    const d = new Date(isoString);
    if (isNaN(d.getTime())) return null;
    return d.toLocaleString("en-US", {
      timeZone: tz,
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });
  } catch {
    return null;
  }
}

export class NousManager {
  private plugins?: PluginRegistry;
  private watchdog?: Watchdog;
  private skillsSection?: string | undefined;
  competence?: CompetenceModel;
  uncertainty?: UncertaintyTracker;
  activeTurns = 0;
  isDraining: () => boolean = () => false;

  constructor(
    private config: AletheiaConfig,
    private store: SessionStore,
    private router: ProviderRouter,
    private tools: ToolRegistry,
  ) {
    log.info(
      `NousManager initialized with ${config.agents.list.length} nous`,
    );
  }

  setPlugins(plugins: PluginRegistry): void {
    this.plugins = plugins;
  }

  setWatchdog(watchdog: Watchdog): void {
    this.watchdog = watchdog;
  }

  setSkillsSection(section: string | undefined): void {
    this.skillsSection = section;
  }

  setCompetence(model: CompetenceModel): void {
    this.competence = model;
  }

  setUncertainty(tracker: UncertaintyTracker): void {
    this.uncertainty = tracker;
  }

  async *handleMessageStreaming(msg: InboundMessage): AsyncGenerator<TurnStreamEvent> {
    if (this.isDraining()) {
      yield { type: "error", message: "Runtime is shutting down" };
      return;
    }

    const maxDepth = this.config.session.agentToAgent.maxPingPongTurns;
    if (msg.depth && msg.depth >= maxDepth) {
      yield { type: "error", message: `Cross-agent depth limit (${maxDepth}) exceeded` };
      return;
    }

    const nousId = this.resolveNousId(msg);
    const nous = resolveNous(this.config, nousId);
    if (!nous) {
      yield { type: "error", message: `Unknown nous: ${nousId}` };
      return;
    }

    const sessionKey = msg.sessionKey ?? "main";
    let model = msg.model ?? resolveModel(this.config, nous);

    let temperature: number | undefined;
    const routing = this.config.agents.defaults.routing;
    if (routing.enabled && !msg.model) {
      const session = this.store.findSession(nousId, sessionKey);
      const override = routing.agentOverrides[nousId] as ComplexityTier | undefined;
      const complexity = scoreComplexity({
        messageText: msg.text,
        messageCount: session?.messageCount ?? 0,
        depth: msg.depth ?? 0,
        ...(override ? { agentOverride: override } : {}),
      });
      model = selectModel(complexity.tier, routing.tiers);
      temperature = selectTemperature(complexity.tier, this.tools.hasTools());
    }

    const session = this.store.findOrCreateSession(nousId, sessionKey, model, msg.parentSessionId);
    yield { type: "turn_start", sessionId: session.id, nousId };

    this.activeTurns++;
    try {
      // Collect events from executeTurnStreaming through session lock
      // We can't yield directly from withSessionLock, so collect into a buffer
      const events: TurnStreamEvent[] = [];
      await withSessionLock(session.id, async () => {
        for await (const event of this.executeTurnStreaming(
          nousId, session.id, sessionKey, model, msg, nous, temperature,
        )) {
          events.push(event);
        }
      });
      for (const event of events) {
        yield event;
      }
    } catch (err) {
      yield { type: "error", message: err instanceof Error ? err.message : String(err) };
    } finally {
      this.activeTurns--;
    }
  }

  private async *executeTurnStreaming(
    nousId: string,
    sessionId: string,
    sessionKey: string,
    model: string,
    msg: InboundMessage,
    nous: ReturnType<typeof resolveNous>,
    temperature?: number,
  ): AsyncGenerator<TurnStreamEvent> {
    if (!nous) throw new Error(`Unknown nous: ${nousId}`);

    const inputCheck = checkInputCircuitBreakers(msg.text);
    if (inputCheck.triggered) {
      log.warn(`Circuit breaker (${inputCheck.severity}): ${inputCheck.reason} [${nousId}]`);
      this.store.appendMessage(sessionId, "user", msg.text, { tokenEstimate: estimateTokens(msg.text) });
      const refusal = `I can't process that request. ${inputCheck.reason}`;
      this.store.appendMessage(sessionId, "assistant", refusal, { tokenEstimate: estimateTokens(refusal) });
      yield { type: "text_delta", text: refusal };
      yield {
        type: "turn_complete",
        outcome: { text: refusal, nousId, sessionId, toolCalls: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 },
      };
      return;
    }

    eventBus.emit("turn:before", { nousId, sessionId, sessionKey, channel: msg.channel });
    log.info(`Processing streaming message for ${nousId}:${sessionKey} (session ${sessionId})`);

    const workspace = resolveWorkspace(this.config, nous);
    const degradedServices: string[] = [];
    if (this.watchdog) {
      for (const svc of this.watchdog.getStatus()) {
        if (!svc.healthy) degradedServices.push(svc.name);
      }
    }

    const bootstrap = assembleBootstrap(workspace, {
      maxTokens: this.config.agents.defaults.bootstrapMaxTokens,
      ...(this.skillsSection ? { skillsSection: this.skillsSection } : {}),
      ...(degradedServices.length > 0 ? { degradedServices } : {}),
    });

    this.store.updateBootstrapHash(sessionId, bootstrap.contentHash);
    const diff = detectBootstrapDiff(nousId, bootstrap.fileHashes, workspace);
    if (diff) logBootstrapDiff(diff, workspace);

    const trace = new TraceBuilder(sessionId, nousId, 0, model);
    trace.setBootstrap(Object.keys(bootstrap.fileHashes), bootstrap.totalTokens);
    if (degradedServices.length > 0) trace.setDegradedServices(degradedServices);

    const systemPrompt: Array<{ type: "text"; text: string; cache_control?: { type: "ephemeral" } }> = [
      ...bootstrap.staticBlocks,
      ...bootstrap.dynamicBlocks,
    ];

    const broadcasts = this.store.blackboardReadPrefix("broadcast:");
    if (broadcasts.length > 0) {
      const broadcastLines = broadcasts
        .slice(0, 5)
        .map((b) => `- **[${b.key.replace("broadcast:", "")}]** ${b.value.slice(0, 300)}`)
        .join("\n");
      systemPrompt.push({ type: "text", text: `## Broadcasts\n\n${broadcastLines}` });
    }

    const currentSession = this.store.findSessionById(sessionId);
    const msgCount = currentSession?.messageCount ?? 0;
    if (msgCount > 0 && msgCount % 8 === 0) {
      const recentTools = this.store.getRecentToolCalls(sessionId, 6);
      const elapsed = currentSession?.createdAt
        ? Math.round((Date.now() - new Date(currentSession.createdAt).getTime()) / 60000)
        : 0;
      const utilization = this.config.agents.defaults.contextTokens > 0
        ? Math.round(((currentSession?.tokenCountEstimate ?? 0) / this.config.agents.defaults.contextTokens) * 100)
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

    const toolDefs = this.tools.getDefinitions({
      sessionId,
      ...(nous.tools.allow.length > 0 ? { allow: nous.tools.allow } : {}),
      ...(nous.tools.deny.length > 0 ? { deny: nous.tools.deny } : {}),
    });

    const toolDefTokens = estimateToolDefTokens(toolDefs);
    const contextTokens = this.config.agents.defaults.contextTokens;
    const maxOutput = this.config.agents.defaults.maxOutputTokens;
    const historyBudget = Math.max(0, contextTokens - bootstrap.totalTokens - toolDefTokens - maxOutput);
    const history = this.store.getHistoryWithBudget(sessionId, historyBudget);

    let crossAgentNotice: string | null = null;
    const unsurfaced = this.store.getUnsurfacedMessages(nousId);
    if (unsurfaced.length > 0) {
      const lines = unsurfaced.map((m) => {
        const from = m.sourceNousId ?? "unknown";
        const summary = m.response ? `\n  Response: ${m.response.slice(0, 500)}` : "";
        return `[From ${from}, ${m.kind}] ${m.content}${summary}`;
      });
      crossAgentNotice =
        `While you were in another conversation, you received cross-agent messages:\n\n` +
        lines.join("\n\n") +
        `\n\nThe user may not be aware of these. Mention them if relevant.`;
      this.store.appendMessage(sessionId, "user", crossAgentNotice, { tokenEstimate: estimateTokens(crossAgentNotice) });
      this.store.markMessagesSurfaced(unsurfaced.map((m) => m.id), sessionId);
    }

    const seq = this.store.appendMessage(sessionId, "user", msg.text, { tokenEstimate: estimateTokens(msg.text) });

    const currentText = crossAgentNotice ? crossAgentNotice + "\n\n" + msg.text : msg.text;
    const messages = this.buildMessages(history, currentText, msg.media, nous["userTimezone"] as string | undefined);

    const toolContext: ToolContext = { nousId, sessionId, workspace, allowedRoots: [paths.root], depth: msg.depth ?? 0 };

    if (this.plugins) {
      await this.plugins.dispatchBeforeTurn({ nousId, sessionId, messageText: msg.text, ...(msg.media ? { media: msg.media } : {}) });
    }

    let totalToolCalls = 0;
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let totalCacheReadTokens = 0;
    let totalCacheWriteTokens = 0;
    let currentMessages = messages;
    const turnToolCalls: ToolCallRecord[] = [];

    const MAX_TOOL_LOOPS = 20;
    for (let loop = 0; loop < MAX_TOOL_LOOPS; loop++) {
      // Stream the completion
      let accumulatedText = "";
      let streamResult: import("../hermeneus/anthropic.js").TurnResult | null = null;

      for await (const streamEvent of this.router.completeStreaming({
        model,
        system: systemPrompt,
        messages: currentMessages,
        ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
        maxTokens: this.config.agents.defaults.maxOutputTokens,
        ...(temperature !== undefined ? { temperature } : {}),
      })) {
        switch (streamEvent.type) {
          case "text_delta":
            accumulatedText += streamEvent.text;
            yield { type: "text_delta", text: streamEvent.text };
            break;
          case "tool_use_start":
            yield { type: "tool_start", toolName: streamEvent.name, toolId: streamEvent.id };
            break;
          case "message_complete":
            streamResult = streamEvent.result;
            break;
        }
      }

      if (!streamResult) throw new Error("Stream ended without message_complete");

      totalInputTokens += streamResult.usage.inputTokens;
      totalOutputTokens += streamResult.usage.outputTokens;
      totalCacheReadTokens += streamResult.usage.cacheReadTokens;
      totalCacheWriteTokens += streamResult.usage.cacheWriteTokens;

      this.store.recordUsage({
        sessionId,
        turnSeq: seq + loop,
        inputTokens: streamResult.usage.inputTokens,
        outputTokens: streamResult.usage.outputTokens,
        cacheReadTokens: streamResult.usage.cacheReadTokens,
        cacheWriteTokens: streamResult.usage.cacheWriteTokens,
        model: streamResult.model,
      });

      const toolUses = streamResult.content.filter(
        (b): b is ToolUseBlock => b.type === "tool_use",
      );

      if (toolUses.length === 0) {
        const text = accumulatedText || streamResult.content
          .filter((b): b is { type: "text"; text: string } => b.type === "text")
          .map((b) => b.text)
          .join("\n");

        const qualityCheck = checkResponseQuality(text);
        if (qualityCheck.triggered) {
          log.warn(`Response quality issue (${qualityCheck.severity}): ${qualityCheck.reason} [${nousId}]`);
          trace.addToolCall({ name: "_circuit_breaker", input: { check: "response_quality" }, output: qualityCheck.reason ?? "quality check triggered", durationMs: 0, isError: true });
        }

        this.store.appendMessage(sessionId, "assistant", text, { tokenEstimate: estimateTokens(text) });

        const outcome: TurnOutcome = {
          text, nousId, sessionId, toolCalls: totalToolCalls,
          inputTokens: totalInputTokens, outputTokens: totalOutputTokens,
          cacheReadTokens: totalCacheReadTokens, cacheWriteTokens: totalCacheWriteTokens,
        };

        trace.setUsage(totalInputTokens, totalOutputTokens, totalCacheReadTokens, totalCacheWriteTokens);
        trace.setResponseLength(text.length);
        trace.setToolLoops(loop + 1);
        const finalTrace = trace.finalize();
        (await import("./trace.js")).persistTrace(finalTrace, workspace);

        this.store.updateSessionActualTokens(sessionId, totalInputTokens);

        if (this.plugins) {
          await this.plugins.dispatchAfterTurn({
            nousId, sessionId, responseText: text, messageText: msg.text,
            toolCalls: totalToolCalls, inputTokens: totalInputTokens, outputTokens: totalOutputTokens,
          });
        }

        eventBus.emit("turn:after", { nousId, sessionId, toolCalls: totalToolCalls, inputTokens: totalInputTokens, outputTokens: totalOutputTokens });

        const signal = classifyInteraction(msg.text, text);
        this.store.recordSignal({ sessionId, nousId, turnSeq: seq, signal: signal.signal, confidence: signal.confidence });
        if (signal.signal === "correction" && this.competence) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          this.competence.recordCorrection(nousId, domain);
        }

        if (this.competence && totalToolCalls > 0) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          this.competence.recordSuccess(nousId, domain);
        }

        if (turnToolCalls.length >= 3) {
          const skillModel = this.config.agents.defaults.compaction.distillationModel;
          const skillsDir = join(resolveWorkspace(this.config, nous)!, "..", "..", "shared", "skills");
          extractSkillCandidate(this.router, turnToolCalls, skillModel, sessionId, seq, nousId)
            .then((candidate) => { if (candidate) saveLearnedSkill(candidate, skillsDir); })
            .catch(() => {});
        }

        this.tools.expireUnusedTools(sessionId, seq + loop);

        const compaction = this.config.agents.defaults.compaction;
        const distillThreshold = Math.floor(contextTokens * compaction.maxHistoryShare);
        try {
          const sess = this.store.findSessionById(sessionId);
          const actualContext = sess?.lastInputTokens ?? sess?.tokenCountEstimate ?? 0;
          if (sess && sess.messageCount >= 10 && actualContext >= distillThreshold) {
            const distillModel = compaction.distillationModel;
            await distillSession(this.store, this.router, sessionId, nousId, {
              triggerThreshold: distillThreshold, minMessages: 10,
              extractionModel: distillModel, summaryModel: distillModel,
              ...(this.plugins ? { plugins: this.plugins } : {}),
            });
          }
        } catch (err) {
          log.warn(`Distillation failed: ${err instanceof Error ? err.message : err}`);
        }

        yield { type: "turn_complete", outcome };
        return;
      }

      // Store assistant tool_use response
      this.store.appendMessage(sessionId, "assistant", JSON.stringify(streamResult.content), {
        tokenEstimate: estimateTokens(JSON.stringify(streamResult.content)),
      });

      currentMessages = [
        ...currentMessages,
        { role: "assistant" as const, content: streamResult.content as ContentBlock[] },
      ];

      // Execute tools
      const toolResults: UserContentBlock[] = [];
      for (const toolUse of toolUses) {
        totalToolCalls++;
        const reversibility = getReversibility(toolUse.name);
        const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);

        let toolResult: string;
        let isError = false;
        const toolStart = Date.now();
        try {
          toolResult = await this.tools.execute(toolUse.name, toolUse.input, toolContext);
        } catch (err) {
          isError = true;
          toolResult = err instanceof Error ? err.message : String(err);
          if (this.competence) {
            const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
            this.competence.recordCorrection(nousId, domain);
          }
        }
        const toolDuration = Date.now() - toolStart;

        yield {
          type: "tool_result",
          toolName: toolUse.name,
          result: toolResult.slice(0, 2000),
          isError,
          durationMs: toolDuration,
        };

        if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
        this.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
        eventBus.emit(isError ? "tool:failed" : "tool:called", {
          nousId, sessionId, tool: toolUse.name, durationMs: toolDuration,
          ...(isError ? { error: toolResult.slice(0, 200) } : {}),
        });

        trace.addToolCall({
          name: toolUse.name, input: toolUse.input as Record<string, unknown>,
          output: toolResult.slice(0, 500), durationMs: toolDuration, isError,
          ...(reversibility !== "reversible" ? { reversibility } : {}),
          ...(needsSim ? { simulationRequired: true } : {}),
        });

        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id, content: toolResult,
          ...(isError ? { is_error: true } : {}),
        });

        this.store.appendMessage(sessionId, "tool_result", toolResult, {
          toolCallId: toolUse.id, toolName: toolUse.name, tokenEstimate: estimateTokens(toolResult),
        });
      }

      currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
    }

    yield { type: "error", message: "Max tool loops exceeded" };
  }

  async handleMessage(msg: InboundMessage): Promise<TurnOutcome> {
    if (this.isDraining()) {
      throw new Error("Runtime is shutting down — rejecting new messages");
    }

    const maxDepth = this.config.session.agentToAgent.maxPingPongTurns;
    if (msg.depth && msg.depth >= maxDepth) {
      throw new Error(`Cross-agent depth limit (${maxDepth}) exceeded`);
    }

    const nousId = this.resolveNousId(msg);
    const nous = resolveNous(this.config, nousId);
    if (!nous) {
      throw new Error(`Unknown nous: ${nousId}`);
    }

    const sessionKey = msg.sessionKey ?? "main";
    let model = msg.model ?? resolveModel(this.config, nous);

    // Adaptive inference routing — select model tier based on message complexity
    let temperature: number | undefined;
    const routing = this.config.agents.defaults.routing;
    if (routing.enabled && !msg.model) {
      const session = this.store.findSession(nousId, sessionKey);
      const override = routing.agentOverrides[nousId] as ComplexityTier | undefined;
      const complexity = scoreComplexity({
        messageText: msg.text,
        messageCount: session?.messageCount ?? 0,
        depth: msg.depth ?? 0,
        ...(override ? { agentOverride: override } : {}),
      });
      model = selectModel(complexity.tier, routing.tiers);
      temperature = selectTemperature(complexity.tier, this.tools.hasTools());
      log.info(
        `Routing ${nousId}: ${complexity.tier} (score=${complexity.score}, ${complexity.reason}) → ${model} temp=${temperature}`,
      );
    }

    const session = this.store.findOrCreateSession(
      nousId,
      sessionKey,
      model,
      msg.parentSessionId,
    );

    // Serialize concurrent turns on the same session
    this.activeTurns++;
    try {
      return await withSessionLock(session.id, () =>
        this.executeTurn(nousId, session.id, sessionKey, model, msg, nous, temperature),
      );
    } finally {
      this.activeTurns--;
    }
  }

  private async executeTurn(
    nousId: string,
    sessionId: string,
    sessionKey: string,
    model: string,
    msg: InboundMessage,
    nous: ReturnType<typeof resolveNous>,
    temperature?: number,
  ): Promise<TurnOutcome> {
    if (!nous) throw new Error(`Unknown nous: ${nousId}`);

    // Input circuit breaker — block safety-violating messages before any processing
    const inputCheck = checkInputCircuitBreakers(msg.text);
    if (inputCheck.triggered) {
      log.warn(`Circuit breaker (${inputCheck.severity}): ${inputCheck.reason} [${nousId}]`);
      this.store.appendMessage(sessionId, "user", msg.text, {
        tokenEstimate: estimateTokens(msg.text),
      });
      const refusal = `I can't process that request. ${inputCheck.reason}`;
      this.store.appendMessage(sessionId, "assistant", refusal, {
        tokenEstimate: estimateTokens(refusal),
      });
      return {
        text: refusal,
        nousId,
        sessionId,
        toolCalls: 0,
        inputTokens: 0,
        outputTokens: 0,
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
      };
    }

    eventBus.emit("turn:before", { nousId, sessionId, sessionKey, channel: msg.channel });

    log.info(
      `Processing message for ${nousId}:${sessionKey} (session ${sessionId})`,
    );

    const workspace = resolveWorkspace(this.config, nous);

    // Check watchdog for degraded services — inject into bootstrap
    const degradedServices: string[] = [];
    if (this.watchdog) {
      for (const svc of this.watchdog.getStatus()) {
        if (!svc.healthy) degradedServices.push(svc.name);
      }
    }

    const bootstrap = assembleBootstrap(workspace, {
      maxTokens: this.config.agents.defaults.bootstrapMaxTokens,
      ...(this.skillsSection ? { skillsSection: this.skillsSection } : {}),
      ...(degradedServices.length > 0 ? { degradedServices } : {}),
    });

    // Store composite hash and detect file-level diffs
    this.store.updateBootstrapHash(sessionId, bootstrap.contentHash);
    const diff = detectBootstrapDiff(nousId, bootstrap.fileHashes, workspace);
    if (diff) logBootstrapDiff(diff, workspace);

    if (bootstrap.droppedFiles.length > 0) {
      log.warn(`Bootstrap for ${nousId} dropped files due to budget: ${bootstrap.droppedFiles.join(", ")}`);
    }

    // Initialize causal trace for this turn
    const trace = new TraceBuilder(sessionId, nousId, 0, model);
    trace.setBootstrap(
      Object.keys(bootstrap.fileHashes),
      bootstrap.totalTokens,
    );
    if (degradedServices.length > 0) {
      trace.setDegradedServices(degradedServices);
    }

    const systemPrompt: Array<{ type: "text"; text: string; cache_control?: { type: "ephemeral" } }> = [
      ...bootstrap.staticBlocks,
      ...bootstrap.dynamicBlocks,
    ];

    // Broadcast injection — prosoche and agents can post attention-worthy items via blackboard
    const broadcasts = this.store.blackboardReadPrefix("broadcast:");
    if (broadcasts.length > 0) {
      const broadcastLines = broadcasts
        .slice(0, 5)
        .map((b) => `- **[${b.key.replace("broadcast:", "")}]** ${b.value.slice(0, 300)}`)
        .join("\n");
      systemPrompt.push({
        type: "text",
        text: `## Broadcasts\n\n${broadcastLines}`,
      });
    }

    // Mid-session working state injection — every 8 turns, give the agent a lightweight status pulse
    const currentSession = this.store.findSessionById(sessionId);
    const msgCount = currentSession?.messageCount ?? 0;
    if (msgCount > 0 && msgCount % 8 === 0) {
      const recentTools = this.store.getRecentToolCalls(sessionId, 6);
      const elapsed = currentSession?.createdAt
        ? Math.round((Date.now() - new Date(currentSession.createdAt).getTime()) / 60000)
        : 0;
      const utilization = this.config.agents.defaults.contextTokens > 0
        ? Math.round(((currentSession?.tokenCountEstimate ?? 0) / this.config.agents.defaults.contextTokens) * 100)
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

    const toolDefs = this.tools.getDefinitions({
      sessionId,
      ...(nous.tools.allow.length > 0 ? { allow: nous.tools.allow } : {}),
      ...(nous.tools.deny.length > 0 ? { deny: nous.tools.deny } : {}),
    });

    // Tool definitions count toward the input token budget — estimate with safety margin
    const toolDefTokens = estimateToolDefTokens(toolDefs);

    const contextTokens = this.config.agents.defaults.contextTokens;
    const maxOutput = this.config.agents.defaults.maxOutputTokens;
    const historyBudget = Math.max(0, contextTokens - bootstrap.totalTokens - toolDefTokens - maxOutput);

    const history = this.store.getHistoryWithBudget(sessionId, historyBudget);

    // Surface unsurfaced cross-agent messages into this session
    let crossAgentNotice: string | null = null;
    const unsurfaced = this.store.getUnsurfacedMessages(nousId);
    if (unsurfaced.length > 0) {
      const lines = unsurfaced.map((m) => {
        const from = m.sourceNousId ?? "unknown";
        const summary = m.response ? `\n  Response: ${m.response.slice(0, 500)}` : "";
        return `[From ${from}, ${m.kind}] ${m.content}${summary}`;
      });
      crossAgentNotice =
        `While you were in another conversation, you received cross-agent messages:\n\n` +
        lines.join("\n\n") +
        `\n\nThe user may not be aware of these. Mention them if relevant.`;

      this.store.appendMessage(sessionId, "user", crossAgentNotice, {
        tokenEstimate: estimateTokens(crossAgentNotice),
      });
      this.store.markMessagesSurfaced(
        unsurfaced.map((m) => m.id),
        sessionId,
      );
      log.info(`Surfaced ${unsurfaced.length} cross-agent messages into session ${sessionId}`);
    }

    const seq = this.store.appendMessage(sessionId, "user", msg.text, {
      tokenEstimate: estimateTokens(msg.text),
    });

    // Build messages from history, injecting any cross-agent notice before current text.
    // The notice was stored in DB but history was fetched before it was appended,
    // so we must inject it manually for the current turn.
    const currentText = crossAgentNotice
      ? crossAgentNotice + "\n\n" + msg.text
      : msg.text;
    const messages = this.buildMessages(history, currentText, msg.media, nous["userTimezone"] as string | undefined);

    const toolContext: ToolContext = {
      nousId,
      sessionId,
      workspace,
      allowedRoots: [paths.root],
      depth: msg.depth ?? 0,
    };

    if (this.plugins) {
      await this.plugins.dispatchBeforeTurn({
        nousId,
        sessionId,
        messageText: msg.text,
        ...(msg.media ? { media: msg.media } : {}),
      });
    }

    let totalToolCalls = 0;
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let totalCacheReadTokens = 0;
    let totalCacheWriteTokens = 0;
    let currentMessages = messages;
    const turnToolCalls: ToolCallRecord[] = [];

    const MAX_TOOL_LOOPS = 20;
    for (let loop = 0; loop < MAX_TOOL_LOOPS; loop++) {
      const result = await this.router.complete({
        model,
        system: systemPrompt,
        messages: currentMessages,
        ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
        maxTokens: this.config.agents.defaults.maxOutputTokens,
        ...(temperature !== undefined ? { temperature } : {}),
      });

      totalInputTokens += result.usage.inputTokens;
      totalOutputTokens += result.usage.outputTokens;
      totalCacheReadTokens += result.usage.cacheReadTokens;
      totalCacheWriteTokens += result.usage.cacheWriteTokens;

      this.store.recordUsage({
        sessionId,
        turnSeq: seq + loop,
        inputTokens: result.usage.inputTokens,
        outputTokens: result.usage.outputTokens,
        cacheReadTokens: result.usage.cacheReadTokens,
        cacheWriteTokens: result.usage.cacheWriteTokens,
        model: result.model,
      });

      const toolUses = result.content.filter(
        (b): b is ToolUseBlock => b.type === "tool_use",
      );

      // Only exit when there are no tool calls — don't check stopReason
      // (Anthropic can return end_turn with tool_use blocks in the same response)
      if (toolUses.length === 0) {
        const text = result.content
          .filter((b): b is { type: "text"; text: string } => b.type === "text")
          .map((b) => b.text)
          .join("\n");

        // Response quality circuit breaker — detect generation loops and low-substance responses
        const qualityCheck = checkResponseQuality(text);
        if (qualityCheck.triggered) {
          log.warn(`Response quality issue (${qualityCheck.severity}): ${qualityCheck.reason} [${nousId}]`);
          trace.addToolCall({
            name: "_circuit_breaker",
            input: { check: "response_quality" },
            output: qualityCheck.reason ?? "quality check triggered",
            durationMs: 0,
            isError: true,
          });
        }

        this.store.appendMessage(sessionId, "assistant", text, {
          tokenEstimate: estimateTokens(text),
        });

        const outcome: TurnOutcome = {
          text,
          nousId,
          sessionId,
          toolCalls: totalToolCalls,
          inputTokens: totalInputTokens,
          outputTokens: totalOutputTokens,
          cacheReadTokens: totalCacheReadTokens,
          cacheWriteTokens: totalCacheWriteTokens,
        };

        // Finalize and persist causal trace
        trace.setUsage(totalInputTokens, totalOutputTokens, totalCacheReadTokens, totalCacheWriteTokens);
        trace.setResponseLength(text.length);
        trace.setToolLoops(loop + 1);
        const finalTrace = trace.finalize();
        (await import("./trace.js")).persistTrace(finalTrace, workspace);

        const cacheHitRate = totalInputTokens > 0
          ? Math.round((totalCacheReadTokens / totalInputTokens) * 100)
          : 0;
        log.info(
          `Turn complete for ${nousId}: ${totalInputTokens}in/${totalOutputTokens}out, ` +
          `cache ${totalCacheReadTokens}r/${totalCacheWriteTokens}w (${cacheHitRate}% hit), ` +
          `${totalToolCalls} tool calls`,
        );

        // Store actual API-reported context consumption for accurate distillation triggering
        this.store.updateSessionActualTokens(sessionId, totalInputTokens);

        if (this.plugins) {
          await this.plugins.dispatchAfterTurn({
            nousId,
            sessionId,
            responseText: text,
            messageText: msg.text,
            toolCalls: totalToolCalls,
            inputTokens: totalInputTokens,
            outputTokens: totalOutputTokens,
          });
        }

        eventBus.emit("turn:after", { nousId, sessionId, toolCalls: totalToolCalls, inputTokens: totalInputTokens, outputTokens: totalOutputTokens });

        // Classify interaction signal and record
        const signal = classifyInteraction(msg.text, text);
        this.store.recordSignal({ sessionId, nousId, turnSeq: seq, signal: signal.signal, confidence: signal.confidence });
        if (signal.signal === "correction" && this.competence) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          this.competence.recordCorrection(nousId, domain);
        }

        // Record competence success for the agent's domain (session key as proxy)
        if (this.competence && totalToolCalls > 0) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          this.competence.recordSuccess(nousId, domain);
        }

        // Skill learning — extract reusable patterns from successful multi-tool turns
        if (turnToolCalls.length >= 3) {
          const skillModel = this.config.agents.defaults.compaction.distillationModel;
          const skillsDir = join(resolveWorkspace(this.config, nous)!, "..", "..", "shared", "skills");
          extractSkillCandidate(this.router, turnToolCalls, skillModel, sessionId, seq, nousId)
            .then((candidate) => {
              if (candidate) saveLearnedSkill(candidate, skillsDir);
            })
            .catch(() => {}); // Fire-and-forget
        }

        // Expire unused dynamic tools
        this.tools.expireUnusedTools(sessionId, seq + loop);

        // Auto-trigger distillation using actual API-reported input tokens (most accurate)
        // Falls back to heuristic estimate if actual tokens aren't available
        const compaction = this.config.agents.defaults.compaction;
        const distillThreshold = Math.floor(contextTokens * compaction.maxHistoryShare);
        try {
          const session = this.store.findSessionById(sessionId);
          const actualContext = session?.lastInputTokens ?? session?.tokenCountEstimate ?? 0;
          if (session && session.messageCount >= 10 && actualContext >= distillThreshold) {
            const utilization = Math.round((actualContext / contextTokens) * 100);
            log.info(
              `Distillation triggered for ${nousId} session=${sessionId} ` +
              `(${utilization}% context, threshold=${Math.round(compaction.maxHistoryShare * 100)}%, ` +
              `actual=${actualContext} tokens)`,
            );
            const distillModel = compaction.distillationModel;
            await distillSession(this.store, this.router, sessionId, nousId, {
              triggerThreshold: distillThreshold,
              minMessages: 10,
              extractionModel: distillModel,
              summaryModel: distillModel,
              ...(this.plugins ? { plugins: this.plugins } : {}),
            });
          }
        } catch (err) {
          log.warn(`Distillation failed: ${err instanceof Error ? err.message : err}`);
        }

        return outcome;
      }

      // Store the assistant's tool_use response as JSON for history replay
      this.store.appendMessage(
        sessionId,
        "assistant",
        JSON.stringify(result.content),
        { tokenEstimate: estimateTokens(JSON.stringify(result.content)) },
      );

      currentMessages = [
        ...currentMessages,
        {
          role: "assistant" as const,
          content: result.content as ContentBlock[],
        },
      ];

      const toolResults: UserContentBlock[] = [];
      for (const toolUse of toolUses) {
        totalToolCalls++;
        const reversibility = getReversibility(toolUse.name);
        const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);
        log.debug(`Tool call: ${toolUse.name} (${reversibility}${needsSim ? ", SIMULATED" : ""})`);

        if (needsSim) {
          log.warn(`Simulation required for ${toolUse.name} (${reversibility}) — logging to trace`);
        }

        let toolResult: string;
        let isError = false;
        const toolStart = Date.now();
        try {
          toolResult = await this.tools.execute(
            toolUse.name,
            toolUse.input,
            toolContext,
          );
        } catch (err) {
          isError = true;
          toolResult = err instanceof Error ? err.message : String(err);
          log.warn(`Tool ${toolUse.name} failed: ${toolResult}`);
          // Record tool failure in competence model
          if (this.competence) {
            const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
            this.competence.recordCorrection(nousId, domain);
          }
        }
        const toolDuration = Date.now() - toolStart;

        if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
        this.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
        eventBus.emit(isError ? "tool:failed" : "tool:called", {
          nousId, sessionId, tool: toolUse.name, durationMs: toolDuration,
          ...(isError ? { error: toolResult.slice(0, 200) } : {}),
        });

        trace.addToolCall({
          name: toolUse.name,
          input: toolUse.input as Record<string, unknown>,
          output: toolResult.slice(0, 500),
          durationMs: toolDuration,
          isError,
          ...(reversibility !== "reversible" ? { reversibility } : {}),
          ...(needsSim ? { simulationRequired: true } : {}),
        });

        toolResults.push({
          type: "tool_result",
          tool_use_id: toolUse.id,
          content: toolResult,
          ...(isError ? { is_error: true } : {}),
        });

        this.store.appendMessage(sessionId, "tool_result", toolResult, {
          toolCallId: toolUse.id,
          toolName: toolUse.name,
          tokenEstimate: estimateTokens(toolResult),
        });
      }

      currentMessages = [
        ...currentMessages,
        {
          role: "user" as const,
          content: toolResults,
        },
      ];
    }

    throw new Error("Max tool loops exceeded");
  }

  private resolveNousId(msg: InboundMessage): string {
    if (msg.nousId) return msg.nousId;

    if (msg.channel && msg.peerKind && msg.peerId) {
      const routed = this.store.resolveRoute(
        msg.channel,
        msg.peerKind,
        msg.peerId,
        msg.accountId,
      );
      if (routed) return routed;
    }

    const defaultNous = resolveDefaultNous(this.config);
    return defaultNous?.id ?? "syn";
  }

  private buildMessages(
    history: Message[],
    currentText: string,
    media?: MediaAttachment[],
    tz?: string,
  ): MessageParam[] {
    const messages: MessageParam[] = [];

    for (let i = 0; i < history.length; i++) {
      const msg = history[i]!;

      if (msg.role === "user") {
        // Ephemeral timestamps — inject absolute time for temporal awareness
        // These exist only in the API call, never stored
        const ts = formatEphemeralTimestamp(msg.createdAt, tz);
        const content = ts ? `[${ts}] ${msg.content}` : msg.content;
        messages.push({ role: "user", content });
      } else if (msg.role === "assistant") {
        // Try parsing as JSON content blocks (tool_use responses stored as JSON)
        try {
          const parsed = JSON.parse(msg.content);
          if (Array.isArray(parsed) && parsed.length > 0 && parsed[0]?.type) {
            messages.push({
              role: "assistant",
              content: parsed as ContentBlock[],
            });
            continue;
          }
        } catch {
          // Not JSON — plain text assistant message
        }
        messages.push({ role: "assistant", content: msg.content });
      } else if (msg.role === "tool_result") {
        // Group consecutive tool_results into a single user message
        const toolResults: UserContentBlock[] = [];
        while (i < history.length && history[i]!.role === "tool_result") {
          const tr = history[i]!;
          toolResults.push({
            type: "tool_result",
            tool_use_id: tr.toolCallId ?? "",
            content: tr.content,
          });
          i++;
        }
        i--; // Back up — for loop will increment

        // Validate: tool_results must follow an assistant message with matching tool_use blocks.
        // Old runtime stored tool_results without the preceding assistant tool_use — skip orphans.
        const prev = messages[messages.length - 1];
        if (prev?.role === "assistant" && Array.isArray(prev.content)) {
          const toolUseIds = new Set(
            (prev.content as ContentBlock[])
              .filter((b): b is ToolUseBlock => b.type === "tool_use")
              .map((b) => b.id),
          );
          const valid = toolResults.filter((tr) =>
            "tool_use_id" in tr && toolUseIds.has(tr.tool_use_id),
          );
          if (valid.length > 0) {
            messages.push({ role: "user", content: valid });
          } else {
            log.debug("Dropping orphaned tool_results (no matching tool_use)");
          }
        } else {
          log.debug("Dropping orphaned tool_results (no preceding assistant tool_use)");
        }
      }
    }

    // Current message — multimodal if images present
    const imageMedia = media?.filter((m) =>
      /^image\/(jpeg|png|gif|webp)$/i.test(m.contentType),
    );
    if (imageMedia && imageMedia.length > 0) {
      const blocks: UserContentBlock[] = [];
      for (const img of imageMedia) {
        // Strip data URI prefix if present
        let data = img.data;
        const dataUriMatch = data.match(/^data:[^;]+;base64,(.+)$/);
        if (dataUriMatch) data = dataUriMatch[1]!;

        blocks.push({
          type: "image",
          source: {
            type: "base64",
            media_type: img.contentType,
            data,
          },
        } as ImageBlock);
      }
      blocks.push({ type: "text", text: currentText });
      messages.push({ role: "user", content: blocks });
    } else {
      messages.push({ role: "user", content: currentText });
    }

    // Merge consecutive user messages to prevent Anthropic 400 errors
    const merged: MessageParam[] = [];
    for (const m of messages) {
      const prev = merged[merged.length - 1];
      if (
        prev &&
        prev.role === "user" &&
        m.role === "user" &&
        typeof prev.content === "string" &&
        typeof m.content === "string"
      ) {
        prev.content = prev.content + "\n\n" + m.content;
      } else {
        merged.push({ ...m });
      }
    }

    return merged;
  }

  async triggerDistillation(sessionId: string): Promise<void> {
    const compaction = this.config.agents.defaults.compaction;
    const contextTokens = this.config.agents.defaults.contextTokens ?? 200000;
    const distillThreshold = Math.floor(contextTokens * compaction.maxHistoryShare);
    const distillModel = compaction.distillationModel;

    const session = this.store.findSessionById(sessionId);
    if (!session) throw new Error(`Session ${sessionId} not found`);

    log.info(`Manual distillation triggered for session ${sessionId}`);
    await distillSession(this.store, this.router, sessionId, session.nousId, {
      triggerThreshold: distillThreshold,
      minMessages: 4,
      extractionModel: distillModel,
      summaryModel: distillModel,
      ...(this.plugins ? { plugins: this.plugins } : {}),
    });
  }
}

