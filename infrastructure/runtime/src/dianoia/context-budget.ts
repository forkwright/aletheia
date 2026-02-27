// ContextBudget — orchestrator token ceiling enforcement (ENG-08)
//
// The orchestrator must stay under 40k tokens. It holds only:
// - PROJECT.md (goal, state)
// - ROADMAP.md (phase overview)
// - Current phase status
// - Handoff context (if resuming)
//
// Everything else is delegated via context packets to sub-agents.
// This module enforces the ceiling and provides utilities for budget-aware operations.

import { createLogger } from "../koina/logger.js";
import { getEncoding } from "js-tiktoken";
import {
  readProjectFile,
  readRoadmapFile,
} from "./project-files.js";
import { readHandoffFile } from "./handoff.js";
import type { PlanningProject, PlanningPhase } from "./types.js";

const log = createLogger("dianoia:budget");

// Use tiktoken for accurate token counting
let encoder: ReturnType<typeof getEncoding> | null = null;

function getEncoder() {
  if (!encoder) {
    encoder = getEncoding("cl100k_base");
  }
  return encoder;
}

function countTokens(text: string): number {
  try {
    return getEncoder().encode(text).length;
  } catch {
    // Fallback
    return Math.ceil(text.length / 4);
  }
}

/** Default orchestrator token ceiling */
export const DEFAULT_ORCHESTRATOR_CEILING = 40_000;

/** Minimum tokens reserved for orchestrator reasoning */
const ORCHESTRATOR_REASONING_RESERVE = 8_000;

export interface BudgetAllocation {
  /** Total budget for orchestrator context */
  totalBudget: number;
  /** Tokens used by project context */
  projectTokens: number;
  /** Tokens used by roadmap */
  roadmapTokens: number;
  /** Tokens used by current phase status */
  phaseStatusTokens: number;
  /** Tokens used by handoff context (if any) */
  handoffTokens: number;
  /** Total tokens consumed */
  totalConsumed: number;
  /** Tokens remaining for reasoning */
  remaining: number;
  /** Whether the budget is within ceiling */
  withinBudget: boolean;
  /** Warning if budget is tight (< 20% remaining for reasoning) */
  warning: string | null;
}

/**
 * Calculate the orchestrator's context budget allocation.
 * Returns a breakdown of where tokens are being spent.
 */
export function calculateBudgetAllocation(opts: {
  projectDirValue: string;
  project: PlanningProject;
  phases: PlanningPhase[];
  ceiling?: number;
}): BudgetAllocation {
  const ceiling = opts.ceiling ?? DEFAULT_ORCHESTRATOR_CEILING;

  // PROJECT.md — goal, state, context
  let projectTokens = 0;
  const projectMd = readProjectFile(opts.projectDirValue);
  if (projectMd) {
    projectTokens = countTokens(projectMd);
  }

  // ROADMAP.md — phase overview
  let roadmapTokens = 0;
  const roadmapMd = readRoadmapFile(opts.projectDirValue);
  if (roadmapMd) {
    roadmapTokens = countTokens(roadmapMd);
  }

  // Current phase status — compact summary of active/pending phases
  const phaseStatusText = formatPhaseStatus(opts.phases);
  const phaseStatusTokens = countTokens(phaseStatusText);

  // Handoff context — if resuming from a paused session
  let handoffTokens = 0;
  const handoff = readHandoffFile(opts.projectDirValue);
  if (handoff) {
    // Only include the resume-relevant parts, not the full JSON
    const handoffText = [
      `Resume from: ${handoff.phaseName} (wave ${handoff.currentWave + 1}/${handoff.totalWaves})`,
      `Reason: ${handoff.pauseReason} — ${handoff.pauseDetail}`,
      `Action: ${handoff.resumeAction}`,
      handoff.blockers.length > 0 ? `Blockers: ${handoff.blockers.join("; ")}` : "",
      handoff.completedTaskIds.length > 0 ? `Done: ${handoff.completedTaskIds.join(", ")}` : "",
      handoff.pendingTaskIds.length > 0 ? `Pending: ${handoff.pendingTaskIds.join(", ")}` : "",
    ].filter(Boolean).join("\n");
    handoffTokens = countTokens(handoffText);
  }

  const totalConsumed = projectTokens + roadmapTokens + phaseStatusTokens + handoffTokens;
  const remaining = ceiling - totalConsumed;
  const withinBudget = remaining >= ORCHESTRATOR_REASONING_RESERVE;

  let warning: string | null = null;
  if (!withinBudget) {
    warning = `Orchestrator context (${totalConsumed} tokens) exceeds safe ceiling (${ceiling - ORCHESTRATOR_REASONING_RESERVE} usable). Consider trimming project context or roadmap.`;
  } else if (remaining < ceiling * 0.2) {
    warning = `Orchestrator context budget tight: ${remaining} tokens remaining (${Math.round((remaining / ceiling) * 100)}% of ${ceiling})`;
  }

  if (warning) {
    log.warn(warning);
  }

  return {
    totalBudget: ceiling,
    projectTokens,
    roadmapTokens,
    phaseStatusTokens,
    handoffTokens,
    totalConsumed,
    remaining,
    withinBudget,
    warning,
  };
}

/**
 * Build a compact orchestrator context string that fits within budget.
 * This is what the orchestrator loads — nothing more.
 */
export function buildOrchestratorContext(opts: {
  projectDirValue: string;
  project: PlanningProject;
  phases: PlanningPhase[];
  ceiling?: number;
}): { context: string; budget: BudgetAllocation } {
  const budget = calculateBudgetAllocation(opts);
  const parts: string[] = [];

  // Always include project goal (priority 0)
  parts.push(`# ${opts.project.goal}`);
  parts.push(`State: ${opts.project.state}`);
  parts.push("");

  // Phase overview — compact format, not full ROADMAP.md if budget is tight
  if (budget.roadmapTokens > budget.totalBudget * 0.3) {
    // Roadmap is using >30% of budget — use compact format
    parts.push("## Phases");
    for (const phase of opts.phases) {
      const icon = phaseIcon(phase.status);
      parts.push(`${icon} ${phase.name}: ${phase.goal}`);
    }
  } else {
    // Roadmap fits comfortably — use full file
    const roadmapMd = readRoadmapFile(opts.projectDirValue);
    if (roadmapMd) {
      parts.push(roadmapMd);
    }
  }
  parts.push("");

  // Current execution state
  const executing = opts.phases.filter((p) => p.status === "executing");
  if (executing.length > 0) {
    parts.push("## Active");
    for (const phase of executing) {
      parts.push(`🔄 **${phase.name}**: ${phase.goal}`);
      if (phase.successCriteria.length > 0) {
        for (const c of phase.successCriteria) {
          parts.push(`  - ${c}`);
        }
      }
    }
    parts.push("");
  }

  // Handoff context if resuming
  const handoff = readHandoffFile(opts.projectDirValue);
  if (handoff) {
    parts.push("## Resume Context");
    parts.push(`Paused: ${handoff.pauseReason} — ${handoff.pauseDetail}`);
    parts.push(`Action: ${handoff.resumeAction}`);
    if (handoff.blockers.length > 0) {
      parts.push(`⚠️ Blockers: ${handoff.blockers.join("; ")}`);
    }
    parts.push("");
  }

  const context = parts.join("\n");
  return { context, budget };
}

/**
 * Format a compact phase status summary.
 */
function formatPhaseStatus(phases: PlanningPhase[]): string {
  if (phases.length === 0) return "(no phases)";

  return phases
    .map((p) => `${phaseIcon(p.status)} ${p.name} [${p.status}]`)
    .join("\n");
}

function phaseIcon(status: string): string {
  switch (status) {
    case "complete": return "✅";
    case "executing": return "🔄";
    case "failed": return "❌";
    case "skipped": return "⏭";
    default: return "⬜";
  }
}

/**
 * Check if a project's orchestrator context fits within the budget ceiling.
 * Returns true if within budget, false if exceeds.
 */
export function checkBudget(opts: {
  projectDirValue: string;
  project: PlanningProject;
  phases: PlanningPhase[];
  ceiling?: number;
}): boolean {
  const budget = calculateBudgetAllocation(opts);
  return budget.withinBudget;
}
