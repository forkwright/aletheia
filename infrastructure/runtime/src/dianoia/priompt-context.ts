// Enhanced context assembly with accurate tiktoken tokenization
// Improves on hand-rolled assembleSections() with js-tiktoken for precise token counting

import { getEncoding } from "js-tiktoken";
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
import type { SubAgentRole } from "./context-packet.js";

const log = createLogger("dianoia:priompt-context");

export interface PriomptContextOptions {
  /** Project directory value (slug for new projects, absolute path for legacy) */
  projectDirValue: string;
  /** Phase ID (null for project-level dispatches) */
  phaseId: string | null;
  /** Role determines which sections are included and prioritized */
  role: SubAgentRole;
  /** Maximum tokens for the context packet (default: 8000) */
  maxTokens?: number;
  /** Phase object with goal/criteria/plan */
  phase?: PlanningPhase | null;
  /** All phases for roadmap context */
  allPhases?: PlanningPhase[];
  /** Filtered requirements relevant to this phase */
  requirements?: PlanningRequirement[];
  /** Project goal string */
  projectGoal?: string;
  /** Additional context to append */
  supplementary?: string;
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

interface ContextSection {
  header: string;
  content: string;
  priority: number;
  tokenCount: number;
}

/**
 * Build a scoped context packet with accurate tiktoken tokenization (sync).
 * Uses js-tiktoken for precise token counting instead of character estimation.
 */
export function buildContextPacketWithPriomptSync(opts: PriomptContextOptions): string {
  return buildContextPacketWithPriomptImpl(opts);
}

/**
 * Async wrapper — preserved for backward compatibility.
 * @deprecated Use buildContextPacketWithPriomptSync() instead.
 */
export function buildContextPacketWithPriompt(opts: PriomptContextOptions): Promise<string> {
  return Promise.resolve(buildContextPacketWithPriomptImpl(opts));
}

function buildContextPacketWithPriomptImpl(opts: PriomptContextOptions): string {
  const maxTokens = opts.maxTokens ?? 8000;
  const config = ROLE_SECTIONS[opts.role];
  const sections: ContextSection[] = [];

  try {
    // Build sections in priority order
    
    // Priority 0: Phase goal and success criteria
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
      
      const sectionText = `## Phase Objective\n\n${lines.join("\n")}\n\n`;
      sections.push({
        header: "Phase Objective",
        content: sectionText,
        priority: 0,
        tokenCount: countTokens(sectionText)
      });
    }

    // Priority 1: Project goal
    if (opts.projectGoal) {
      const sectionText = `## Project Goal\n\n${opts.projectGoal}\n\n`;
      sections.push({
        header: "Project Goal",
        content: sectionText,
        priority: 1,
        tokenCount: countTokens(sectionText)
      });
    }

    // Priority 2: Execution plan
    if (config.includePlan && opts.phaseId) {
      const plan = readPlanFile(opts.projectDirValue, opts.phaseId);
      let planContent: string | null = null;
      
      if (plan) {
        planContent = plan;
      } else if (opts.phase?.plan) {
        planContent = typeof opts.phase.plan === "string"
          ? opts.phase.plan
          : JSON.stringify(opts.phase.plan, null, 2);
      }
      
      if (planContent) {
        const sectionText = `## Execution Plan\n\n${planContent}\n\n`;
        sections.push({
          header: "Execution Plan",
          content: sectionText,
          priority: 2,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Priority 3: Design decisions
    if (config.includeDiscussion && opts.phaseId) {
      const discuss = readDiscussFile(opts.projectDirValue, opts.phaseId);
      if (discuss) {
        const sectionText = `## Design Decisions\n\n${discuss}\n\n`;
        sections.push({
          header: "Design Decisions",
          content: sectionText,
          priority: 3,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Priority 4: Requirements
    if (config.includeRequirements) {
      let content: string | null = null;
      
      if (opts.requirements && opts.requirements.length > 0) {
        content = formatRequirements(opts.requirements);
      } else {
        const reqFile = readRequirementsFile(opts.projectDirValue);
        if (reqFile) {
          content = opts.phase
            ? filterRequirementsToPhase(reqFile, opts.phase.requirements)
            : reqFile;
        }
      }
      
      if (content) {
        const sectionText = `## Requirements\n\n${content}\n\n`;
        sections.push({
          header: "Requirements",
          content: sectionText,
          priority: 4,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Priority 5: Supplementary context
    if (config.includeSupplementary && opts.supplementary) {
      const sectionText = `## Reference Material\n\n${opts.supplementary}\n\n`;
      sections.push({
        header: "Reference Material",
        content: sectionText,
        priority: 5,
        tokenCount: countTokens(sectionText)
      });
    }

    // Priority 6: Roadmap overview
    if (config.includeRoadmap) {
      let content: string | null = null;
      
      if (opts.allPhases && opts.allPhases.length > 0) {
        content = formatRoadmapSummary(opts.allPhases);
      } else {
        const roadmap = readRoadmapFile(opts.projectDirValue);
        if (roadmap) {
          content = roadmap;
        }
      }
      
      if (content) {
        const sectionText = `## Roadmap Overview\n\n${content}\n\n`;
        sections.push({
          header: "Roadmap Overview",
          content: sectionText,
          priority: 6,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Priority 7: Research findings
    if (config.includeResearch) {
      const research = readResearchFile(opts.projectDirValue);
      if (research) {
        const sectionText = `## Research Findings\n\n${research}\n\n`;
        sections.push({
          header: "Research Findings",
          content: sectionText,
          priority: 7,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Priority 8: Project context (lowest priority)
    if (config.includeProject) {
      const projectMd = readProjectFile(opts.projectDirValue);
      if (projectMd) {
        const sectionText = `## Project Context\n\n${projectMd}\n\n`;
        sections.push({
          header: "Project Context",
          content: sectionText,
          priority: 8,
          tokenCount: countTokens(sectionText)
        });
      }
    }

    // Assemble sections using improved algorithm
    const result = assembleWithAccurateTokens(sections, maxTokens);

    log.debug(`Context packet assembled with accurate tokenization: ${result.totalTokens} tokens`, {
      role: opts.role,
      tokenBudget: maxTokens,
      utilization: `${Math.round((result.totalTokens / maxTokens) * 100)}%`,
      sections: result.includedSections.length,
      truncated: result.truncated
    });

    // If nothing assembled (bad parameters, empty budget, missing data), return error
    if (!result.content || result.content.trim().length === 0) {
      log.warn("Context packet assembled but empty — bad parameters or insufficient budget");
      return "# Context Error\n\nContext packet is empty. Check budget and parameters.";
    }

    return result.content;
  } catch (error) {
    log.error(`Failed to render context packet: ${error instanceof Error ? error.message : String(error)}`);
    // Fallback to empty context rather than throwing
    return "# Context Error\n\nFailed to assemble context packet.";
  }
}

/**
 * Assemble sections using accurate token counting and priority-based inclusion.
 * Replaces the character-estimation heuristic with precise tiktoken counts.
 */
function assembleWithAccurateTokens(sections: ContextSection[], maxTokens: number) {
  // Sort by priority (lower = higher priority)
  const sorted = [...sections].toSorted((a, b) => a.priority - b.priority);
  
  const includedSections: ContextSection[] = [];
  let totalTokens = 0;
  let truncated = false;
  
  for (const section of sorted) {
    if (totalTokens + section.tokenCount <= maxTokens) {
      // Section fits entirely
      includedSections.push(section);
      totalTokens += section.tokenCount;
    } else {
      // Partial fit - try to truncate the section
      const remainingTokens = maxTokens - totalTokens;
      if (remainingTokens > 50) { // Only truncate if we have meaningful space
        const truncatedContent = truncateToTokens(section.content, remainingTokens - 10); // Reserve tokens for ellipsis
        if (truncatedContent.length > 0) {
          const truncatedSection: ContextSection = {
            ...section,
            content: truncatedContent + "\n\n[...truncated]",
            tokenCount: countTokens(truncatedContent + "\n\n[...truncated]")
          };
          includedSections.push(truncatedSection);
          totalTokens += truncatedSection.tokenCount;
          truncated = true;
        }
      }
      break; // No room for more sections
    }
  }
  
  const content = includedSections.map(s => s.content).join("").trim();
  
  return {
    content,
    totalTokens,
    includedSections: includedSections.map(s => s.header),
    truncated
  };
}

/**
 * Truncate text to approximately fit within a token budget.
 * Uses binary search for precision.
 */
function truncateToTokens(text: string, targetTokens: number): string {
  if (countTokens(text) <= targetTokens) {
    return text;
  }
  
  let left = 0;
  let right = text.length;
  let bestFit = "";
  
  // Binary search for the longest prefix that fits
  while (left <= right) {
    const mid = Math.floor((left + right) / 2);
    const candidate = text.slice(0, mid);
    const tokens = countTokens(candidate);
    
    if (tokens <= targetTokens) {
      bestFit = candidate;
      left = mid + 1;
    } else {
      right = mid - 1;
    }
  }
  
  return bestFit;
}

// Helper functions (copied from context-packet.ts)

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