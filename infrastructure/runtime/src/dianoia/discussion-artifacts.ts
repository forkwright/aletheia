// DiscussionArtifacts — structured gray-area documentation (ENG-02)
//
// Enhances DISCUSS.md with four structured sections:
// 1. Phase Boundary — what's in scope vs what belongs to other phases
// 2. Implementation Decisions — architectural choices made during discussion
// 3. Claude's Discretion — things the agent can decide without asking
// 4. Deferred Ideas — good ideas that belong in v2/later
//
// Also provides lock semantics for concurrent access and feeds decisions
// into the planning pipeline.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import {
  ensurePhaseDir,
  getPhaseDir,
} from "./project-files.js";
import type { DiscussionQuestion } from "./types.js";

// Re-use atomic write
import { writeFileSync, renameSync, unlinkSync } from "node:fs";

function atomicWriteFile(filePath: string, content: string): void {
  const tmpPath = `${filePath}.tmp`;
  try {
    writeFileSync(tmpPath, content, "utf-8");
    renameSync(tmpPath, filePath);
  } catch (error) {
    try { if (existsSync(tmpPath)) unlinkSync(tmpPath); } catch { /* ignore */ }
    throw error;
  }
}

const log = createLogger("dianoia:discuss");

export interface BoundaryItem {
  /** What's being bounded */
  item: string;
  /** Where it belongs: "in-scope", "other-phase", "out-of-scope" */
  scope: "in-scope" | "other-phase" | "out-of-scope";
  /** Which phase (if other-phase) */
  targetPhase?: string;
  /** Why this boundary was drawn */
  rationale: string;
}

export interface ImplementationDecision {
  /** What was decided */
  decision: string;
  /** Alternatives that were considered */
  alternatives: string[];
  /** Why this alternative was chosen */
  rationale: string;
  /** Impact level */
  impact: "low" | "medium" | "high";
  /** Decision source */
  source: "human" | "agent" | "discussion";
}

export interface DiscretionItem {
  /** What the agent can decide without asking */
  item: string;
  /** Constraints on the discretion */
  constraints: string[];
  /** What would require escalation */
  escalationTrigger: string;
}

export interface DeferredIdea {
  /** The idea */
  idea: string;
  /** When it should be revisited */
  targetPhase: string;
  /** Why it's deferred */
  rationale: string;
  /** How important is it (for prioritization) */
  priority: "low" | "medium" | "high";
}

export interface DiscussionArtifact {
  /** Phase this artifact belongs to */
  phaseId: string;
  projectId: string;
  /** Phase boundary decisions */
  boundaries: BoundaryItem[];
  /** Implementation decisions made during discussion */
  decisions: ImplementationDecision[];
  /** Things the agent can decide autonomously */
  discretion: DiscretionItem[];
  /** Ideas deferred to later phases */
  deferred: DeferredIdea[];
  /** Raw discussion questions (preserved) */
  questions: DiscussionQuestion[];
  /** Timestamp */
  updatedAt: string;
}

/**
 * Write a structured DISCUSS.md with all four artifact sections.
 */
export function writeStructuredDiscussFile(
  workspaceRoot: string,
  artifact: DiscussionArtifact,
): void {
  const dir = ensurePhaseDir(workspaceRoot, artifact.projectId, artifact.phaseId);
  const lines: string[] = [];

  lines.push("# Phase Discussion Artifacts", "");
  lines.push(`*Updated: ${artifact.updatedAt}*`, "");

  // Section 1: Phase Boundary
  lines.push("## Phase Boundary", "");
  if (artifact.boundaries.length === 0) {
    lines.push("*No explicit boundaries defined.*", "");
  } else {
    lines.push("| Item | Scope | Target | Rationale |");
    lines.push("|------|-------|--------|-----------|");
    for (const b of artifact.boundaries) {
      const target = b.scope === "other-phase" ? (b.targetPhase ?? "—") : "—";
      lines.push(`| ${b.item} | ${b.scope} | ${target} | ${b.rationale} |`);
    }
    lines.push("");
  }

  // Section 2: Implementation Decisions
  lines.push("## Implementation Decisions", "");
  if (artifact.decisions.length === 0) {
    lines.push("*No implementation decisions recorded.*", "");
  } else {
    for (const d of artifact.decisions) {
      const impactIcon = d.impact === "high" ? "🔴" : d.impact === "medium" ? "🟡" : "🟢";
      lines.push(`### ${impactIcon} ${d.decision}`, "");
      lines.push(`**Source:** ${d.source} | **Impact:** ${d.impact}`, "");
      lines.push(`**Rationale:** ${d.rationale}`, "");
      if (d.alternatives.length > 0) {
        lines.push("**Alternatives considered:**");
        for (const alt of d.alternatives) {
          lines.push(`- ${alt}`);
        }
      }
      lines.push("");
    }
  }

  // Section 3: Claude's Discretion
  lines.push("## Claude's Discretion", "");
  lines.push("*Things the agent can decide without asking.*", "");
  if (artifact.discretion.length === 0) {
    lines.push("*No discretion items defined.*", "");
  } else {
    for (const item of artifact.discretion) {
      lines.push(`### ${item.item}`, "");
      if (item.constraints.length > 0) {
        lines.push("**Constraints:**");
        for (const c of item.constraints) {
          lines.push(`- ${c}`);
        }
      }
      lines.push(`**Escalate if:** ${item.escalationTrigger}`, "");
    }
  }

  // Section 4: Deferred Ideas
  lines.push("## Deferred Ideas", "");
  if (artifact.deferred.length === 0) {
    lines.push("*No deferred ideas.*", "");
  } else {
    lines.push("| Idea | Target Phase | Priority | Rationale |");
    lines.push("|------|-------------|----------|-----------|");
    for (const d of artifact.deferred) {
      const priorityIcon = d.priority === "high" ? "🔴" : d.priority === "medium" ? "🟡" : "🟢";
      lines.push(`| ${d.idea} | ${d.targetPhase} | ${priorityIcon} ${d.priority} | ${d.rationale} |`);
    }
    lines.push("");
  }

  // Section 5: Raw Discussion (preserved for context)
  if (artifact.questions.length > 0) {
    lines.push("## Discussion Questions", "");
    for (const q of artifact.questions) {
      const statusIcon = q.status === "answered" ? "✅" :
        q.status === "skipped" ? "⏭" : "❓";
      lines.push(`### ${statusIcon} ${q.question}`, "");

      if (q.options.length > 0) {
        lines.push("**Options:**");
        for (const opt of q.options) {
          const selected = q.decision === opt.label ? " ← **selected**" : "";
          lines.push(`- **${opt.label}:** ${opt.rationale}${selected}`);
        }
        lines.push("");
      }

      if (q.recommendation) {
        lines.push(`**Recommended:** ${q.recommendation}`, "");
      }

      if (q.decision) {
        lines.push(`**Decision:** ${q.decision}`, "");
      }

      if (q.userNote) {
        lines.push(`**Note:** ${q.userNote}`, "");
      }
    }
  }

  // Append machine-readable JSON trailer
  lines.push("---", "");
  lines.push("```json");
  lines.push(JSON.stringify({
    boundaries: artifact.boundaries,
    decisions: artifact.decisions,
    discretion: artifact.discretion,
    deferred: artifact.deferred,
  }, null, 2));
  lines.push("```");

  const filePath = join(dir, "DISCUSS.md");
  atomicWriteFile(filePath, lines.join("\n"));
  log.debug(`Wrote structured DISCUSS.md for phase ${artifact.phaseId}`);
}

/**
 * Read and parse a structured DISCUSS.md file.
 * Returns the structured artifact sections from the JSON trailer.
 */
export function readStructuredDiscussFile(
  workspaceRoot: string,
  projectId: string,
  phaseId: string,
): { boundaries: BoundaryItem[]; decisions: ImplementationDecision[]; discretion: DiscretionItem[]; deferred: DeferredIdea[] } | null {
  const dir = getPhaseDir(workspaceRoot, projectId, phaseId);
  const filePath = join(dir, "DISCUSS.md");

  if (!existsSync(filePath)) return null;

  try {
    const content = readFileSync(filePath, "utf-8");
    const jsonMatch = content.match(/```json\n([\s\S]+?)\n```/);
    if (!jsonMatch?.[1]) return null;

    return JSON.parse(jsonMatch[1]);
  } catch {
    return null;
  }
}

/**
 * Extract implementation decisions from discussion questions.
 * Converts answered questions into structured decisions.
 */
export function extractDecisionsFromQuestions(
  questions: DiscussionQuestion[],
): ImplementationDecision[] {
  return questions
    .filter((q) => q.status === "answered" && q.decision)
    .map((q) => ({
      decision: `${q.question} → ${q.decision}`,
      alternatives: q.options.map((o) => o.label).filter((l) => l !== q.decision),
      rationale: q.userNote ?? q.options.find((o) => o.label === q.decision)?.rationale ?? "",
      impact: "medium" as const,
      source: q.userNote ? ("human" as const) : ("discussion" as const),
    }));
}

/**
 * Create a minimal empty artifact for a phase.
 */
export function createEmptyArtifact(projectId: string, phaseId: string): DiscussionArtifact {
  return {
    phaseId,
    projectId,
    boundaries: [],
    decisions: [],
    discretion: [],
    deferred: [],
    questions: [],
    updatedAt: new Date().toISOString(),
  };
}

// --- Lock semantics ---

/**
 * Check if a discussion file is locked (being edited by another session).
 * Lock is a .discuss.lock file with session ID and timestamp.
 */
export function isDiscussionLocked(
  workspaceRoot: string,
  projectId: string,
  phaseId: string,
): { locked: boolean; lockedBy?: string; lockedAt?: string } {
  const dir = getPhaseDir(workspaceRoot, projectId, phaseId);
  const lockPath = join(dir, ".discuss.lock");

  if (!existsSync(lockPath)) return { locked: false };

  try {
    const content = readFileSync(lockPath, "utf-8");
    const lock = JSON.parse(content);

    // Lock expires after 5 minutes (stale lock protection)
    const lockTime = new Date(lock.lockedAt).getTime();
    if (Date.now() - lockTime > 5 * 60 * 1000) {
      // Stale lock — treat as unlocked
      return { locked: false };
    }

    return { locked: true, lockedBy: lock.lockedBy, lockedAt: lock.lockedAt };
  } catch {
    return { locked: false };
  }
}

/**
 * Acquire a discussion lock.
 */
export function acquireDiscussionLock(
  workspaceRoot: string,
  projectId: string,
  phaseId: string,
  sessionId: string,
): boolean {
  const status = isDiscussionLocked(workspaceRoot, projectId, phaseId);
  if (status.locked && status.lockedBy !== sessionId) {
    return false;
  }

  const dir = ensurePhaseDir(workspaceRoot, projectId, phaseId);
  const lockPath = join(dir, ".discuss.lock");

  try {
    atomicWriteFile(lockPath, JSON.stringify({
      lockedBy: sessionId,
      lockedAt: new Date().toISOString(),
    }));
    return true;
  } catch {
    return false;
  }
}

/**
 * Release a discussion lock.
 */
export function releaseDiscussionLock(
  workspaceRoot: string,
  projectId: string,
  phaseId: string,
  sessionId: string,
): boolean {
  const status = isDiscussionLocked(workspaceRoot, projectId, phaseId);
  if (!status.locked) return true;
  if (status.lockedBy !== sessionId) return false;

  const dir = getPhaseDir(workspaceRoot, projectId, phaseId);
  const lockPath = join(dir, ".discuss.lock");

  try {
    if (existsSync(lockPath)) unlinkSync(lockPath);
    return true;
  } catch {
    return false;
  }
}
