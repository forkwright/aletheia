// Finalize stage — trace persistence, signal classification, skill extraction
import { join } from "node:path";
import { persistTrace } from "../../trace.js";
import { classifyInteraction } from "../../interaction-signals.js";
import { extractSkillCandidate, saveLearnedSkill } from "../../../organon/skill-learner.js";
import { resolveWorkspace } from "../../../taxis/loader.js";
import { eventBus } from "../../../koina/event-bus.js";
import type { TurnState, RuntimeServices } from "../types.js";

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
      .catch(() => {});
  }

  // Note: auto-distillation scheduling moved to NousManager (runs after session lock release)
}
