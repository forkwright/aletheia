// ContextPacketBuilder — assembles scoped context packets for sub-agent dispatches (Spec 32 Phase 2)
//
// Problem: Sub-agents currently receive minimal prompts (goal + criteria + raw JSON plan).
// The orchestrator burns 100k+ tokens synthesizing context that never reaches the executor.
//
// Solution: File-backed state from Phase 1 feeds a ContextPacketBuilder that reads only
// what's relevant per dispatch role, trims to a token budget, and returns a self-contained
// context string that starts at token 1 with exactly what the sub-agent needs.

import { createLogger } from "../koina/logger.js";
import {
  readProjectFile,
  readRequirementsFile,
  readRoadmapFile,
  readResearchFile,
  readDiscussFile,
  readPlanFile,
} from "./project-files.js";
import type { PlanningPhase, PlanningRequirement } from "./types.js";

const log = createLogger("dianoia:context-packet");

// Rough token estimation: ~4 chars per token for English text
const CHARS_PER_TOKEN = 4;

export type SubAgentRole =
  | "researcher"   // Domain research phase
  | "planner"      // Roadmap generation and phase planning
  | "executor"     // Phase execution (implementation)
  | "reviewer"     // Plan checking and verification
  | "verifier";    // Goal-backward verification

export interface ContextPacketOptions {
  /** Workspace root for file reads */
  workspaceRoot: string;
  /** Project ID */
  projectId: string;
  /** Phase ID (null for project-level dispatches like roadmap generation) */
  phaseId: string | null;
  /** Role determines which sections are included and prioritized */
  role: SubAgentRole;
  /** Maximum tokens for the context packet (default: 8000) */
  maxTokens?: number;
  /** Phase object with goal/criteria/plan (avoids re-reading from DB) */
  phase?: PlanningPhase | null;
  /** All phases for roadmap context */
  allPhases?: PlanningPhase[];
  /** Filtered requirements relevant to this phase */
  requirements?: PlanningRequirement[];
  /** Project goal string */
  projectGoal?: string;
  /** Additional context to append (e.g., codebase excerpts) */
  supplementary?: string;
}

interface ContextSection {
  /** Header for the section */
  header: string;
  /** Content to include */
  content: string;
  /** Priority (lower = included first). Sections are included in priority order until budget exhausted. */
  priority: number;
}

/** Role-based section inclusion matrix */
const ROLE_SECTIONS: Record<SubAgentRole, {
  includeProject: boolean;
  includeRequirements: boolean;
  includeRoadmap: boolean;
  includeResearch: boolean;
  includeDiscussion: boolean;
  includePlan: boolean;
  includePhaseGoal: boolean;
  includeSupplementary: boolean;
}> = {
  researcher: {
    includeProject: true,
    includeRequirements: false,
    includeRoadmap: false,
    includeResearch: false,    // Previous research — not needed for new research
    includeDiscussion: false,
    includePlan: false,
    includePhaseGoal: false,
    includeSupplementary: true,
  },
  planner: {
    includeProject: true,
    includeRequirements: true,
    includeRoadmap: true,       // Previous phases for dependency awareness
    includeResearch: true,
    includeDiscussion: true,
    includePlan: false,
    includePhaseGoal: true,
    includeSupplementary: false,
  },
  executor: {
    includeProject: false,       // Executor doesn't need full project context
    includeRequirements: true,   // Only phase-scoped requirements
    includeRoadmap: false,
    includeResearch: false,
    includeDiscussion: true,     // Decisions constrain implementation
    includePlan: true,           // The actual plan to execute
    includePhaseGoal: true,
    includeSupplementary: true,  // Codebase excerpts, file contents
  },
  reviewer: {
    includeProject: false,
    includeRequirements: true,
    includeRoadmap: false,
    includeResearch: false,
    includeDiscussion: true,
    includePlan: true,
    includePhaseGoal: true,
    includeSupplementary: false,
  },
  verifier: {
    includeProject: true,        // Needs full project goal for goal-backward check
    includeRequirements: true,
    includeRoadmap: true,        // Needs to see phase in context of whole
    includeResearch: false,
    includeDiscussion: true,     // Decisions inform what "met" means
    includePlan: true,
    includePhaseGoal: true,
    includeSupplementary: true,  // Implementation artifacts to verify against
  },
};

/**
 * Build a scoped context packet for a sub-agent dispatch.
 *
 * Reads from file-backed state (Phase 1), filters by role, trims to token budget.
 * The resulting string is self-contained — the sub-agent needs nothing else.
 */
export function buildContextPacket(opts: ContextPacketOptions): string {
  const maxTokens = opts.maxTokens ?? 8000;
  const maxChars = maxTokens * CHARS_PER_TOKEN;
  const config = ROLE_SECTIONS[opts.role];
  const sections: ContextSection[] = [];

  // Priority 0: Phase goal and success criteria (always highest priority when available)
  if (config.includePhaseGoal && opts.phase) {
    const lines = [
      `**Phase:** ${opts.phase.name}`,
      `**Goal:** ${opts.phase.goal}`,
    ];
    if (opts.phase.successCriteria.length > 0) {
      lines.push("", "**Success Criteria:**");
      for (const c of opts.phase.successCriteria) {
        lines.push(`- ${c}`);
      }
    }
    sections.push({ header: "Phase Objective", content: lines.join("\n"), priority: 0 });
  }

  // Priority 1: Project goal (brief — just the goal, not full PROJECT.md)
  if (opts.projectGoal) {
    sections.push({ header: "Project Goal", content: opts.projectGoal, priority: 1 });
  }

  // Priority 2: Plan (for executor/reviewer/verifier)
  if (config.includePlan && opts.phaseId) {
    const plan = readPlanFile(opts.workspaceRoot, opts.projectId, opts.phaseId);
    if (plan) {
      sections.push({ header: "Execution Plan", content: plan, priority: 2 });
    } else if (opts.phase?.plan) {
      // Fallback to in-memory plan
      const planStr = typeof opts.phase.plan === "string"
        ? opts.phase.plan
        : JSON.stringify(opts.phase.plan, null, 2);
      sections.push({ header: "Execution Plan", content: planStr, priority: 2 });
    }
  }

  // Priority 3: Discussion decisions (constrain what's acceptable)
  if (config.includeDiscussion && opts.phaseId) {
    const discuss = readDiscussFile(opts.workspaceRoot, opts.projectId, opts.phaseId);
    if (discuss) {
      sections.push({ header: "Design Decisions", content: discuss, priority: 3 });
    }
  }

  // Priority 4: Requirements (phase-scoped if available)
  if (config.includeRequirements) {
    if (opts.requirements && opts.requirements.length > 0) {
      const lines = formatRequirements(opts.requirements);
      sections.push({ header: "Requirements", content: lines, priority: 4 });
    } else {
      const reqFile = readRequirementsFile(opts.workspaceRoot, opts.projectId);
      if (reqFile) {
        // If we have a phase, filter to relevant requirements
        const filtered = opts.phase
          ? filterRequirementsToPhase(reqFile, opts.phase.requirements)
          : reqFile;
        sections.push({ header: "Requirements", content: filtered, priority: 4 });
      }
    }
  }

  // Priority 5: Supplementary context (codebase, implementation artifacts)
  if (config.includeSupplementary && opts.supplementary) {
    sections.push({ header: "Reference Material", content: opts.supplementary, priority: 5 });
  }

  // Priority 6: Roadmap (for planners and verifiers — phase ordering context)
  if (config.includeRoadmap) {
    if (opts.allPhases && opts.allPhases.length > 0) {
      const lines = formatRoadmapSummary(opts.allPhases);
      sections.push({ header: "Roadmap Overview", content: lines, priority: 6 });
    } else {
      const roadmap = readRoadmapFile(opts.workspaceRoot, opts.projectId);
      if (roadmap) {
        sections.push({ header: "Roadmap Overview", content: roadmap, priority: 6 });
      }
    }
  }

  // Priority 7: Research findings
  if (config.includeResearch) {
    const research = readResearchFile(opts.workspaceRoot, opts.projectId);
    if (research) {
      sections.push({ header: "Research Findings", content: research, priority: 7 });
    }
  }

  // Priority 8: Full project context (lowest priority — only if budget allows)
  if (config.includeProject) {
    const projectMd = readProjectFile(opts.workspaceRoot, opts.projectId);
    if (projectMd) {
      sections.push({ header: "Project Context", content: projectMd, priority: 8 });
    }
  }

  // Assemble sections in priority order, respecting token budget
  return assembleSections(sections, maxChars);
}

/**
 * Assemble sections in priority order, truncating to fit budget.
 * Final section may be truncated mid-content if budget is tight.
 */
function assembleSections(sections: ContextSection[], maxChars: number): string {
  // Sort by priority (lower = first)
  const sorted = [...sections].sort((a, b) => a.priority - b.priority);

  const parts: string[] = [];
  let currentChars = 0;

  for (const section of sorted) {
    const sectionText = `## ${section.header}\n\n${section.content}\n\n`;
    const sectionChars = sectionText.length;

    if (currentChars + sectionChars <= maxChars) {
      // Fits entirely
      parts.push(sectionText);
      currentChars += sectionChars;
    } else {
      // Partial fit — truncate and add ellipsis
      const remaining = maxChars - currentChars;
      if (remaining > 100) {
        // Only include if we can fit a meaningful chunk
        const truncated = sectionText.slice(0, remaining - 20) + "\n\n[...truncated]";
        parts.push(truncated);
        currentChars = maxChars;
      }
      break;
    }
  }

  const result = parts.join("").trim();
  const estimatedTokens = Math.ceil(result.length / CHARS_PER_TOKEN);
  log.debug(`Context packet assembled: ${sorted.length} sections, ~${estimatedTokens} tokens`, {
    includedSections: sorted.map((s) => s.header),
  });

  return result;
}

/**
 * Format requirements as a compact table for context packets.
 */
function formatRequirements(requirements: PlanningRequirement[]): string {
  if (requirements.length === 0) return "(none)";

  const lines = ["| ID | Description | Tier |", "|-----|-------------|------|"];
  for (const req of requirements) {
    lines.push(`| ${req.reqId} | ${req.description} | ${req.tier} |`);
  }
  return lines.join("\n");
}

/**
 * Filter a full REQUIREMENTS.md string to only lines containing the given req IDs.
 * Falls back to full content if filtering would lose everything.
 */
function filterRequirementsToPhase(reqMarkdown: string, phaseReqIds: string[]): string {
  if (phaseReqIds.length === 0) return reqMarkdown;

  const lines = reqMarkdown.split("\n");
  const filtered: string[] = [];
  let inHeader = true;

  for (const line of lines) {
    // Keep headers and table structure
    if (line.startsWith("# ") || line.startsWith("## ") || line.startsWith("|---")) {
      filtered.push(line);
      inHeader = false;
      continue;
    }
    // Keep table header rows
    if (inHeader || line.startsWith("| ID")) {
      filtered.push(line);
      continue;
    }
    // Keep rows that contain any of our phase req IDs
    if (phaseReqIds.some((id) => line.includes(id))) {
      filtered.push(line);
    }
  }

  // If we filtered everything meaningful, return the original
  const meaningful = filtered.filter((l) => l.startsWith("| ") && !l.startsWith("|---"));
  if (meaningful.length <= 1) return reqMarkdown; // Only header row left

  return filtered.join("\n");
}

/**
 * Format a compact roadmap summary from phase objects.
 */
function formatRoadmapSummary(phases: PlanningPhase[]): string {
  const lines: string[] = [];
  for (const phase of phases) {
    const status = phase.status === "complete" ? "✅" :
      phase.status === "executing" ? "🔄" :
      phase.status === "failed" ? "❌" :
      phase.status === "skipped" ? "⏭" : "⬜";
    lines.push(`${status} **Phase ${phase.phaseOrder + 1}: ${phase.name}** — ${phase.goal}`);
    if (phase.requirements.length > 0) {
      lines.push(`  Requirements: ${phase.requirements.join(", ")}`);
    }
  }
  return lines.join("\n");
}

/**
 * Select the appropriate model tier for a sub-agent role and task complexity.
 *
 * Model selection strategy:
 * - Haiku: exploration, read-only queries, simple validation (cheap + fast)
 * - Sonnet: implementation, code generation, plan creation (capable + cost-effective)
 * - Opus: architecture decisions, complex tradeoffs, judgment calls (never used for sub-agents — that's the orchestrator)
 */
export type ModelTier = "haiku" | "sonnet";

export function selectModelForRole(role: SubAgentRole): ModelTier {
  switch (role) {
    case "researcher":
      return "sonnet";     // Research needs reasoning about domain
    case "planner":
      return "sonnet";     // Plan generation needs structured thinking
    case "executor":
      return "sonnet";     // Code generation needs capability
    case "reviewer":
      return "sonnet";     // Review needs judgment
    case "verifier":
      return "sonnet";     // Verification needs reasoning
    default:
      return "sonnet";
  }
}

/**
 * Maps our model tier to the actual model ID used in dispatch.
 * Uses the same role strings that sessions_spawn understands.
 */
export function modelTierToRole(tier: ModelTier): "coder" | "reviewer" | "researcher" | "explorer" | "runner" {
  switch (tier) {
    case "haiku":
      return "explorer";   // Haiku-backed roles
    case "sonnet":
      return "coder";      // Sonnet-backed roles
  }
}
