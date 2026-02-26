// Finalize stage — trace persistence, signal classification, skill extraction, working state, memory
import { join } from "node:path";
import { persistTrace } from "../../trace.js";
import { classifyInteraction } from "../../interaction-signals.js";
import { extractSkillCandidate, saveLearnedSkill } from "../../../organon/skill-learner.js";
import { extractWorkingState } from "../../working-state.js";
import { extractTurnFacts } from "../../turn-facts.js";
import { resolveWorkspace } from "../../../taxis/loader.js";
import { loadPipelineConfig } from "../../pipeline-config.js";
import { eventBus } from "../../../koina/event-bus.js";
import { createLogger } from "../../../koina/logger.js";
import { getSidecarUrl, getUserId } from "../../../koina/memory-client.js";
import type { RuntimeServices, TurnState } from "../types.js";

const log = createLogger("finalize");
const reinforcementLog = createLogger("nous:reinforcement");

function tokenizeForJaccard(s: string): Set<string> {
  return new Set(
    s.toLowerCase()
      .split(/\s+/)
      .map((t) => t.replace(/[^a-z0-9]/g, ""))
      .filter((t) => t.length >= 3),
  );
}

/**
 * Token-level Jaccard overlap between two strings.
 * Tokenizes both strings (lowercase, strips non-alphanumeric, splits on whitespace, filters tokens < 3 chars).
 * Returns intersection / union, or 0 if both token sets are empty.
 */
export function tokenJaccardOverlap(a: string, b: string): number {
  const tokensA = tokenizeForJaccard(a);
  const tokensB = tokenizeForJaccard(b);
  if (tokensA.size === 0 && tokensB.size === 0) return 0;
  let intersectionCount = 0;
  for (const t of tokensA) {
    if (tokensB.has(t)) intersectionCount++;
  }
  const union = tokensA.size + tokensB.size - intersectionCount;
  return union === 0 ? 0 : intersectionCount / union;
}

/**
 * Fire-and-forget reinforcement of memories actually used in the response.
 * Uses Jaccard overlap >= 0.25 to detect which recalled memories influenced the output.
 * Only those memories are sent to /evolution/reinforce — not every recall hit.
 */
function reinforceUsedMemories(
  memoryIds: string[],
  memoryTexts: Map<string, string>,
  responseText: string,
  sidecarUrl: string,
  nousId: string,
): void {
  try {
    const JACCARD_THRESHOLD = 0.25;
    const usedIds = memoryIds.filter((id) => {
      const text = memoryTexts.get(id);
      if (!text) return false;
      return tokenJaccardOverlap(text, responseText) >= JACCARD_THRESHOLD;
    });

    if (usedIds.length === 0) return;

    reinforcementLog.debug(
      `Reinforcing ${usedIds.length}/${memoryIds.length} used memories for ${nousId}`,
    );

    for (const memoryId of usedIds) {
      void (async () => {
        try {
          await fetch(`${sidecarUrl}/evolution/reinforce`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ memory_id: memoryId, user_id: nousId }),
            signal: AbortSignal.timeout(5_000),
          });
        } catch (error: unknown) {
          reinforcementLog.warn(
            `Reinforcement failed for ${memoryId}: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
      })();
    }
  } catch (error: unknown) {
    reinforcementLog.warn(
      `reinforceUsedMemories failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

export async function finalize(
  state: TurnState,
  services: RuntimeServices,
): Promise<void> {
  const {
    nousId, sessionId, sessionKey, msg, nous, workspace, seq, trace,
    totalToolCalls, totalInputTokens, totalOutputTokens, turnToolCalls, outcome,
  } = state;

  if (!outcome) return;

  // Expire tools not used in the last N turns — frees definition token budget
  const pipelineConfig = loadPipelineConfig(workspace);
  const expired = services.tools.expireUnusedTools(sessionId, seq, pipelineConfig.tools.expiryTurns);
  if (expired.length > 0) {
    log.info(`Expired ${expired.length} unused tools for session ${sessionId}: ${expired.join(", ")}`);
  }

  // Persist causal trace
  const finalTrace = trace.finalize();
  persistTrace(finalTrace, workspace);

  // Update actual API-reported context for distillation triggering
  services.store.updateSessionActualTokens(sessionId, totalInputTokens);
  services.store.updateComputedContextTokens(sessionId, totalInputTokens);

  // Plugin afterTurn
  if (services.plugins) {
    await services.plugins.dispatchAfterTurn({
      nousId,
      sessionId,
      responseText: outcome.text,
      messageText: msg.text,
      toolCalls: totalToolCalls,
      inputTokens: totalInputTokens,
      outputTokens: totalOutputTokens,
    });
  }

  eventBus.emit("turn:after", {
    nousId, sessionId, toolCalls: totalToolCalls,
    inputTokens: totalInputTokens, outputTokens: totalOutputTokens,
    text: outcome.text.slice(0, 120),
  });

  // Interaction signal classification
  const signal = classifyInteraction(msg.text, outcome.text);
  services.store.recordSignal({
    sessionId, nousId, turnSeq: seq,
    signal: signal.signal, confidence: signal.confidence,
  });
  if (signal.signal === "correction" && services.competence) {
    const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
    services.competence.recordCorrection(nousId, domain);
  }

  // Competence tracking
  if (services.competence && totalToolCalls > 0) {
    const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
    services.competence.recordSuccess(nousId, domain);
  }

  // Skill learning
  if (turnToolCalls.length >= 3) {
    const skillModel = services.config.agents.defaults.compaction.distillationModel;
    const skillsDir = join(resolveWorkspace(services.config, nous)!, "..", "..", "shared", "skills");
    void (async () => {
      try {
        const candidate = await extractSkillCandidate(services.router, turnToolCalls, skillModel, sessionId, seq, nousId);
        if (candidate) saveLearnedSkill(candidate, skillsDir);
      } catch (error) {
        log.debug(`Skill extraction failed (non-fatal): ${error instanceof Error ? error.message : error}`);
      }
    })();
  }

  // Working state extraction — async, non-blocking, on cheap model
  // Only runs when there were tool calls (indicates active work, not just conversation)
  if (totalToolCalls > 0) {
    const wsModel = services.config.agents.defaults.compaction.distillationModel;
    const toolSummary = turnToolCalls
      .map((t) => `${t.name}(${JSON.stringify(t.input).slice(0, 100)}) → ${t.output.slice(0, 100)}`)
      .join("\n");
    const previousState = services.store.getWorkingState(sessionId);
    void (async () => {
      try {
        const newState = await extractWorkingState(services.router, outcome.text, toolSummary, previousState, wsModel);
        if (newState) services.store.updateWorkingState(sessionId, newState);
      } catch (error) {
        log.debug(`Working state extraction failed: ${error instanceof Error ? error.message : error}`);
      }
    })();
  }

  // After-turn memory extraction — lightweight, non-blocking, on cheap model
  // Extracts 0-3 durable facts and stores them immediately via sidecar
  if (services.memoryTarget && outcome.text.length > 150) {
    const factModel = services.config.agents.defaults.compaction.distillationModel;
    const toolSummary = turnToolCalls
      .map((t) => `${t.name}(${JSON.stringify(t.input).slice(0, 80)}) → ${t.output.slice(0, 80)}`)
      .join("\n");

    void (async () => {
      try {
        const result = await extractTurnFacts(services.router, outcome.text, toolSummary, factModel);
        if (result.facts.length > 0) {
          try {
            const res = await fetch(`${getSidecarUrl()}/add_batch`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                texts: result.facts,
                user_id: getUserId(),
                agent_id: nousId,
                source: "turn",
                session_id: sessionId,
                confidence: 0.7, // Lower than distillation — single-turn context
              }),
              signal: AbortSignal.timeout(10_000),
            });
            if (res.ok) {
              const data = await res.json() as { added?: number; skipped?: number; errors?: number };
              const receipt = {
                origin: "turn_extraction" as const,
                agentId: nousId,
                sessionId,
                timestamp: new Date().toISOString(),
                factCount: result.facts.length,
                added: data.added ?? 0,
                skipped: data.skipped ?? 0,
                errors: data.errors ?? 0,
                durationMs: result.durationMs,
              };
              log.info("Memory write receipt", receipt);
            }
          } catch (error) {
            log.debug(`Turn fact storage failed: ${error instanceof Error ? error.message : error}`);
          }
        }
      } catch (error) {
        log.debug(`Turn fact extraction failed: ${error instanceof Error ? error.message : error}`);
      }
    })();
  }

  // Reinforce recalled memories that were actually used in the response
  // Fire-and-forget — never blocks turn completion
  const { recalledMemoryIds, recalledMemoryTexts } = state;
  if (
    recalledMemoryIds &&
    recalledMemoryIds.length > 0 &&
    recalledMemoryTexts &&
    outcome.text.length > 100
  ) {
    void reinforceUsedMemories(
      recalledMemoryIds,
      recalledMemoryTexts,
      outcome.text,
      getSidecarUrl(),
      nousId,
    );
  }

  // Note: auto-distillation scheduling moved to NousManager (runs after session lock release)
}
