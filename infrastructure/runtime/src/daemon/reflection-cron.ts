// Nightly reflection cron — sleep-time compute for all active agents
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { reflectOnAgent, weeklyReflection } from "../distillation/reflect.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { SessionStore } from "../mneme/store.js";
import type { MemoryFlushTarget } from "../distillation/hooks.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { CompetenceModel } from "../nous/competence.js";

const log = createLogger("daemon:reflection");

export interface ReflectionCronOpts {
  /** Model for reflection (default: extraction model from compaction config) */
  model?: string;
  /** Minimum human messages for a session to qualify (default: 10) */
  minHumanMessages?: number;
  /** Lookback window in hours (default: 24) */
  lookbackHours?: number;
  /** Memory target for persisting findings */
  memoryTarget?: MemoryFlushTarget;
  /** Fetch existing memories for contradiction detection */
  fetchExistingMemories?: (nousId: string) => Promise<string[]>;
  /** Competence model for structured eval output */
  competence?: CompetenceModel;
}

/**
 * Run nightly reflection for all configured agents.
 * Designed to be called from a cron job (schedule: "at 03:00" or similar).
 */
export async function runNightlyReflection(
  store: SessionStore,
  router: ProviderRouter,
  config: AletheiaConfig,
  opts: ReflectionCronOpts = {},
): Promise<{
  agentsReflected: number;
  totalFindings: number;
  totalMemoriesStored: number;
  errors: string[];
}> {
  const model = opts.model ?? config.agents.defaults.compaction.distillationModel;
  const minHumanMessages = opts.minHumanMessages ?? 10;
  const lookbackHours = opts.lookbackHours ?? 24;

  // Get all configured agents
  const agentIds = config.agents.list.map((a) => a.id);
  if (agentIds.length === 0) {
    log.info("No agents configured — skipping reflection");
    return { agentsReflected: 0, totalFindings: 0, totalMemoriesStored: 0, errors: [] };
  }

  log.info(`Starting nightly reflection for ${agentIds.length} agents (model: ${model}, lookback: ${lookbackHours}h)`);

  let agentsReflected = 0;
  let totalFindings = 0;
  let totalMemoriesStored = 0;
  const errors: string[] = [];

  for (const nousId of agentIds) {
    try {
      // Optionally fetch existing memories for contradiction detection
      let existingMemories: string[] | undefined;
      if (opts.fetchExistingMemories) {
        try {
          existingMemories = await opts.fetchExistingMemories(nousId);
        } catch (err) {
          log.warn(`Failed to fetch existing memories for ${nousId}: ${err instanceof Error ? err.message : err}`);
          // Continue without — contradiction detection will be less effective but reflection still works
        }
      }

      const reflectOpts: Parameters<typeof reflectOnAgent>[3] = {
        model,
        minHumanMessages,
        lookbackHours,
      };
      if (opts.memoryTarget) reflectOpts.memoryTarget = opts.memoryTarget;
      if (existingMemories) reflectOpts.existingMemories = existingMemories;

      const result = await reflectOnAgent(store, router, nousId, reflectOpts);

      if (result.sessionsReviewed > 0) {
        agentsReflected++;
        const findings =
          result.findings.patterns.length +
          result.findings.contradictions.length +
          result.findings.corrections.length +
          result.findings.preferences.length +
          result.findings.relationships.length +
          result.findings.unresolvedThreads.length;
        totalFindings += findings;
        totalMemoriesStored += result.memoriesStored;

        log.info(
          `Reflection for ${nousId}: ${result.sessionsReviewed} sessions, ` +
          `${findings} findings, ${result.memoriesStored} stored, ` +
          `${result.tokensUsed} tokens, ${result.durationMs}ms`,
        );

        // Write structured EVAL_FEEDBACK.md to agent workspace
        const agentConfig = config.agents.list.find((a) => a.id === nousId);
        if (agentConfig?.workspace) {
          try {
            const evalContent = buildEvalFeedback(nousId, result.findings, opts.competence);
            writeFileSync(join(agentConfig.workspace, "EVAL_FEEDBACK.md"), evalContent);
            log.info(`Wrote EVAL_FEEDBACK.md for ${nousId}`);
          } catch (writeErr) {
            log.warn(`Failed to write EVAL_FEEDBACK.md for ${nousId}: ${writeErr instanceof Error ? writeErr.message : writeErr}`);
          }
        }
      }
    } catch (err) {
      const msg = `Reflection failed for ${nousId}: ${err instanceof Error ? err.message : err}`;
      log.error(msg);
      errors.push(msg);
    }
  }

  log.info(
    `Nightly reflection complete: ${agentsReflected}/${agentIds.length} agents, ` +
    `${totalFindings} findings, ${totalMemoriesStored} memories stored` +
    (errors.length > 0 ? `, ${errors.length} errors` : ""),
  );

  return { agentsReflected, totalFindings, totalMemoriesStored, errors };
}


/**
 * Run weekly cross-session reflection for all configured agents.
 * Designed to be called from a cron job (schedule: "0 4 * * 0" — Sunday 4am).
 */
export async function runWeeklyReflection(
  store: SessionStore,
  router: ProviderRouter,
  config: AletheiaConfig,
  opts: { model?: string; lookbackDays?: number } = {},
): Promise<{
  agentsReflected: number;
  totalFindings: number;
  errors: string[];
}> {
  const model = opts.model ?? config.agents.defaults.compaction.distillationModel;
  const lookbackDays = opts.lookbackDays ?? 7;

  const agentIds = config.agents.list.map((a) => a.id);
  if (agentIds.length === 0) {
    log.info("No agents configured — skipping weekly reflection");
    return { agentsReflected: 0, totalFindings: 0, errors: [] };
  }

  log.info(`Starting weekly reflection for ${agentIds.length} agents (model: ${model}, lookback: ${lookbackDays}d)`);

  let agentsReflected = 0;
  let totalFindings = 0;
  const errors: string[] = [];

  for (const nousId of agentIds) {
    try {
      const result = await weeklyReflection(store, router, nousId, {
        model,
        lookbackDays,
      });

      if (result.summariesReviewed > 0) {
        agentsReflected++;
        const findings = result.trajectory.length +
          result.topicDrift.length +
          result.weeklyPatterns.length +
          result.unresolvedArcs.length;
        totalFindings += findings;

        log.info(
          `Weekly reflection for ${nousId}: ${result.summariesReviewed} summaries, ` +
          `${findings} findings, ${result.tokensUsed} tokens, ${result.durationMs}ms`,
        );
      }
    } catch (err) {
      const msg = `Weekly reflection failed for ${nousId}: ${err instanceof Error ? err.message : err}`;
      log.error(msg);
      errors.push(msg);
    }
  }

  log.info(
    `Weekly reflection complete: ${agentsReflected}/${agentIds.length} agents, ` +
    `${totalFindings} findings` +
    (errors.length > 0 ? `, ${errors.length} errors` : ""),
  );

  return { agentsReflected, totalFindings, errors };
}

function buildEvalFeedback(
  nousId: string,
  findings: import("../mneme/store.js").ReflectionFindings,
  competence?: CompetenceModel,
): string {
  const date = new Date().toISOString().split("T")[0];
  const lines: string[] = [`## Self-Evaluation — ${date}\n`];

  // Strengths from competence model
  lines.push("### Strengths");
  if (competence) {
    const agent = competence.getAgentCompetence(nousId);
    if (agent) {
      const strong = Object.entries(agent.domains)
        .filter(([, d]) => d.score >= 0.5)
        .sort((a, b) => b[1].score - a[1].score);
      for (const [domain, data] of strong) {
        lines.push(`- **${domain}**: score ${data.score.toFixed(2)} (${data.successes} successes)`);
      }
      if (strong.length === 0) lines.push("- No high-scoring domains yet");
    } else {
      lines.push("- No competence data recorded yet");
    }
  } else {
    lines.push("- Competence model not available");
  }

  // Weaknesses from competence model
  lines.push("\n### Weaknesses");
  if (competence) {
    const agent = competence.getAgentCompetence(nousId);
    if (agent) {
      const weak = Object.entries(agent.domains)
        .filter(([, d]) => d.score < 0.5)
        .sort((a, b) => a[1].score - b[1].score);
      for (const [domain, data] of weak) {
        lines.push(`- **${domain}**: score ${data.score.toFixed(2)} (${data.corrections} corrections)`);
      }
      if (weak.length === 0) lines.push("- No weak domains detected");
    }
  }

  // Correction patterns from reflection findings
  lines.push("\n### Correction Patterns");
  if (findings.corrections.length > 0) {
    for (const c of findings.corrections.slice(0, 5)) lines.push(`- ${c}`);
  } else {
    lines.push("- No corrections found in this period");
  }

  // Recommended adjustments
  lines.push("\n### Recommended Adjustments");
  if (findings.patterns.length > 0) {
    for (const p of findings.patterns.slice(0, 3)) lines.push(`- ${p}`);
  }
  if (findings.contradictions.length > 0) {
    lines.push(`- Resolve ${findings.contradictions.length} contradiction(s) in memory`);
  }
  if (findings.unresolvedThreads.length > 0) {
    lines.push(`- Follow up on ${findings.unresolvedThreads.length} unresolved thread(s)`);
  }
  if (findings.patterns.length === 0 && findings.contradictions.length === 0 && findings.unresolvedThreads.length === 0) {
    lines.push("- No adjustments needed");
  }

  return lines.join("\n") + "\n";
}
