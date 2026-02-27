// Handoff — .continue-here.md session survival files (ENG-12)
//
// When execution pauses (manual, checkpoint, crash, context distillation),
// a .continue-here.md file captures the exact mid-task state so the next
// session can bootstrap instantly without re-reading the entire project.
//
// On resume, the orchestrator reads the handoff file, validates it against
// DB state, and resumes from the captured point.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { getProjectDir } from "./project-files.js";
import type { PlanningStore } from "./store.js";
import type { PlanningPhase, PlanningProject } from "./types.js";

const log = createLogger("dianoia:handoff");

// Re-use atomic write from project-files
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

export interface HandoffState {
  /** Project being executed */
  projectId: string;
  projectGoal: string;
  /** Current phase */
  phaseId: string;
  phaseName: string;
  phaseGoal: string;
  /** Execution progress */
  currentWave: number;
  totalWaves: number;
  /** Task-level progress (if using task executor) */
  currentTaskId: string | null;
  currentTaskLabel: string | null;
  completedTaskIds: string[];
  pendingTaskIds: string[];
  /** What was happening when we paused */
  pauseReason: "manual" | "checkpoint" | "crash" | "distillation" | "timeout" | "error";
  pauseDetail: string;
  /** Git state */
  lastCommitHash: string | null;
  uncommittedChanges: string[];
  /** What to do next */
  resumeAction: string;
  resumeContext: string;
  /** Blockers preventing automatic resume */
  blockers: string[];
  /** Timestamp */
  createdAt: string;
}

/**
 * Write a .continue-here.md handoff file for a project.
 * Written to the project root directory for easy discovery.
 */
export function writeHandoffFile(
  workspaceRoot: string,
  state: HandoffState,
): string {
  const projectDir = getProjectDir(workspaceRoot, state.projectId);
  const filePath = join(projectDir, ".continue-here.md");

  const lines = [
    "# Continue Here",
    "",
    `> Auto-generated handoff file. Resume from this point.`,
    `> Created: ${state.createdAt}`,
    "",
    "## Project",
    "",
    `| Field | Value |`,
    `|-------|-------|`,
    `| ID | \`${state.projectId}\` |`,
    `| Goal | ${state.projectGoal} |`,
    "",
    "## Current Phase",
    "",
    `| Field | Value |`,
    `|-------|-------|`,
    `| Phase | ${state.phaseName} |`,
    `| Phase ID | \`${state.phaseId}\` |`,
    `| Goal | ${state.phaseGoal} |`,
    `| Wave | ${state.currentWave + 1}/${state.totalWaves} |`,
    "",
    "## Progress",
    "",
  ];

  if (state.currentTaskId) {
    lines.push(
      `**Current Task:** ${state.currentTaskLabel ?? state.currentTaskId}`,
      "",
    );
  }

  if (state.completedTaskIds.length > 0) {
    lines.push(`**Completed (${state.completedTaskIds.length}):**`);
    for (const id of state.completedTaskIds) {
      lines.push(`- ✅ ${id}`);
    }
    lines.push("");
  }

  if (state.pendingTaskIds.length > 0) {
    lines.push(`**Pending (${state.pendingTaskIds.length}):**`);
    for (const id of state.pendingTaskIds) {
      lines.push(`- ⬜ ${id}`);
    }
    lines.push("");
  }

  lines.push(
    "## Pause",
    "",
    `**Reason:** ${state.pauseReason}`,
    `**Detail:** ${state.pauseDetail}`,
    "",
  );

  if (state.lastCommitHash) {
    lines.push(`**Last Commit:** \`${state.lastCommitHash}\``);
  }

  if (state.uncommittedChanges.length > 0) {
    lines.push("", "**Uncommitted Changes:**");
    for (const change of state.uncommittedChanges) {
      lines.push(`- ${change}`);
    }
  }

  lines.push("", "");

  if (state.blockers.length > 0) {
    lines.push("## ⚠️ Blockers", "");
    for (const blocker of state.blockers) {
      lines.push(`- ${blocker}`);
    }
    lines.push("");
  }

  lines.push(
    "## Resume",
    "",
    `**Action:** ${state.resumeAction}`,
    "",
    state.resumeContext,
    "",
    "---",
    "",
    "```json",
    JSON.stringify(state, null, 2),
    "```",
  );

  atomicWriteFile(filePath, lines.join("\n"));
  log.info(`Wrote handoff file for project ${state.projectId}: ${state.pauseReason}`);
  return filePath;
}

/**
 * Read and parse a .continue-here.md handoff file.
 * Returns null if no handoff file exists or it's unparseable.
 */
export function readHandoffFile(
  workspaceRoot: string,
  projectId: string,
): HandoffState | null {
  const projectDir = getProjectDir(workspaceRoot, projectId);
  const filePath = join(projectDir, ".continue-here.md");

  if (!existsSync(filePath)) return null;

  try {
    const content = readFileSync(filePath, "utf-8");

    // Extract the JSON block at the end of the file
    const jsonMatch = content.match(/```json\n([\s\S]+?)\n```/);
    if (!jsonMatch?.[1]) {
      log.warn(`Handoff file for ${projectId} missing JSON block — cannot parse`);
      return null;
    }

    const state = JSON.parse(jsonMatch[1]) as HandoffState;

    // Validate required fields
    if (!state.projectId || !state.phaseId || !state.pauseReason) {
      log.warn(`Handoff file for ${projectId} missing required fields`);
      return null;
    }

    return state;
  } catch (error) {
    log.warn(`Failed to read handoff file for ${projectId}: ${error instanceof Error ? error.message : String(error)}`);
    return null;
  }
}

/**
 * Remove the handoff file after successful resume.
 */
export function clearHandoffFile(
  workspaceRoot: string,
  projectId: string,
): boolean {
  const projectDir = getProjectDir(workspaceRoot, projectId);
  const filePath = join(projectDir, ".continue-here.md");

  if (!existsSync(filePath)) return false;

  try {
    unlinkSync(filePath);
    log.info(`Cleared handoff file for project ${projectId}`);
    return true;
  } catch (error) {
    log.warn(`Failed to clear handoff file: ${error instanceof Error ? error.message : String(error)}`);
    return false;
  }
}

/**
 * Discover all projects with pending handoff files.
 * Used on startup to identify interrupted work.
 */
export function discoverHandoffs(workspaceRoot: string): HandoffState[] {
  const projectsDir = join(workspaceRoot, ".dianoia", "projects");
  const handoffs: HandoffState[] = [];

  if (!existsSync(projectsDir)) return handoffs;

  try {
    const entries = readdirSync(projectsDir);

    for (const entry of entries) {
      if (!entry.startsWith("proj_")) continue;

      const handoff = readHandoffFile(workspaceRoot, entry);
      if (handoff) {
        handoffs.push(handoff);
      }
    }
  } catch {
    // Projects dir unreadable
  }

  return handoffs;
}

/**
 * Build a handoff state from current execution context.
 * Call this when pausing or before context distillation.
 */
export function buildHandoffState(opts: {
  store: PlanningStore;
  project: PlanningProject;
  phase: PlanningPhase;
  currentWave: number;
  totalWaves: number;
  currentTaskId?: string | null;
  currentTaskLabel?: string | null;
  completedTaskIds?: string[];
  pendingTaskIds?: string[];
  pauseReason: HandoffState["pauseReason"];
  pauseDetail: string;
  lastCommitHash?: string | null;
  uncommittedChanges?: string[];
}): HandoffState {
  // Build resume instructions based on pause reason
  let resumeAction: string;
  let resumeContext: string;

  switch (opts.pauseReason) {
    case "checkpoint":
      resumeAction = "Resolve checkpoint, then continue execution";
      resumeContext = `A checkpoint requires human decision before proceeding. Check the project's checkpoints via the API or UI.`;
      break;
    case "manual":
      resumeAction = "Resume execution from current wave";
      resumeContext = `Execution was manually paused. Use plan_execute with action=resume to continue.`;
      break;
    case "distillation":
      resumeAction = "Resume execution — context was compacted";
      resumeContext = `Session context was distilled. The new session should read this file and resume from wave ${opts.currentWave + 1}/${opts.totalWaves}.`;
      break;
    case "crash":
    case "error":
      resumeAction = "Investigate failure, then retry or skip";
      resumeContext = `Execution failed unexpectedly. Check the error detail and decide whether to retry the current task or skip it.`;
      break;
    case "timeout":
      resumeAction = "Retry with longer timeout or simplify task";
      resumeContext = `A sub-agent timed out. Consider splitting the task into smaller pieces or increasing the timeout.`;
      break;
    default:
      resumeAction = "Continue execution";
      resumeContext = "";
  }

  // Detect blockers
  const blockers: string[] = [];
  if (opts.pauseReason === "checkpoint") {
    blockers.push("Checkpoint requires human resolution before execution can continue");
  }
  if (opts.uncommittedChanges && opts.uncommittedChanges.length > 0) {
    blockers.push(`${opts.uncommittedChanges.length} uncommitted changes — commit or stash before resuming`);
  }

  return {
    projectId: opts.project.id,
    projectGoal: opts.project.goal,
    phaseId: opts.phase.id,
    phaseName: opts.phase.name,
    phaseGoal: opts.phase.goal,
    currentWave: opts.currentWave,
    totalWaves: opts.totalWaves,
    currentTaskId: opts.currentTaskId ?? null,
    currentTaskLabel: opts.currentTaskLabel ?? null,
    completedTaskIds: opts.completedTaskIds ?? [],
    pendingTaskIds: opts.pendingTaskIds ?? [],
    pauseReason: opts.pauseReason,
    pauseDetail: opts.pauseDetail,
    lastCommitHash: opts.lastCommitHash ?? null,
    uncommittedChanges: opts.uncommittedChanges ?? [],
    resumeAction,
    resumeContext,
    blockers,
    createdAt: new Date().toISOString(),
  };
}
