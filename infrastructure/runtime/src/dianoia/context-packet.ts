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
  readDiscussFile,
  readPlanFile,
  readProjectFile,
  readRequirementsFile,
  readResearchFile,
  readRoadmapFile,
} from "./project-files.js";
import type { PlanningPhase, PlanningRequirement } from "./types.js";
import { getEncoding } from "js-tiktoken";
import { buildContextPacketWithPriompt, buildContextPacketWithPriomptSync } from "./priompt-context.js";

const log = createLogger("dianoia:context-packet");

// Use tiktoken for accurate token counting (Claude uses cl100k_base)
const encoder = getEncoding("cl100k_base");

function countTokens(text: string): number {
  try {
    return encoder.encode(text).length;
  } catch (error) {
    log.warn(`Failed to encode text for token counting: ${error instanceof Error ? error.message : String(error)}`);
    // Fallback to character estimation
    return Math.ceil(text.length / 4);
  }
}

export type SubAgentRole =
  | "researcher"   // Domain research phase
  | "planner"      // Roadmap generation and phase planning
  | "executor"     // Phase execution (implementation)
  | "reviewer"     // Plan checking and verification
  | "verifier";    // Goal-backward verification

export interface ContextPacketOptions {
  /** Project directory value (slug for new projects, absolute path for legacy pre-migration projects) */
  projectDirValue: string;
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
 * 
 * Now uses Priompt for accurate tokenization and priority-based rendering.
 */
export async function buildContextPacket(opts: ContextPacketOptions): Promise<string> {
  try {
    // Use Priompt-based implementation for accurate tokenization
    return await buildContextPacketWithPriompt(opts);
  } catch (error) {
    log.warn(`Priompt context assembly failed, falling back to legacy: ${error instanceof Error ? error.message : String(error)}`);
    // Fallback to legacy implementation if Priompt fails
    return buildContextPacketLegacy(opts);
  }
}

/**
 * Synchronous context packet builder using accurate tiktoken tokenization.
 * This is the primary call path — used by execution, verification, and roadmap.
 */
export function buildContextPacketSync(opts: ContextPacketOptions): string {
  try {
    return buildContextPacketWithPriomptSync(opts);
  } catch (error) {
    log.warn(`Priompt sync context assembly failed, falling back to legacy: ${error instanceof Error ? error.message : String(error)}`);
    return buildContextPacketLegacy(opts);
  }
}

/**
 * Legacy context packet builder (fallback implementation).
 * Preserved for compatibility if Priompt fails.
 */
function buildContextPacketLegacy(opts: ContextPacketOptions): string {
  const maxTokens = opts.maxTokens ?? 8000;
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
    // Add output format requirement for executors
    if (opts.role === "executor") {
      lines.push("", "---", "",
        "**IMPORTANT — Output Format Requirement:**",
        "When you have completed your work (or cannot proceed), you MUST end your final response with a JSON result block:",
        "",
        "```json",
        "{",
        '  "status": "success" | "partial" | "failed",',
        '  "summary": "Brief description of what was accomplished",',
        '  "filesChanged": ["list", "of", "files"],',
        '  "issues": [],',
        '  "confidence": 0.0-1.0',
        "}",
        "```",
        "",
        "This structured output is required for the orchestrator to process your results.",
        "Do NOT omit this block. Do NOT return only prose.",
      );
    }
    sections.push({ header: "Phase Objective", content: lines.join("\n"), priority: 0 });
  }

  // Priority 1: Project goal (brief — just the goal, not full PROJECT.md)
  if (opts.projectGoal) {
    sections.push({ header: "Project Goal", content: opts.projectGoal, priority: 1 });
  }

  // Priority 2: Plan (for executor/reviewer/verifier)
  if (config.includePlan && opts.phaseId) {
    const plan = readPlanFile(opts.projectDirValue, opts.phaseId);
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
    const discuss = readDiscussFile(opts.projectDirValue, opts.phaseId);
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
      const reqFile = readRequirementsFile(opts.projectDirValue);
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
      const roadmap = readRoadmapFile(opts.projectDirValue);
      if (roadmap) {
        sections.push({ header: "Roadmap Overview", content: roadmap, priority: 6 });
      }
    }
  }

  // Priority 7: Research findings
  if (config.includeResearch) {
    const research = readResearchFile(opts.projectDirValue);
    if (research) {
      sections.push({ header: "Research Findings", content: research, priority: 7 });
    }
  }

  // Priority 8: Full project context (lowest priority — only if budget allows)
  if (config.includeProject) {
    const projectMd = readProjectFile(opts.projectDirValue);
    if (projectMd) {
      sections.push({ header: "Project Context", content: projectMd, priority: 8 });
    }
  }

  // Assemble sections in priority order, respecting token budget
  return assembleSections(sections, maxTokens);
}

/**
 * Assemble sections in priority order, truncating to fit budget.
 * Final section may be truncated mid-content if budget is tight.
 */
function assembleSections(sections: ContextSection[], maxTokens: number): string {
  // Sort by priority (lower = first)
  const sorted = [...sections].toSorted((a, b) => a.priority - b.priority);

  const parts: string[] = [];
  let currentTokens = 0;

  for (const section of sorted) {
    const sectionText = `## ${section.header}\n\n${section.content}\n\n`;
    const sectionTokens = countTokens(sectionText);

    if (currentTokens + sectionTokens <= maxTokens) {
      // Fits entirely
      parts.push(sectionText);
      currentTokens += sectionTokens;
    } else {
      // Partial fit — truncate by tokens and add ellipsis
      const remainingTokens = maxTokens - currentTokens;
      if (remainingTokens > 50) {
        // Only include if we can fit a meaningful chunk
        // Estimate chars that would fit in remaining tokens (rough approximation)
        const estimatedCharsToFit = remainingTokens * 4;
        const truncated = sectionText.slice(0, estimatedCharsToFit - 50) + "\n\n[...truncated]";
        const finalTokens = countTokens(truncated);
        if (currentTokens + finalTokens <= maxTokens) {
          parts.push(truncated);
          currentTokens += finalTokens;
        }
      }
      break;
    }
  }

  const result = parts.join("").trim();
  const actualTokens = countTokens(result);
  log.debug(`Context packet assembled: ${sorted.length} sections, ${actualTokens} tokens`, {
    includedSections: sorted.map((s) => s.header),
    tokenBudget: maxTokens,
    utilization: `${Math.round((actualTokens / maxTokens) * 100)}%`,
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
 * Task type classification for smart role/model selection.
 */
export type TaskType = 
  | "code-generation"     // Writing new code, implementing features
  | "code-editing"        // Modifying existing code, bug fixes
  | "code-review"         // Analyzing code for issues, style, logic
  | "exploration"         // Read-only codebase investigation
  | "testing"             // Running tests, validation, health checks
  | "research"            // Web research, documentation lookup
  | "planning"            // Task decomposition, strategy
  | "verification";       // Checking completeness, goal alignment

export interface TaskClassification {
  type: TaskType;
  complexity: "low" | "medium" | "high";
  requiresTooling: boolean;
  readOnly: boolean;
}

/**
 * Classify a task description to determine appropriate role and model.
 * Uses heuristics based on keywords and patterns in the task text.
 */
export function classifyTask(task: string): TaskClassification {
  const taskLower = task.toLowerCase();
  
  // Code generation indicators
  if (taskLower.match(/\b(implement|create|build|write|add|generate)\b.*\b(function|class|component|module|feature|endpoint|api)\b/)) {
    return { type: "code-generation", complexity: "medium", requiresTooling: true, readOnly: false };
  }
  
  // Code editing indicators  
  if (taskLower.match(/\b(fix|update|modify|change|edit|refactor|migrate)\b.*\b(bug|code|file|function|class)\b/)) {
    return { type: "code-editing", complexity: "medium", requiresTooling: true, readOnly: false };
  }
  
  // Code review indicators
  if (taskLower.match(/\b(review|check|analyze|audit|inspect|validate)\b.*\b(code|pr|diff|changes|file)\b/)) {
    return { type: "code-review", complexity: "low", requiresTooling: false, readOnly: true };
  }
  
  // Exploration indicators
  if (taskLower.match(/\b(find|locate|search|explore|investigate|trace|grep)\b/)) {
    return { type: "exploration", complexity: "low", requiresTooling: false, readOnly: true };
  }
  
  // Testing indicators
  if (taskLower.match(/\b(test|run|execute|check|validate)\b.*\b(tests?|build|command|script)\b/)) {
    return { type: "testing", complexity: "low", requiresTooling: true, readOnly: true };
  }
  
  // Research indicators
  if (taskLower.match(/\b(research|lookup|fetch|search|find)\b.*\b(documentation|api|library|package)\b/)) {
    return { type: "research", complexity: "medium", requiresTooling: true, readOnly: true };
  }
  
  // Planning indicators
  if (taskLower.match(/\b(plan|design|architect|decompose|break down|organize)\b/)) {
    return { type: "planning", complexity: "high", requiresTooling: false, readOnly: false };
  }
  
  // Verification indicators
  if (taskLower.match(/\b(verify|confirm|ensure|validate)\b.*\b(complete|goal|requirement|criteria)\b/)) {
    return { type: "verification", complexity: "medium", requiresTooling: false, readOnly: true };
  }
  
  // Default to code generation for ambiguous tasks
  return { type: "code-generation", complexity: "medium", requiresTooling: true, readOnly: false };
}

/**
 * Map a task classification to the optimal sub-agent role.
 */
export function taskTypeToRole(classification: TaskClassification): "coder" | "reviewer" | "researcher" | "explorer" | "runner" {
  switch (classification.type) {
    case "code-generation":
    case "code-editing":
      return "coder";
    case "code-review":
      return "reviewer";
    case "research":
      return "researcher";
    case "exploration":
      return "explorer";
    case "testing":
      return "runner";
    case "planning":
    case "verification":
      return classification.complexity === "high" ? "coder" : "reviewer";  // Complex planning needs coder capability
    default:
      return "coder";
  }
}

/**
 * Select the appropriate role and model for a task.
 * Replaces the old role-first approach with task-first classification.
 */
export function selectRoleForTask(task: string): "coder" | "reviewer" | "researcher" | "explorer" | "runner" {
  const classification = classifyTask(task);
  return taskTypeToRole(classification);
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
