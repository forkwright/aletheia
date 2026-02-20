// Nous manager — lifecycle, turn coordination, pipeline delegation
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ToolRegistry } from "../organon/registry.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import { PipelineError, SessionError } from "../koina/errors.js";
import { resolveNous, resolveWorkspace } from "../taxis/loader.js";
import type { PluginRegistry } from "../prostheke/registry.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { CompetenceModel } from "./competence.js";
import type { UncertaintyTracker } from "./uncertainty.js";
import { distillSession } from "../distillation/pipeline.js";
import { ApprovalGate } from "../organon/approval.js";
import type { ApprovalMode } from "../organon/approval.js";
import { AsyncChannel } from "./async-channel.js";
import { resolveNousId } from "./pipeline/stages/resolve.js";
import { runBufferedPipeline, runStreamingPipeline } from "./pipeline/runner.js";
import type { InboundMessage, RuntimeServices, TurnOutcome, TurnStreamEvent } from "./pipeline/types.js";

export type { InboundMessage, TurnOutcome, TurnStreamEvent, MediaAttachment } from "./pipeline/types.js";

const log = createLogger("nous");

const sessionLocks = new Map<string, Promise<unknown>>();

function withSessionLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const previous = sessionLocks.get(key) ?? Promise.resolve();
  const current = previous.then(
    () => fn(),
    (prevErr) => {
      log.warn(`Previous turn on lock "${key}" failed: ${prevErr instanceof Error ? prevErr.message : prevErr}`);
      return fn();
    },
  );
  const settled = current.catch(() => {});
  sessionLocks.set(key, settled);
  settled.then(() => {
    if (sessionLocks.get(key) === settled) sessionLocks.delete(key);
  });
  return current;
}

let turnCounter = 0;

export class NousManager {
  private plugins?: PluginRegistry;
  private watchdog?: Watchdog;
  private skillsSection?: string | undefined;
  competence?: CompetenceModel;
  uncertainty?: UncertaintyTracker;
  activeTurns = 0;
  private activeTurnsByNous = new Map<string, number>();
  private turnAbortControllers = new Map<string, AbortController>();
  private turnMeta = new Map<string, { nousId: string; sessionId: string; startedAt: number }>();
  private activeSessionsByLock = new Map<string, string>(); // lockKey → sessionId
  readonly approvalGate = new ApprovalGate();
  isDraining: () => boolean = () => false;

  constructor(
    private config: AletheiaConfig,
    private store: SessionStore,
    private router: ProviderRouter,
    private tools: ToolRegistry,
  ) {
    log.info(`NousManager initialized with ${config.agents.list.length} nous`);
  }

  get sessionStore(): SessionStore { return this.store; }

  setPlugins(plugins: PluginRegistry): void { this.plugins = plugins; }
  setWatchdog(watchdog: Watchdog): void { this.watchdog = watchdog; }
  setSkillsSection(section: string | undefined): void { this.skillsSection = section; }
  setCompetence(model: CompetenceModel): void { this.competence = model; }
  setUncertainty(tracker: UncertaintyTracker): void { this.uncertainty = tracker; }

  getActiveTurnsByNous(): Record<string, number> {
    const result: Record<string, number> = {};
    for (const [nousId, count] of this.activeTurnsByNous) {
      if (count > 0) result[nousId] = count;
    }
    return result;
  }

  getActiveTurnDetails(): Array<{ turnId: string; nousId: string; sessionId: string; startedAt: number }> {
    return [...this.turnMeta].map(([turnId, meta]) => ({ turnId, ...meta }));
  }

  abortTurn(turnId: string): boolean {
    const controller = this.turnAbortControllers.get(turnId);
    if (!controller) return false;
    controller.abort();
    return true;
  }

  private buildServices(): RuntimeServices {
    const approvalMode: ApprovalMode =
      ((this.config.agents.defaults as Record<string, unknown>)["approval"] as { mode?: ApprovalMode } | undefined)?.mode ?? "autonomous";

    return {
      config: this.config,
      store: this.store,
      router: this.router,
      tools: this.tools,
      ...(this.plugins ? { plugins: this.plugins } : {}),
      ...(this.watchdog ? { watchdog: this.watchdog } : {}),
      ...(this.competence ? { competence: this.competence } : {}),
      ...(this.uncertainty ? { uncertainty: this.uncertainty } : {}),
      ...(this.skillsSection !== undefined ? { skillsSection: this.skillsSection } : {}),
      approvalGate: this.approvalGate,
      approvalMode,
    };
  }

  private trackTurnStart(nousId: string): void {
    this.activeTurns++;
    this.activeTurnsByNous.set(nousId, (this.activeTurnsByNous.get(nousId) ?? 0) + 1);
  }

  private trackTurnEnd(nousId: string): void {
    this.activeTurns--;
    const cur = this.activeTurnsByNous.get(nousId) ?? 1;
    if (cur <= 1) this.activeTurnsByNous.delete(nousId);
    else this.activeTurnsByNous.set(nousId, cur - 1);
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

    const services = this.buildServices();
    const nousId = resolveNousId(msg, services);
    const lockKey = msg.lockKey ?? `${nousId}:${msg.sessionKey ?? "main"}`;
    const turnId = `${nousId}:${++turnCounter}:${Date.now()}`;
    const abortController = new AbortController();

    this.turnAbortControllers.set(turnId, abortController);
    this.turnMeta.set(turnId, { nousId, sessionId: "", startedAt: Date.now() });
    this.trackTurnStart(nousId);

    const channel = new AsyncChannel<TurnStreamEvent>();

    const turnPromise = withSessionLock(lockKey, async () => {
      try {
        for await (const event of runStreamingPipeline(msg, services, {
          abortSignal: abortController.signal,
          turnId,
        })) {
          if (event.type === "turn_start") {
            this.turnMeta.set(turnId, { nousId, sessionId: event.sessionId, startedAt: Date.now() });
            this.activeSessionsByLock.set(lockKey, event.sessionId);
          }
          channel.push(event);
        }
      } catch (err) {
        channel.push({ type: "error", message: err instanceof Error ? err.message : String(err) });
      } finally {
        channel.close();
      }
    });

    let resolvedSessionId = "";
    try {
      for await (const event of channel) {
        if (event.type === "turn_start") resolvedSessionId = event.sessionId;
        yield event;
      }
      await turnPromise;
      if (resolvedSessionId) {
        this.maybeScheduleDistillation(resolvedSessionId, nousId, lockKey);
      }
    } catch (err) {
      yield { type: "error", message: err instanceof Error ? err.message : String(err) };
    } finally {
      this.trackTurnEnd(nousId);
      this.turnAbortControllers.delete(turnId);
      this.turnMeta.delete(turnId);
      this.activeSessionsByLock.delete(lockKey);
    }
  }

  async handleMessage(msg: InboundMessage): Promise<TurnOutcome> {
    if (this.isDraining()) {
      throw new PipelineError("Runtime is shutting down — rejecting new messages", {
        code: "TURN_REJECTED", context: { reason: "draining" },
      });
    }

    const maxDepth = this.config.session.agentToAgent.maxPingPongTurns;
    if (msg.depth && msg.depth >= maxDepth) {
      throw new PipelineError(`Cross-agent depth limit (${maxDepth}) exceeded`, {
        code: "TURN_REJECTED", context: { depth: msg.depth, maxDepth },
      });
    }

    const services = this.buildServices();
    const nousId = resolveNousId(msg, services);
    const lockKey = msg.lockKey ?? `${nousId}:${msg.sessionKey ?? "main"}`;

    this.trackTurnStart(nousId);
    try {
      const outcome = await withSessionLock(lockKey, () => runBufferedPipeline(msg, services));
      this.maybeScheduleDistillation(outcome.sessionId, outcome.nousId, lockKey);
      return outcome;
    } finally {
      this.trackTurnEnd(nousId);
    }
  }

  // --- Message Queue ---

  /** Check if a session has an active turn (use lockKey = `${nousId}:${sessionKey}`) */
  isSessionActive(lockKey: string): boolean {
    return this.activeSessionsByLock.has(lockKey);
  }

  /** Get the session ID for an active turn by lock key */
  getActiveSessionId(lockKey: string): string | undefined {
    return this.activeSessionsByLock.get(lockKey);
  }

  /** Queue a message for delivery during an active turn. Returns false if no active turn. */
  queueMessageForSession(lockKey: string, text: string, sender?: string): boolean {
    const sessionId = this.activeSessionsByLock.get(lockKey);
    if (!sessionId) return false;
    this.store.queueMessage(sessionId, text, sender);
    return true;
  }

  private maybeScheduleDistillation(sessionId: string, nousId: string, lockKey: string): void {
    const compaction = this.config.agents.defaults.compaction;
    const contextTokens = this.config.agents.defaults.contextTokens ?? 200000;
    const distillThreshold = Math.floor(contextTokens * compaction.maxHistoryShare);

    const session = this.store.findSessionById(sessionId);
    if (!session) return;

    const nous = resolveNous(this.config, nousId);
    const workspace = nous ? resolveWorkspace(this.config, nous) : undefined;
    const distillModel = compaction.distillationModel;

    // Background sessions distill earlier with lightweight settings
    if (session.sessionType === "background") {
      if (session.messageCount < 50 && session.tokenCountEstimate < 10000) return;
      log.info(`Scheduling lightweight distillation for background session ${sessionId} (${session.messageCount} msgs, ${session.tokenCountEstimate} tokens)`);
      withSessionLock(lockKey, async () => {
        await distillSession(this.store, this.router, sessionId, nousId, {
          triggerThreshold: 10000,
          minMessages: 20,
          extractionModel: distillModel,
          summaryModel: distillModel,
          preserveRecentMessages: 20,
          lightweight: true,
          ...(workspace ? { workspace } : {}),
        });
      }).catch((err) => {
        log.warn(`Background distillation failed for ${sessionId}: ${err instanceof Error ? err.message : err}`);
      });
      return;
    }

    // Primary sessions: multi-signal trigger — fire if ANY condition is met
    const actualContext = session.computedContextTokens || session.lastInputTokens || session.tokenCountEstimate || 0;
    const lastDistilledMs = session.lastDistilledAt ? Date.now() - new Date(session.lastDistilledAt).getTime() : Infinity;
    const sevenDays = 7 * 24 * 60 * 60 * 1000;

    let triggerReason: string | null = null;
    if (actualContext >= 120_000) {
      triggerReason = `context=${actualContext} >= 120K`;
    } else if (session.messageCount >= 150) {
      triggerReason = `messageCount=${session.messageCount} >= 150`;
    } else if (lastDistilledMs > sevenDays && session.messageCount >= 20) {
      triggerReason = `stale (${Math.round(lastDistilledMs / 86400000)}d since last distill) + ${session.messageCount} msgs`;
    } else if (session.distillationCount === 0 && session.messageCount >= 30) {
      triggerReason = `never distilled + ${session.messageCount} msgs`;
    } else if (actualContext >= distillThreshold && session.messageCount >= 10) {
      triggerReason = `legacy threshold (${actualContext} >= ${distillThreshold})`;
    }

    if (!triggerReason) return;

    const utilization = Math.round((actualContext / contextTokens) * 100);
    log.info(
      `Scheduling distillation for ${nousId} session=${sessionId} ` +
      `(${utilization}% context, trigger: ${triggerReason})`,
    );

    const thread = this.store.getThreadForSession(sessionId);

    withSessionLock(lockKey, async () => {
      await distillSession(this.store, this.router, sessionId, nousId, {
        triggerThreshold: distillThreshold,
        minMessages: 10,
        extractionModel: distillModel,
        summaryModel: distillModel,
        preserveRecentMessages: compaction.preserveRecentMessages,
        preserveRecentMaxTokens: compaction.preserveRecentMaxTokens,
        ...(workspace ? { workspace } : {}),
        ...(this.plugins ? { plugins: this.plugins } : {}),
        ...(thread ? {
          onThreadSummaryUpdate: (summary: string, keyFacts: string[]) => {
            this.store.updateThreadSummary(thread.id, summary, keyFacts);
          },
        } : {}),
      });
    }).catch((err) => {
      log.warn(`Deferred distillation failed for ${sessionId}: ${err instanceof Error ? err.message : err}`);
    });
  }

  async triggerDistillation(sessionId: string): Promise<void> {
    const compaction = this.config.agents.defaults.compaction;
    const contextTokens = this.config.agents.defaults.contextTokens ?? 200000;
    const distillThreshold = Math.floor(contextTokens * compaction.maxHistoryShare);
    const distillModel = compaction.distillationModel;

    const session = this.store.findSessionById(sessionId);
    if (!session) throw new SessionError(`Session ${sessionId} not found`, {
      code: "SESSION_NOT_FOUND", context: { sessionId },
    });

    const nous = resolveNous(this.config, session.nousId);
    const workspace = nous ? resolveWorkspace(this.config, nous) : undefined;

    const thread = this.store.getThreadForSession(sessionId);
    log.info(`Manual distillation triggered for session ${sessionId}`);
    await distillSession(this.store, this.router, sessionId, session.nousId, {
      triggerThreshold: distillThreshold,
      minMessages: 4,
      extractionModel: distillModel,
      summaryModel: distillModel,
      preserveRecentMessages: compaction.preserveRecentMessages,
      preserveRecentMaxTokens: compaction.preserveRecentMaxTokens,
      ...(workspace ? { workspace } : {}),
      ...(this.plugins ? { plugins: this.plugins } : {}),
      ...(thread ? {
        onThreadSummaryUpdate: (summary, keyFacts) => {
          this.store.updateThreadSummary(thread.id, summary, keyFacts);
        },
      } : {}),
    });
  }
}
