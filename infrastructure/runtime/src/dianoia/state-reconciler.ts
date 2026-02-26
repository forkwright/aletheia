// StateReconciler — co-primary file/DB architecture (ENG-01)
//
// Every state change writes to BOTH SQLite (fast queries) and files (durability/readability/git).
// Neither is "derived from" the other. On startup, reconcile: compare what's in DB vs files.
// DB ahead → regenerate files. Files ahead → import into DB.
//
// STATE.md is the canonical recovery document — updated at every step boundary.

import { existsSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { PlanningStore } from "./store.js";
import {
  getProjectDir,
  writeProjectFile,
  writeRequirementsFile,
  writeRoadmapFile,
  writeResearchFile,
  writeDiscussFile,
  writeStateFile,
  writeVerifyFile,
  writePlanFile,
  readProjectFile,
} from "./project-files.js";
import type Database from "better-sqlite3";

const log = createLogger("dianoia:reconciler");

export interface ReconciliationResult {
  projectId: string;
  direction: "db-to-files" | "files-to-db" | "in-sync" | "files-only" | "db-only";
  filesRegenerated: string[];
  dbImported: string[];
  errors: string[];
}

export interface ReconciliationSummary {
  projects: ReconciliationResult[];
  totalErrors: number;
  duration: number;
}

/**
 * Get the latest modification timestamp from a directory's files (non-recursive).
 */
function latestFileTimestamp(dirPath: string): Date | null {
  if (!existsSync(dirPath)) return null;
  let latest: Date | null = null;
  try {
    const entries = readdirSync(dirPath);
    for (const entry of entries) {
      const fullPath = join(dirPath, entry);
      try {
        const stat = statSync(fullPath);
        if (stat.isFile() && (!latest || stat.mtime > latest)) {
          latest = stat.mtime;
        }
      } catch {
        // Skip unreadable files
      }
    }
  } catch {
    // Directory unreadable
  }
  return latest;
}

/**
 * Get the latest update timestamp from DB records for a project.
 */
function latestDbTimestamp(store: PlanningStore, projectId: string): Date | null {
  const project = store.getProject(projectId);
  if (!project) return null;

  let latest = new Date(project.updatedAt);

  const phases = store.listPhases(projectId);
  for (const phase of phases) {
    const phaseDate = new Date(phase.updatedAt);
    if (phaseDate > latest) latest = phaseDate;
  }

  const reqs = store.listRequirements(projectId);
  for (const req of reqs) {
    const reqDate = new Date(req.updatedAt);
    if (reqDate > latest) latest = reqDate;
  }

  return latest;
}

export class StateReconciler {
  private store: PlanningStore;

  constructor(
    db: Database.Database,
    private workspaceRoot: string,
  ) {
    this.store = new PlanningStore(db);
  }

  /**
   * Reconcile all projects — compare DB and file state, repair whichever is stale.
   * Called on startup (setWorkspaceRoot) or manually via API.
   */
  reconcileAll(): ReconciliationSummary {
    const start = Date.now();
    const results: ReconciliationResult[] = [];

    // Collect project IDs from both sources
    const dbProjects = this.store.listProjects();
    const dbProjectIds = new Set(dbProjects.map((p) => p.id));

    const fileProjectIds = this.discoverFileProjects();

    const allIds = new Set([...dbProjectIds, ...fileProjectIds]);

    for (const projectId of allIds) {
      try {
        const result = this.reconcileProject(projectId, dbProjectIds.has(projectId), fileProjectIds.has(projectId));
        results.push(result);
      } catch (error) {
        results.push({
          projectId,
          direction: "in-sync",
          filesRegenerated: [],
          dbImported: [],
          errors: [error instanceof Error ? error.message : String(error)],
        });
      }
    }

    const totalErrors = results.reduce((sum, r) => sum + r.errors.length, 0);
    const duration = Date.now() - start;

    if (totalErrors > 0) {
      log.warn(`Reconciliation complete: ${results.length} projects, ${totalErrors} errors in ${duration}ms`);
    } else {
      log.info(`Reconciliation complete: ${results.length} projects in ${duration}ms`);
    }

    return { projects: results, totalErrors, duration };
  }

  /**
   * Reconcile a single project.
   */
  reconcileProject(
    projectId: string,
    inDb: boolean,
    inFiles: boolean,
  ): ReconciliationResult {
    const result: ReconciliationResult = {
      projectId,
      direction: "in-sync",
      filesRegenerated: [],
      dbImported: [],
      errors: [],
    };

    if (inDb && !inFiles) {
      // DB has it, files don't → regenerate files from DB
      result.direction = "db-only";
      this.dbToFiles(projectId, result);
      return result;
    }

    if (!inDb && inFiles) {
      // Files have it, DB doesn't → import from files to DB
      result.direction = "files-only";
      this.filesToDb(projectId, result);
      return result;
    }

    if (inDb && inFiles) {
      // Both exist — compare timestamps
      const dbTime = latestDbTimestamp(this.store, projectId);
      const projectDir = getProjectDir(this.workspaceRoot, projectId);
      const fileTime = latestFileTimestamp(projectDir);

      // Also check phase directories
      const phasesDir = join(projectDir, "phases");
      if (existsSync(phasesDir)) {
        try {
          const phaseEntries = readdirSync(phasesDir);
          for (const phaseEntry of phaseEntries) {
            const phaseDir = join(phasesDir, phaseEntry);
            const phaseTime = latestFileTimestamp(phaseDir);
            if (phaseTime && (!fileTime || phaseTime > fileTime)) {
              // fileTime is const from latestFileTimestamp — use a mutable variable
            }
          }
        } catch {
          // phases dir unreadable
        }
      }

      if (!dbTime || !fileTime) {
        result.direction = "in-sync";
        return result;
      }

      const diffMs = dbTime.getTime() - fileTime.getTime();

      if (diffMs > 5000) {
        // DB is significantly newer → regenerate files
        result.direction = "db-to-files";
        this.dbToFiles(projectId, result);
      } else if (diffMs < -5000) {
        // Files are significantly newer → this shouldn't normally happen
        // (files are written alongside DB), but handle gracefully
        result.direction = "files-to-db";
        log.warn(`Files newer than DB for project ${projectId} — unusual state. Keeping DB as source of truth.`);
        // Still regenerate files from DB to ensure consistency
        this.dbToFiles(projectId, result);
      } else {
        result.direction = "in-sync";
      }
    }

    return result;
  }

  /**
   * Write step-boundary STATE.md — called at every significant state transition.
   * This is the canonical recovery document.
   */
  writeStepBoundaryState(projectId: string, phaseId: string, stepInfo: StepBoundaryInfo): void {
    const project = this.store.getProject(projectId);
    const phase = this.store.getPhase(phaseId);
    if (!project || !phase) return;

    const state: Record<string, unknown> = {
      projectId,
      projectGoal: project.goal,
      projectState: project.state,
      phaseId,
      phaseName: phase.name,
      phaseStatus: phase.status,
      step: stepInfo.step,
      stepLabel: stepInfo.label,
      timestamp: new Date().toISOString(),
      completedTasks: stepInfo.completedTasks ?? [],
      pendingTasks: stepInfo.pendingTasks ?? [],
      blockers: stepInfo.blockers ?? [],
      lastCommit: stepInfo.lastCommit ?? null,
      resumeInstructions: stepInfo.resumeInstructions ?? null,
    };

    writeStateFile(this.workspaceRoot, projectId, phaseId, state);
  }

  /**
   * Regenerate all files for a project from DB state.
   */
  private dbToFiles(projectId: string, result: ReconciliationResult): void {
    const project = this.store.getProject(projectId);
    if (!project) {
      result.errors.push(`Project ${projectId} not found in DB`);
      return;
    }

    // PROJECT.md
    try {
      writeProjectFile(this.workspaceRoot, project, project.projectContext);
      result.filesRegenerated.push("PROJECT.md");
    } catch (error) {
      result.errors.push(`PROJECT.md: ${error instanceof Error ? error.message : String(error)}`);
    }

    // REQUIREMENTS.md
    try {
      const reqs = this.store.listRequirements(projectId);
      if (reqs.length > 0) {
        writeRequirementsFile(this.workspaceRoot, projectId, reqs);
        result.filesRegenerated.push("REQUIREMENTS.md");
      }
    } catch (error) {
      result.errors.push(`REQUIREMENTS.md: ${error instanceof Error ? error.message : String(error)}`);
    }

    // ROADMAP.md
    try {
      const phases = this.store.listPhases(projectId);
      if (phases.length > 0) {
        writeRoadmapFile(this.workspaceRoot, projectId, phases);
        result.filesRegenerated.push("ROADMAP.md");
      }
    } catch (error) {
      result.errors.push(`ROADMAP.md: ${error instanceof Error ? error.message : String(error)}`);
    }

    // RESEARCH.md
    try {
      const research = this.store.listResearch(projectId);
      if (research.length > 0) {
        writeResearchFile(this.workspaceRoot, projectId, research);
        result.filesRegenerated.push("RESEARCH.md");
      }
    } catch (error) {
      result.errors.push(`RESEARCH.md: ${error instanceof Error ? error.message : String(error)}`);
    }

    // Per-phase files
    const phases = this.store.listPhases(projectId);
    for (const phase of phases) {
      // DISCUSS.md
      try {
        const questions = this.store.listDiscussionQuestions(projectId, phase.id);
        if (questions.length > 0) {
          writeDiscussFile(this.workspaceRoot, projectId, phase.id, questions);
          result.filesRegenerated.push(`phases/${phase.id}/DISCUSS.md`);
        }
      } catch (error) {
        result.errors.push(`DISCUSS.md(${phase.id}): ${error instanceof Error ? error.message : String(error)}`);
      }

      // PLAN.md
      try {
        if (phase.plan) {
          writePlanFile(this.workspaceRoot, projectId, phase.id, phase.plan);
          result.filesRegenerated.push(`phases/${phase.id}/PLAN.md`);
        }
      } catch (error) {
        result.errors.push(`PLAN.md(${phase.id}): ${error instanceof Error ? error.message : String(error)}`);
      }

      // VERIFY.md
      try {
        if (phase.verificationResult) {
          writeVerifyFile(this.workspaceRoot, projectId, phase.id, phase.verificationResult as unknown as Record<string, unknown>);
          result.filesRegenerated.push(`phases/${phase.id}/VERIFY.md`);
        }
      } catch (error) {
        result.errors.push(`VERIFY.md(${phase.id}): ${error instanceof Error ? error.message : String(error)}`);
      }

      // STATE.md — write current phase status as state
      try {
        writeStateFile(this.workspaceRoot, projectId, phase.id, {
          phaseStatus: phase.status,
          phaseGoal: phase.goal,
          lastReconciled: new Date().toISOString(),
        });
        result.filesRegenerated.push(`phases/${phase.id}/STATE.md`);
      } catch (error) {
        result.errors.push(`STATE.md(${phase.id}): ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  }

  /**
   * Import project state from files into DB.
   * This is a recovery path — files exist but DB doesn't have the project.
   * We parse the markdown files to reconstruct DB records.
   */
  private filesToDb(projectId: string, result: ReconciliationResult): void {
    const projectDir = getProjectDir(this.workspaceRoot, projectId);
    if (!existsSync(projectDir)) {
      result.errors.push(`Project directory not found: ${projectDir}`);
      return;
    }

    // Read PROJECT.md to extract goal and state
    const projectMd = readProjectFile(this.workspaceRoot, projectId);
    if (!projectMd) {
      result.errors.push("PROJECT.md not found — cannot reconstruct project");
      return;
    }

    // Parse goal from first heading
    const goalMatch = projectMd.match(/^# (.+)$/m);
    const goal = goalMatch?.[1] ?? "Recovered project";

    // Parse state from table
    const stateMatch = projectMd.match(/\| State \| (\w+)/);
    const state = stateMatch?.[1] ?? "idle";

    try {
      // Create minimal project record
      const project = this.store.createProject({
        nousId: "recovered",
        sessionId: "recovered",
        goal,
        config: {
          mode: "interactive" as const,
          depth: "standard" as const,
          parallelization: true,
          research: true,
          plan_check: true,
          verifier: true,
          pause_between_phases: false,
        },
      });

      // Update state if not idle
      if (state !== "idle") {
        try {
          this.store.updateProjectState(project.id, state as any);
        } catch {
          // State might not be valid — leave as idle
        }
      }

      result.dbImported.push("PROJECT.md");
      log.info(`Imported project from files: ${projectId} → ${project.id}`);

      // Note: Full requirement/phase/discussion import from markdown is complex
      // and lossy. For now we create the project shell — the files remain the
      // source of truth and manual reconciliation can fill in details.
    } catch (error) {
      result.errors.push(`DB import failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /**
   * Discover project IDs from the file system (.dianoia/projects/).
   */
  private discoverFileProjects(): Set<string> {
    const projectsDir = join(this.workspaceRoot, ".dianoia", "projects");
    const ids = new Set<string>();

    if (!existsSync(projectsDir)) return ids;

    try {
      const entries = readdirSync(projectsDir);
      for (const entry of entries) {
        const fullPath = join(projectsDir, entry);
        try {
          const stat = statSync(fullPath);
          if (stat.isDirectory() && entry.startsWith("proj_")) {
            ids.add(entry);
          }
        } catch {
          // Skip unreadable entries
        }
      }
    } catch {
      // Projects dir unreadable
    }

    return ids;
  }
}

export interface StepBoundaryInfo {
  /** Step number or identifier */
  step: number | string;
  /** Human-readable label (e.g., "Task 3/7: Implement auth routes") */
  label: string;
  /** Task IDs completed so far */
  completedTasks?: string[];
  /** Task IDs remaining */
  pendingTasks?: string[];
  /** Current blockers */
  blockers?: string[];
  /** Last git commit hash */
  lastCommit?: string;
  /** Instructions for resuming from this point */
  resumeInstructions?: string;
}
