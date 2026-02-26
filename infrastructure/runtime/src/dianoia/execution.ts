// ExecutionOrchestrator — wave-based plan execution, dependency graph, cascade-skip, spawn records
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { PlanningError } from "../koina/errors.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type { PlanningPhase, SpawnRecord } from "./types.js";
import type { PhasePlan } from "./roadmap.js";
import { buildContextPacketSync } from "./context-packet.js";
import { parseDispatchResponse, selectRoleForTask } from "./structured-extraction.js";
import { PhaseExecutor } from "./phase-executor.js";
import { StateReconciler } from "./state-reconciler.js";
import { buildHandoffState, writeHandoffFile, readHandoffFile, clearHandoffFile } from "./handoff.js";
import { buildOrchestratorContext } from "./context-budget.js";

const log = createLogger("dianoia:execution");
const ZOMBIE_THRESHOLD_SECONDS = 600; // Conservative threshold; execution timeouts are per-task, zombies are per-record

export function computeWaves(phases: PlanningPhase[]): PlanningPhase[][] {
  // Unit of parallelism is the PlanningPhase (plan).
  // Uses PhasePlan.dependencies (plan-to-plan), NOT PlanStep.dependsOn (step-to-step within a plan).
  const idSet = new Set(phases.map((p) => p.id));
  const deps = new Map<string, Set<string>>();

  // First pass: prefer column-level dependencies (PlanningPhase.dependencies)
  // over plan-blob dependencies (PhasePlan.dependencies). Column deps are set
  // by the roadmap orchestrator from LLM output; plan deps are often wrong
  // (LLM fills them with package names instead of phase IDs).
  let hasExplicitDeps = false;
  let hasDependencyColumn = false;
  for (const phase of phases) {
    // Column-level dependencies (V27 migration) — preferred source
    // Distinguish between `dependencies: []` (explicit "no deps") and `undefined` (not specified)
    if (phase.dependencies !== undefined && phase.dependencies !== null) {
      hasDependencyColumn = true;
    }
    const columnDeps = (phase.dependencies ?? []).filter((d) => idSet.has(d));
    
    // Fallback to plan-blob dependencies for backward compatibility
    const plan = phase.plan as PhasePlan | null;
    const planDeps = columnDeps.length > 0
      ? columnDeps
      : (plan?.dependencies ?? []).filter((d) => idSet.has(d));
    
    deps.set(phase.id, new Set(planDeps));
    if (planDeps.length > 0) hasExplicitDeps = true;
  }

  // Fallback: if NO phase has valid inter-phase dependencies AND no phase has
  // the dependencies column set (even to []), infer sequential ordering from
  // phaseOrder. `dependencies: []` means "explicitly no deps" — don't override.
  if (!hasExplicitDeps && !hasDependencyColumn && phases.length > 1) {
    const sorted = [...phases].toSorted((a, b) => a.phaseOrder - b.phaseOrder);
    for (let i = 1; i < sorted.length; i++) {
      const prev = sorted[i - 1]!;
      const curr = sorted[i]!;
      deps.get(curr.id)!.add(prev.id);
    }
    log.info(`No explicit inter-phase dependencies found; inferred sequential order from phaseOrder (${sorted.map(p => p.phaseOrder).join(" → ")})`);
  }

  const waves: PlanningPhase[][] = [];
  const completed = new Set<string>();
  let remaining = [...phases];

  while (remaining.length > 0) {
    const wave = remaining.filter((p) =>
      [...(deps.get(p.id) ?? new Set())].every((dep) => completed.has(dep)),
    );
    if (wave.length === 0) {
      // Cycle detected — treat all remaining as one wave to avoid infinite loop
      log.warn("Dependency cycle detected; treating remaining plans as independent wave");
      waves.push(remaining);
      break;
    }
    waves.push(wave);
    wave.forEach((p) => completed.add(p.id));
    remaining = remaining.filter((p) => !wave.some((w) => w.id === p.id));
  }
  return waves;
}

export function directDependents(failedPhaseId: string, allPhases: PlanningPhase[]): PlanningPhase[] {
  // Direct-dependents-only per CONTEXT.md decision:
  // Plan A fails -> skip B (depends on A). Plan C (depends on B) still runs if no other blocker.
  return allPhases.filter((p) => {
    // Prefer column-level dependencies over plan-blob
    const columnDeps = p.dependencies ?? [];
    if (columnDeps.length > 0) {
      return columnDeps.includes(failedPhaseId);
    }
    // Fallback to plan-blob
    const plan = p.plan as PhasePlan | null;
    const phaseDeps = plan?.dependencies ?? [];
    return phaseDeps.includes(failedPhaseId);
  });
}

export function findResumeWave(records: SpawnRecord[]): number {
  if (records.length === 0) return 0;
  const byWave = new Map<number, SpawnRecord[]>();
  for (const r of records) {
    if (!byWave.has(r.waveNumber)) byWave.set(r.waveNumber, []);
    byWave.get(r.waveNumber)!.push(r);
  }
  const sortedWaves = [...byWave.keys()].toSorted((a, b) => a - b);
  for (const waveNum of sortedWaves) {
    const waveRecords = byWave.get(waveNum) ?? [];
    if (waveRecords.some((r) => r.status !== "done" && r.status !== "skipped")) {
      return waveNum;
    }
  }
  return -1; // All waves complete
}

export class ExecutionOrchestrator {
  private store: PlanningStore;
  private workspaceRoot: string | null = null;
  private phaseExecutor: PhaseExecutor | null = null;
  private reconciler: StateReconciler | null = null;

  constructor(
    private db: Database.Database,
    private dispatchTool: ToolHandler,
  ) {
    this.store = new PlanningStore(db);
  }

  /** Set workspace root for context packet assembly from file-backed state */
  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
    // Initialize PhaseExecutor for task-level execution
    this.phaseExecutor = new PhaseExecutor(this.db, {
      workspaceRoot: root,
      maxReviewRounds: 3,
      enableGitCommits: true,
      enableReview: true,
    });
    // Initialize StateReconciler for co-primary file/DB architecture (ENG-01)
    this.reconciler = new StateReconciler(this.db, root);
    // Run reconciliation on startup — ensures files and DB are in sync
    const reconcileResult = this.reconciler.reconcileAll();
    if (reconcileResult.totalErrors > 0) {
      log.warn(`Startup reconciliation had ${reconcileResult.totalErrors} errors across ${reconcileResult.projects.length} projects`);
    }
  }

  /** Get the state reconciler for external use (routes, tools) */
  getReconciler(): StateReconciler | null {
    return this.reconciler;
  }

  /**
   * Build the orchestrator's context — budget-constrained to 40k tokens (ENG-08).
   * Returns only PROJECT.md + ROADMAP.md + current phase status + handoff context.
   */
  getOrchestratorContext(projectId: string): { context: string; withinBudget: boolean } {
    if (!this.workspaceRoot) {
      return { context: "", withinBudget: true };
    }
    const project = this.store.getProjectOrThrow(projectId);
    const phases = this.store.listPhases(projectId);
    const { context, budget } = buildOrchestratorContext({
      workspaceRoot: this.workspaceRoot,
      projectId,
      project,
      phases,
    });
    return { context, withinBudget: budget.withinBudget };
  }

  /**
   * Write a handoff file for session survival (ENG-12).
   * Call before pausing, on distillation, or on error.
   */
  writeHandoff(
    projectId: string,
    phaseId: string,
    currentWave: number,
    totalWaves: number,
    pauseReason: "manual" | "checkpoint" | "crash" | "distillation" | "timeout" | "error",
    pauseDetail: string,
    opts?: {
      currentTaskId?: string;
      currentTaskLabel?: string;
      completedTaskIds?: string[];
      pendingTaskIds?: string[];
      lastCommitHash?: string;
      uncommittedChanges?: string[];
    },
  ): void {
    if (!this.workspaceRoot) return;
    const project = this.store.getProjectOrThrow(projectId);
    const phase = this.store.getPhaseOrThrow(phaseId);
    const state = buildHandoffState({
      store: this.store,
      project,
      phase,
      currentWave,
      totalWaves,
      pauseReason,
      pauseDetail,
      ...opts,
    });
    writeHandoffFile(this.workspaceRoot, state);
  }

  /**
   * Check for and return any pending handoff state for a project (ENG-12).
   */
  getHandoff(projectId: string): ReturnType<typeof readHandoffFile> {
    if (!this.workspaceRoot) return null;
    return readHandoffFile(this.workspaceRoot, projectId);
  }

  /**
   * Clear handoff after successful resume (ENG-12).
   */
  clearHandoff(projectId: string): void {
    if (!this.workspaceRoot) return;
    clearHandoffFile(this.workspaceRoot, projectId);
  }

  async executePhase(
    projectId: string,
    toolContext: ToolContext,
  ): Promise<{ waveCount: number; failed: number; skipped: number }> {
    const project = this.store.getProjectOrThrow(projectId);
    const allPhases = this.store.listPhases(projectId);
    const waves = computeWaves(allPhases);

    // On resume: check for and clear handoff file (ENG-12)
    const handoff = this.getHandoff(projectId);
    if (handoff) {
      log.info(`Resuming from handoff: ${handoff.pauseReason} at wave ${handoff.currentWave + 1}/${handoff.totalWaves}`);
      this.clearHandoff(projectId);
    }

    // On resume: detect and reap zombies (running records older than threshold)
    this.reapZombies(projectId);

    const existingRecords = this.store.listSpawnRecords(projectId);
    const resumeWave = existingRecords.length > 0 ? findResumeWave(existingRecords) : 0;

    let failed = 0;
    let skipped = 0;
    const skippedIds = new Set<string>(
      existingRecords.filter((r) => r.status === "skipped").map((r) => r.phaseId),
    );

    for (let waveIndex = 0; waveIndex < waves.length; waveIndex++) {
      // Skip already-completed waves on resume
      if (resumeWave !== -1 && waveIndex < resumeWave) continue;

      // Check pause flag before each wave (reads from project config)
      if (this.isPaused(projectId)) {
        log.info(`Execution paused for project ${projectId} before wave ${waveIndex}`);
        // Write handoff file for session survival (ENG-12)
        const pausePhase = waves[waveIndex]?.[0];
        if (pausePhase && this.workspaceRoot) {
          this.writeHandoff(projectId, pausePhase.id, waveIndex, waves.length, "manual", "Execution paused by user or config");
        }
        break;
      }

      const wave = waves[waveIndex]!;
      // Re-read records each wave (reapZombies may have mutated earlier records)
      const currentRecords = this.store.listSpawnRecords(projectId);
      const activePlans = wave.filter(
        (p) =>
          !skippedIds.has(p.id) &&
          !currentRecords.some((r) => r.phaseId === p.id && (r.status === "done" || r.status === "running")),
      );

      if (activePlans.length === 0) continue;

      log.info(
        `Wave ${waveIndex + 1}/${waves.length}: dispatching ${activePlans.length} plans for project ${projectId}`,
      );

      // Create spawn records BEFORE dispatch (so crash leaves a recoverable trace)
      const spawnIds: string[] = [];
      for (const plan of activePlans) {
        const record = this.store.createSpawnRecord({
          projectId,
          phaseId: plan.id,
          waveNumber: waveIndex,
        });
        this.store.updateSpawnRecord(record.id, {
          status: "running",
          startedAt: new Date().toISOString(),
        });
        spawnIds.push(record.id);
      }

      // ──────────────────────────────────────────────────────────────────
      // Task-aware execution: if a phase has tasks in TaskStore, execute
      // them individually with verification + review + git commits.
      // Otherwise fall back to the bulk dispatch path.
      // ──────────────────────────────────────────────────────────────────

      // Separate plans into task-based and legacy paths
      const taskBasedPlans: Array<{ plan: PlanningPhase; spawnId: string }> = [];
      const legacyPlans: Array<{ plan: PlanningPhase; spawnId: string }> = [];

      for (let i = 0; i < activePlans.length; i++) {
        const plan = activePlans[i]!;
        const spawnId = spawnIds[i]!;
        if (this.phaseExecutor?.hasTasksForPhase(projectId, plan.id)) {
          taskBasedPlans.push({ plan, spawnId });
        } else {
          legacyPlans.push({ plan, spawnId });
        }
      }

      // Execute task-based plans individually (with verification)
      for (const { plan, spawnId } of taskBasedPlans) {
        try {
          this.store.updatePhaseStatus(plan.id, "executing");

          const phaseResult = await this.phaseExecutor!.executePhase(
            projectId,
            plan.id,
            // Dispatch function: wraps sessions_spawn via the dispatch tool
            async (prompt, role, timeoutSeconds) => {
              const raw = await this.dispatchTool.execute(
                { tasks: [{ role, task: prompt, timeoutSeconds }] },
                toolContext,
              );
              // Extract the result text from dispatch response
              const parsed = await parseDispatchResponse(raw as string);
              return parsed?.results[0]?.result ?? (raw as string);
            },
            // Review function: uses reviewer role
            async (prompt) => {
              const raw = await this.dispatchTool.execute(
                { tasks: [{ role: "reviewer", task: prompt, timeoutSeconds: 120 }] },
                toolContext,
              );
              const parsed = await parseDispatchResponse(raw as string);
              return parsed?.results[0]?.result ?? (raw as string);
            },
          );

          if (phaseResult.failed === 0) {
            this.store.updateSpawnRecord(spawnId, {
              status: "done",
              completedAt: new Date().toISOString(),
            });
            this.store.updatePhaseStatus(plan.id, "complete");
            // Write step-boundary STATE.md (ENG-01)
            this.reconciler?.writeStepBoundaryState(projectId, plan.id, {
              step: "complete",
              label: `Phase "${plan.name}" completed`,
              completedTasks: phaseResult.taskResults.filter(t => t.status === "success").map(t => t.taskId),
            });
            log.info(
              `Phase "${plan.name}" completed via task executor: ${phaseResult.succeeded} tasks, ${phaseResult.commits.length} commits`,
            );
          } else {
            this.store.updateSpawnRecord(spawnId, {
              status: "failed",
              errorMessage: `${phaseResult.failed}/${phaseResult.taskResults.length} tasks failed`,
              completedAt: new Date().toISOString(),
            });
            this.store.updatePhaseStatus(plan.id, "failed");
            failed++;
          }
        } catch (error) {
          this.store.updateSpawnRecord(spawnId, {
            status: "failed",
            errorMessage: error instanceof Error ? error.message : String(error),
            completedAt: new Date().toISOString(),
          });
          this.store.updatePhaseStatus(plan.id, "failed");
          failed++;
        }
      }

      // Legacy bulk dispatch for plans without tasks
      if (legacyPlans.length > 0) {
        const legacyActivePlans = legacyPlans.map(lp => lp.plan);
        const legacySpawnIds = legacyPlans.map(lp => lp.spawnId);

        // Map each plan to appropriate role based on its task content
        const tasks = legacyActivePlans.map((plan) => {
          const contextPacket = this.workspaceRoot
            ? buildContextPacketSync({
                workspaceRoot: this.workspaceRoot,
                projectId,
                phaseId: plan.id,
                role: "executor",
                phase: plan,
                projectGoal: project.goal,
                requirements: this.store
                  .listRequirements(projectId)
                  .filter((r) => r.tier === "v1" && plan.requirements.includes(r.reqId)),
                maxTokens: 12000,
              })
            : buildExecutionPrompt(plan, project.goal);

          const selectedRole = selectRoleForTask(plan.goal + " " + plan.successCriteria.join(" "));
          return { role: selectedRole, task: contextPacket, timeoutSeconds: 900 };
        });

        let dispatchResult: Awaited<ReturnType<typeof parseDispatchResponse>>;
        
        try {
          const raw = await this.dispatchTool.execute({ tasks }, toolContext);
          dispatchResult = await parseDispatchResponse(raw as string, async (errorMessage) => {
            log.warn(`Dispatch response parsing failed: ${errorMessage}. Re-dispatching with error feedback...`);
            const feedbackTasks = tasks.map(t => ({
              ...t,
              task: t.task + `\n\n---\n\n**IMPORTANT: Your previous response failed validation.**\nError: ${errorMessage}\n\nPlease ensure your response ends with a valid JSON block matching the required output format.`,
            }));
            try {
              const retryRaw = await this.dispatchTool.execute({ tasks: feedbackTasks }, toolContext);
              return retryRaw as string;
            } catch (error) {
              throw new PlanningError(`Retry dispatch also failed: ${error instanceof Error ? error.message : String(error)}`, { code: "PLANNING_DISPATCH_FAILED", cause: error instanceof Error ? error : undefined });
            }
          });
          if (!dispatchResult) {
            throw new PlanningError("Failed to parse dispatch response after retry", { code: "PLANNING_DISPATCH_PARSE_FAILED" });
          }
        } catch (error) {
          dispatchResult = {
            taskCount: legacyActivePlans.length,
            succeeded: 0,
            failed: legacyActivePlans.length,
            results: legacyActivePlans.map((_, index) => ({
              index,
              task: legacyActivePlans[index]?.goal ?? "unknown",
              status: "error" as const,
              error: String(error),
              durationMs: 0,
            })),
            timing: { wallClockMs: 0, sequentialMs: 0, savedMs: 0 },
            totalTokens: 0,
          };
        }

        for (let i = 0; i < legacyActivePlans.length; i++) {
          const plan = legacyActivePlans[i]!;
          const spawnId = legacySpawnIds[i]!;
          const result = dispatchResult.results[i];

          if (result?.status === "success") {
            this.store.updateSpawnRecord(spawnId, {
              status: "done",
              completedAt: new Date().toISOString(),
            });
            this.store.updatePhaseStatus(plan.id, "complete");
          } else {
            const errorMessage = result?.error ?? "dispatch failed";
            this.store.updateSpawnRecord(spawnId, {
              status: "failed",
              errorMessage,
              completedAt: new Date().toISOString(),
            });
            this.store.updatePhaseStatus(plan.id, "failed");
            failed++;

            // Cascade-skip direct dependents only (CONTEXT.md: direct-dependents-only rule)
            const dependents = directDependents(plan.id, allPhases);
            for (const dep of dependents) {
              if (!skippedIds.has(dep.id)) {
                const depRecord = this.store.createSpawnRecord({
                  projectId,
                  phaseId: dep.id,
                  waveNumber: waveIndex + 1,
                });
                this.store.updateSpawnRecord(depRecord.id, {
                  status: "skipped",
                  completedAt: new Date().toISOString(),
                });
                this.store.updatePhaseStatus(dep.id, "skipped");
                skippedIds.add(dep.id);
                skipped++;
              }
            }
          }
        }
      } // end legacyPlans block

      // Cascade-skip for task-based plan failures too
      for (const { plan } of taskBasedPlans) {
        const phaseStatus = this.store.getPhaseOrThrow(plan.id).status;
        if (phaseStatus === "failed") {
          const dependents = directDependents(plan.id, allPhases);
          for (const dep of dependents) {
            if (!skippedIds.has(dep.id)) {
              const depRecord = this.store.createSpawnRecord({
                projectId,
                phaseId: dep.id,
                waveNumber: waveIndex + 1,
              });
              this.store.updateSpawnRecord(depRecord.id, {
                status: "skipped",
                completedAt: new Date().toISOString(),
              });
              this.store.updatePhaseStatus(dep.id, "skipped");
              skippedIds.add(dep.id);
              skipped++;
            }
          }
        }
      }
    }

    return { waveCount: waves.length, failed, skipped };
  }

  getExecutionSnapshot(projectId: string): ExecutionSnapshot {
    const project = this.store.getProjectOrThrow(projectId);
    const phases = this.store.listPhases(projectId);
    const records = this.store.listSpawnRecords(projectId);

    const activeWave = records
      .filter((r) => r.status === "running")
      .reduce((max, r) => Math.max(max, r.waveNumber), -1);

    return {
      projectId: project.id,
      state: project.state,
      activeWave: activeWave === -1 ? null : activeWave,
      plans: phases.map((ph) => {
        const record = records.find((r) => r.phaseId === ph.id);
        return {
          phaseId: ph.id,
          name: ph.name,
          status: record?.status ?? "pending",
          waveNumber: record?.waveNumber ?? null,
          startedAt: record?.startedAt ?? null,
          completedAt: record?.completedAt ?? null,
          error: record?.errorMessage ?? null,
        };
      }),
      activePlanIds: records.filter((r) => r.status === "running").map((r) => r.phaseId),
      startedAt: records.length > 0 ? (records[0]?.createdAt ?? null) : null,
      completedAt:
        records.length > 0 &&
        records.every((r) => ["done", "failed", "skipped", "zombie"].includes(r.status))
          ? records.reduce(
              (max, r) => (r.completedAt && r.completedAt > max ? r.completedAt : max),
              "",
            )
          : null,
    };
  }

  private reapZombies(projectId: string): void {
    const records = this.store.listSpawnRecords(projectId);
    const allPhases = this.store.listPhases(projectId);
    const now = Date.now();
    const skippedIds = new Set(records.filter((r) => r.status === "skipped").map((r) => r.phaseId));

    for (const record of records) {
      if (record.status === "running" && record.startedAt) {
        const ageSeconds = (now - new Date(record.startedAt).getTime()) / 1000;
        if (ageSeconds > ZOMBIE_THRESHOLD_SECONDS) {
          log.warn(`Zombie spawn record detected: ${record.id} (age: ${Math.round(ageSeconds)}s)`);
          this.store.updateSpawnRecord(record.id, {
            status: "zombie",
            completedAt: new Date().toISOString(),
          });

          // Cascade-skip direct dependents (same logic as failed plans in executePhase)
          const dependents = directDependents(record.phaseId, allPhases);
          for (const dep of dependents) {
            if (!skippedIds.has(dep.id)) {
              const depRecord = this.store.createSpawnRecord({
                projectId,
                phaseId: dep.id,
                waveNumber: record.waveNumber + 1,
              });
              this.store.updateSpawnRecord(depRecord.id, {
                status: "skipped",
                completedAt: new Date().toISOString(),
              });
              this.store.updatePhaseStatus(dep.id, "skipped");
              skippedIds.add(dep.id);
            }
          }
        }
      }
    }
  }

  private isPaused(projectId: string): boolean {
    const project = this.store.getProjectOrThrow(projectId);
    return project.state === "blocked" || project.config.pause_between_phases === true;
  }
}

export interface PlanEntry {
  phaseId: string;
  name: string;
  status: string;
  waveNumber: number | null;
  startedAt: string | null;
  completedAt: string | null;
  error: string | null;
}

export interface ExecutionSnapshot {
  projectId: string;
  state: string;
  activeWave: number | null;
  plans: PlanEntry[];
  activePlanIds: string[];
  startedAt: string | null;
  completedAt: string | null;
}

function buildExecutionPrompt(phase: PlanningPhase, projectGoal: string): string {
  return [
    `# Execute Phase: ${phase.name}`,
    ``,
    `## Project Goal`,
    projectGoal,
    ``,
    `## Phase Goal`,
    phase.goal,
    ``,
    `## Success Criteria`,
    phase.successCriteria.map((c, i) => `${i + 1}. ${c}`).join("\n"),
    ``,
    `## Phase Plan`,
    phase.plan
      ? JSON.stringify(phase.plan, null, 2)
      : "(no plan — use phase goal and success criteria)",
    ``,
    `---`,
    ``,
    `## Output Format (REQUIRED)`,
    `When done, end your response with:`,
    "```json",
    `{`,
    `  "status": "success" | "partial" | "failed",`,
    `  "summary": "Brief description of what was accomplished",`,
    `  "filesChanged": ["list", "of", "files"],`,
    `  "issues": [],`,
    `  "confidence": 0.0-1.0`,
    `}`,
    "```",
  ].join("\n");
}
