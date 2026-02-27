// orchestrator-context.ts — Compact context assembly for the orchestrator (Spec 32 Phase 1b)
//
// After distillation, the orchestrator loses its understanding of active projects.
// This module reads the file-backed state and assembles a compact context summary
// that can be injected into the system prompt or prepended to the next turn.
//
// Target: orchestrator stays under 4k tokens of planning context regardless of
// project complexity. The files hold the details; this holds the map.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { PlanningStore } from "./store.js";
import {
  getPhaseDir,
} from "./project-files.js";
import type Database from "better-sqlite3";
import type { PlanningPhase, PlanningProject } from "./types.js";

const log = createLogger("dianoia:orchestrator-context");

export interface OrchestratorContextOptions {
  /** Database for structured queries */
  db: Database.Database;
  /** Only include active projects (default: true) */
  activeOnly?: boolean;
  /** Maximum tokens for the summary (default: 4000) */
  maxTokens?: number;
  /** Include phase-level detail (default: true for active phases) */
  includePhaseDetail?: boolean;
}

export interface OrchestratorContextResult {
  /** The assembled context string */
  context: string;
  /** Number of projects included */
  projectCount: number;
  /** Estimated token count */
  estimatedTokens: number;
  /** Active project IDs */
  activeProjectIds: string[];
}

const ACTIVE_STATES = new Set([
  "idle", "questioning", "researching",
  "requirements", "roadmap", "phase-planning",
  "discussing", "executing", "verifying", "blocked",
]);

/**
 * Assemble a compact orchestrator context from file-backed state.
 *
 * Reads PROJECT.md, ROADMAP.md, and current-phase STATE.md for each active project.
 * Produces a markdown summary that fits within the token budget.
 *
 * This is the function called after distillation to restore planning awareness.
 */
export function assembleOrchestratorContext(opts: OrchestratorContextOptions): OrchestratorContextResult {
  const { db, activeOnly = true, maxTokens = 4000, includePhaseDetail = true } = opts;
  const store = new PlanningStore(db);

  const allProjects = store.listProjects();
  const projects = activeOnly
    ? allProjects.filter(p => ACTIVE_STATES.has(p.state))
    : allProjects;

  if (projects.length === 0) {
    return {
      context: "",
      projectCount: 0,
      estimatedTokens: 0,
      activeProjectIds: [],
    };
  }

  const sections: string[] = [];
  sections.push("# Active Planning Projects\n");

  for (const project of projects) {
    const projectContext = buildProjectSummary(store, project, includePhaseDetail);
    sections.push(projectContext);
  }

  let context = sections.join("\n");
  const estimatedTokens = Math.ceil(context.length / 4);

  // Trim if over budget — remove phase detail first, then project detail
  if (estimatedTokens > maxTokens) {
    // Rebuild without phase detail
    const trimmed: string[] = [];
    trimmed.push("# Active Planning Projects\n");
    for (const project of projects) {
      trimmed.push(buildProjectSummary(store, project, false));
    }
    context = trimmed.join("\n");
  }

  const finalTokens = Math.ceil(context.length / 4);

  log.debug(`Orchestrator context assembled: ${projects.length} projects, ~${finalTokens} tokens`);

  return {
    context,
    projectCount: projects.length,
    estimatedTokens: finalTokens,
    activeProjectIds: projects.map(p => p.id),
  };
}

/**
 * Build a compact summary for a single project.
 */
function buildProjectSummary(
  store: PlanningStore,
  project: PlanningProject,
  includePhaseDetail: boolean,
): string {
  const lines: string[] = [];

  // Project header
  lines.push(`## ${project.goal || "Untitled"} (\`${project.id}\`)`);
  lines.push(`**State:** ${project.state} | **Created:** ${project.createdAt.split("T")[0]}`);

  // Key decisions from context
  const ctx = project.projectContext;
  if (ctx?.keyDecisions?.length) {
    lines.push("\n**Key Decisions:**");
    for (const d of ctx.keyDecisions.slice(0, 5)) {
      lines.push(`- ${d}`);
    }
  }
  if (ctx?.constraints?.length) {
    lines.push("\n**Constraints:**");
    for (const c of ctx.constraints.slice(0, 3)) {
      lines.push(`- ${c}`);
    }
  }

  // Phase overview
  const phases = store.listPhases(project.id);
  if (phases.length > 0) {
    lines.push("\n**Phases:**");
    for (const phase of phases) {
      const icon = phaseIcon(phase.status);
      const current = isCurrentPhase(project.state, phase) ? " ← current" : "";
      lines.push(`${icon} ${phase.name}${current}`);

      // Phase detail for current/active phases only
      if (includePhaseDetail && isCurrentPhase(project.state, phase) && project.projectDir) {
        const stateFile = readPhaseState(project.projectDir, phase.id);
        if (stateFile) {
          lines.push(`  ${stateFile}`);
        }
        // Include pending discussion count if in discussing state
        if (project.state === "discussing") {
          const questions = store.listDiscussionQuestions(project.id, phase.id);
          const pending = questions.filter(q => q.status === "pending").length;
          if (pending > 0) {
            lines.push(`  ⚠️ ${pending} pending discussion questions`);
          }
        }
      }
    }
  }

  // Pending messages
  const messages = store.listMessages(project.id);
  const unread = messages.filter(m => m.status === "pending");
  if (unread.length > 0) {
    lines.push(`\n📬 ${unread.length} unread messages`);
  }

  lines.push("");
  return lines.join("\n");
}

/**
 * Read the most recent STATE.md for a phase and return a one-line summary.
 */
function readPhaseState(projectDirValue: string, phaseId: string): string | null {
  const stateFile = join(getPhaseDir(projectDirValue, phaseId), "STATE.md");
  if (!existsSync(stateFile)) return null;

  try {
    const content = readFileSync(stateFile, "utf-8");
    // Extract step and status from the JSON block
    const jsonMatch = content.match(/```json\n([\s\S]*?)\n```/);
    if (jsonMatch?.[1]) {
      const state = JSON.parse(jsonMatch[1]);
      const parts: string[] = [];
      if (state.step) parts.push(`Step: ${state.step}`);
      if (state.stepLabel) parts.push(state.stepLabel);
      if (state.blockers?.length) parts.push(`🚫 ${state.blockers.length} blockers`);
      if (state.lastCommit) parts.push(`Last commit: ${state.lastCommit.slice(0, 8)}`);
      return parts.join(" | ");
    }
  } catch {
    // Corrupted STATE.md — skip
  }
  return null;
}

/**
 * Determine if a phase is the "current" one being worked on.
 * Uses string comparison because project state includes values like "verifying"
 * and "discussing" that don't map 1:1 to PlanningPhase.status.
 */
function isCurrentPhase(projectState: string, phase: PlanningPhase): boolean {
  const status = phase.status as string;
  if (projectState === "executing" || projectState === "verifying") {
    return status === "executing";
  }
  if (projectState === "discussing" || projectState === "phase-planning") {
    // Current phase is the first pending or executing one
    return status === "pending" || status === "executing";
  }
  return false;
}

function phaseIcon(status: string): string {
  switch (status) {
    case "complete": return "  ✅";
    case "executing": return "  🔄";
    case "failed": return "  ❌";
    case "skipped": return "  ⏭";
    default: return "  ⬜";
  }
}

/**
 * Get a list of all project directories on disk.
 * Useful for discovering projects that might not be in DB (recovery path).
 */
export function discoverProjectDirs(workspaceRoot: string): string[] {
  const projectsDir = join(workspaceRoot, ".dianoia", "projects");
  if (!existsSync(projectsDir)) return [];

  try {
    return readdirSync(projectsDir)
      .filter(entry => {
        const full = join(projectsDir, entry);
        return statSync(full).isDirectory() && entry.startsWith("proj_");
      });
  } catch {
    return [];
  }
}
