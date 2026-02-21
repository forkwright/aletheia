// Finalize stage — trace persistence, signal classification, skill extraction, working state, memory
import { join } from "node:path";
import { persistTrace } from "../../trace.js";
import { classifyInteraction } from "../../interaction-signals.js";
import { extractSkillCandidate, saveLearnedSkill } from "../../../organon/skill-learner.js";
import { extractWorkingState } from "../../working-state.js";
import { extractTurnFacts } from "../../turn-facts.js";
import { resolveWorkspace } from "../../../taxis/loader.js";
import { eventBus } from "../../../koina/event-bus.js";
import { createLogger } from "../../../koina/logger.js";
import type { RuntimeServices, TurnState } from "../types.js";

const log = createLogger("finalize");

export async function finalize(
  state: TurnState,
  services: RuntimeServices,
): Promise<void> {
  const {
    nousId, sessionId, sessionKey, msg, nous, workspace, seq, trace,
    totalToolCalls, totalInputTokens, totalOutputTokens, turnToolCalls, outcome,
  } = state;

  if (!outcome) return;

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
    extractSkillCandidate(services.router, turnToolCalls, skillModel, sessionId, seq, nousId)
      .then((candidate) => { if (candidate) saveLearnedSkill(candidate, skillsDir); })
      .catch((err) => { log.debug(`Skill extraction failed (non-fatal): ${err instanceof Error ? err.message : err}`); });
  }

  // Working state extraction — async, non-blocking, on cheap model
  // Only runs when there were tool calls (indicates active work, not just conversation)
  if (totalToolCalls > 0) {
    const wsModel = services.config.agents.defaults.compaction.distillationModel;
    const toolSummary = turnToolCalls
      .map((t) => `${t.name}(${JSON.stringify(t.input).slice(0, 100)}) → ${t.output.slice(0, 100)}`)
      .join("\n");
    const previousState = services.store.getWorkingState(sessionId);
    extractWorkingState(services.router, outcome.text, toolSummary, previousState, wsModel)
      .then((newState) => {
        if (newState) services.store.updateWorkingState(sessionId, newState);
      })
      .catch((err) => { log.debug(`Working state extraction failed: ${err instanceof Error ? err.message : err}`); });
  }

  // After-turn memory extraction — lightweight, non-blocking, on cheap model
  // Extracts 0-3 durable facts and stores them immediately via sidecar
  if (services.memoryTarget && outcome.text.length > 150) {
    const factModel = services.config.agents.defaults.compaction.distillationModel;
    const toolSummary = turnToolCalls
      .map((t) => `${t.name}(${JSON.stringify(t.input).slice(0, 80)}) → ${t.output.slice(0, 80)}`)
      .join("\n");

    extractTurnFacts(services.router, outcome.text, toolSummary, factModel)
      .then(async (result) => {
        if (result.facts.length > 0) {
          const sidecarUrl = process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
          const userId = process.env["ALETHEIA_MEMORY_USER"] ?? "default";
          try {
            const res = await fetch(`${sidecarUrl}/add_batch`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                texts: result.facts,
                user_id: userId,
                agent_id: nousId,
                source: "turn",
                session_id: sessionId,
                confidence: 0.7, // Lower than distillation — single-turn context
              }),
              signal: AbortSignal.timeout(10_000),
            });
            if (res.ok) {
              const data = await res.json() as { added?: number; skipped?: number };
              if ((data.added ?? 0) > 0) {
                log.info(`Turn facts: ${data.added} stored, ${data.skipped ?? 0} deduped (${nousId}, ${result.durationMs}ms)`);
              }
            }
          } catch (err) {
            log.debug(`Turn fact storage failed: ${err instanceof Error ? err.message : err}`);
          }
        }
      })
      .catch((err) => { log.debug(`Turn fact extraction failed: ${err instanceof Error ? err.message : err}`); });
  }

  // Note: auto-distillation scheduling moved to NousManager (runs after session lock release)
}
