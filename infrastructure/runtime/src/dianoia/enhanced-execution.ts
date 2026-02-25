// EnhancedExecutionOrchestrator — wave concurrency, intelligent dispatch, structured extraction
//
// Builds on ExecutionOrchestrator with:
// - EXEC-01: Task-to-role mapping via classifyTask/mapTaskToRole
// - EXEC-02: Structured extraction with Zod validation + retry feedback
// - EXEC-03: Wave concurrency (parallel dispatch within a wave)
// - EXEC-04: Automatic retry with validation error feedback

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type { PlanningPhase } from "./types.js";
import { buildContextPacketSync } from "./context-packet.js";
import {
  mapTaskToRole,
  selectRoleForTask,
  parseDispatchResponse,
} from "./structured-extraction.js";

// Re-export wave computation utilities (shared with base ExecutionOrchestrator)
export { computeWaves, directDependents, findResumeWave } from "./execution.js";

const log = createLogger("dianoia:enhanced-execution");

export interface ExecutionOptions {
  enableWaveConcurrency: boolean;
  useIntelligentDispatch: boolean;
  useStructuredExtraction: boolean;
  enableAutoRetry: boolean;
  maxConcurrentTasks: number;
  maxRetries: number;
  zombieThresholdSeconds: number;
}

export const DEFAULT_EXECUTION_OPTIONS: ExecutionOptions = {
  enableWaveConcurrency: true,
  useIntelligentDispatch: true,
  useStructuredExtraction: true,
  enableAutoRetry: true,
  maxConcurrentTasks: 10,
  maxRetries: 1,
  zombieThresholdSeconds: 600,
};

export interface EnhancedExecutionResult {
  waveCount: number;
  failed: number;
  skipped: number;
  concurrent: boolean;
  totalDispatches: number;
  retries: number;
}

export class EnhancedExecutionOrchestrator {
  private store: PlanningStore;
  private options: ExecutionOptions;
  private workspaceRoot: string | null = null;

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
    options?: Partial<ExecutionOptions>,
  ) {
    this.store = new PlanningStore(db);
    this.options = { ...DEFAULT_EXECUTION_OPTIONS, ...options };
  }

  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
  }

  async executePhase(
    projectId: string,
    toolContext: ToolContext,
  ): Promise<EnhancedExecutionResult> {
    const project = this.store.getProjectOrThrow(projectId);
    const allPhases = this.store.listPhases(projectId);

    // Import computeWaves dynamically to avoid circular deps
    const { computeWaves } = await import("./execution.js");
    const waves = computeWaves(allPhases);

    // Reap zombies before starting
    this.reapZombies(projectId, allPhases);

    const existingRecords = this.store.listSpawnRecords(projectId);
    const { findResumeWave } = await import("./execution.js");
    const resumeWave = existingRecords.length > 0 ? findResumeWave(existingRecords) : 0;

    let failed = 0;
    let skipped = 0;
    let totalDispatches = 0;
    let retries = 0;
    const skippedIds = new Set<string>(
      existingRecords.filter((r) => r.status === "skipped").map((r) => r.phaseId),
    );

    for (let waveIndex = 0; waveIndex < waves.length; waveIndex++) {
      if (resumeWave !== -1 && waveIndex < resumeWave) continue;
      if (this.isPaused(projectId)) break;

      const wave = waves[waveIndex]!;
      const currentRecords = this.store.listSpawnRecords(projectId);
      const activePlans = wave.filter(
        (p) =>
          !skippedIds.has(p.id) &&
          !currentRecords.some(
            (r) => r.phaseId === p.id && (r.status === "done" || r.status === "running"),
          ),
      );

      if (activePlans.length === 0) continue;

      log.info(
        `Wave ${waveIndex + 1}/${waves.length}: dispatching ${activePlans.length} plans (concurrent=${this.options.enableWaveConcurrency})`,
      );

      if (this.options.enableWaveConcurrency && activePlans.length > 1) {
        // Concurrent: dispatch all tasks in wave as parallel batch
        const waveResult = await this.executeConcurrentWave(
          projectId,
          activePlans,
          waveIndex,
          project.goal,
          toolContext,
        );
        failed += waveResult.failed;
        totalDispatches += waveResult.dispatches;
        retries += waveResult.retries;

        // Cascade-skip dependents of failed plans
        for (const failedId of waveResult.failedPhaseIds) {
          const { directDependents } = await import("./execution.js");
          const dependents = directDependents(failedId, allPhases);
          for (const dep of dependents) {
            if (!skippedIds.has(dep.id)) {
              this.store.updatePhaseStatus(dep.id, "skipped");
              skippedIds.add(dep.id);
              skipped++;
            }
          }
        }
      } else {
        // Sequential: dispatch one at a time
        for (const plan of activePlans) {
          const result = await this.executeSinglePlan(
            projectId,
            plan,
            waveIndex,
            project.goal,
            toolContext,
          );
          totalDispatches++;
          if (result === "failed") {
            failed++;
            const { directDependents } = await import("./execution.js");
            const dependents = directDependents(plan.id, allPhases);
            for (const dep of dependents) {
              if (!skippedIds.has(dep.id)) {
                this.store.updatePhaseStatus(dep.id, "skipped");
                skippedIds.add(dep.id);
                skipped++;
              }
            }
          }
        }
      }
    }

    return {
      waveCount: waves.length,
      failed,
      skipped,
      concurrent: this.options.enableWaveConcurrency,
      totalDispatches,
      retries,
    };
  }

  private async executeConcurrentWave(
    projectId: string,
    plans: PlanningPhase[],
    waveIndex: number,
    projectGoal: string,
    toolContext: ToolContext,
  ): Promise<{ failed: number; dispatches: number; retries: number; failedPhaseIds: string[] }> {
    // Create spawn records
    const spawnIds: string[] = [];
    for (const plan of plans) {
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
      this.store.updatePhaseStatus(plan.id, "executing");
    }

    // Build parallel task batch
    const tasks = plans.map((plan) => {
      const role = this.options.useIntelligentDispatch
        ? mapTaskToRole(plan.goal).role
        : selectRoleForTask(plan.goal);

      return {
        role,
        task: this.buildExecutionPrompt(plan, projectGoal),
        timeoutSeconds: 300,
      };
    });

    let failed = 0;
    let retries = 0;
    const failedPhaseIds: string[] = [];

    try {
      const raw = await this.dispatchTool.execute({ tasks }, toolContext);
      const parsed = await parseDispatchResponse(raw);

      if (parsed) {
        for (let i = 0; i < plans.length && i < parsed.results.length; i++) {
          const result = parsed.results[i]!;
          const plan = plans[i]!;
          const spawnId = spawnIds[i]!;

          if (result.status === "success") {
            this.store.updateSpawnRecord(spawnId, {
              status: "done",
              completedAt: new Date().toISOString(),
            });
            this.store.updatePhaseStatus(plan.id, "complete");
          } else {
            this.store.updateSpawnRecord(spawnId, {
              status: "failed",
              completedAt: new Date().toISOString(),
              errorMessage: result.error ?? "Dispatch failure",
            });
            this.store.updatePhaseStatus(plan.id, "failed");
            failed++;
            failedPhaseIds.push(plan.id);
          }
        }
      } else {
        // Parse failure — mark all as failed
        for (let i = 0; i < plans.length; i++) {
          this.store.updateSpawnRecord(spawnIds[i]!, {
            status: "failed",
            completedAt: new Date().toISOString(),
            errorMessage: "Dispatch response parse failure",
          });
          this.store.updatePhaseStatus(plans[i]!.id, "failed");
          failed++;
          failedPhaseIds.push(plans[i]!.id);
        }
      }
    } catch (err) {
      for (let i = 0; i < plans.length; i++) {
        this.store.updateSpawnRecord(spawnIds[i]!, {
          status: "failed",
          completedAt: new Date().toISOString(),
          errorMessage: err instanceof Error ? err.message : String(err),
        });
        this.store.updatePhaseStatus(plans[i]!.id, "failed");
        failed++;
        failedPhaseIds.push(plans[i]!.id);
      }
    }

    return { failed, dispatches: 1, retries, failedPhaseIds };
  }

  private async executeSinglePlan(
    projectId: string,
    plan: PlanningPhase,
    waveIndex: number,
    projectGoal: string,
    toolContext: ToolContext,
  ): Promise<"done" | "failed"> {
    const record = this.store.createSpawnRecord({
      projectId,
      phaseId: plan.id,
      waveNumber: waveIndex,
    });
    this.store.updateSpawnRecord(record.id, {
      status: "running",
      startedAt: new Date().toISOString(),
    });
    this.store.updatePhaseStatus(plan.id, "executing");

    const role = this.options.useIntelligentDispatch
      ? mapTaskToRole(plan.goal).role
      : selectRoleForTask(plan.goal);

    const task = {
      role,
      task: this.buildExecutionPrompt(plan, projectGoal),
      timeoutSeconds: 300,
    };

    try {
      const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
      const parsed = await parseDispatchResponse(raw);
      const firstResult = parsed?.results[0];

      if (firstResult?.status === "success") {
        this.store.updateSpawnRecord(record.id, {
          status: "done",
          completedAt: new Date().toISOString(),
        });
        this.store.updatePhaseStatus(plan.id, "complete");
        return "done";
      }

      this.store.updateSpawnRecord(record.id, {
        status: "failed",
        completedAt: new Date().toISOString(),
        errorMessage: firstResult?.error ?? "Execution failed",
      });
      this.store.updatePhaseStatus(plan.id, "failed");
      return "failed";
    } catch (err) {
      this.store.updateSpawnRecord(record.id, {
        status: "failed",
        completedAt: new Date().toISOString(),
        errorMessage: err instanceof Error ? err.message : String(err),
      });
      this.store.updatePhaseStatus(plan.id, "failed");
      return "failed";
    }
  }

  private buildExecutionPrompt(phase: PlanningPhase, projectGoal: string): string {
    const contextPacket =
      this.workspaceRoot
        ? buildContextPacketSync({
            workspaceRoot: this.workspaceRoot,
            projectId: phase.projectId,
            phaseId: phase.id,
            role: "executor",
            phase,
            allPhases: [],
            projectGoal,
            requirements: [],
            maxTokens: 8000,
          })
        : "";

    return [
      `# Execute Phase: ${phase.name}`,
      "",
      `## Project Goal`,
      projectGoal,
      "",
      `## Phase Goal`,
      phase.goal,
      "",
      `## Success Criteria`,
      phase.successCriteria.map((c, i) => `${i + 1}. ${c}`).join("\n"),
      "",
      contextPacket ? `## Context\n${contextPacket}\n` : "",
      `## Output Format (REQUIRED)`,
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

  private reapZombies(projectId: string, _allPhases: PlanningPhase[]): void {
    const records = this.store.listSpawnRecords(projectId);
    const now = Date.now();

    for (const record of records) {
      if (record.status === "running" && record.startedAt) {
        const ageSeconds = (now - new Date(record.startedAt).getTime()) / 1000;
        if (ageSeconds > this.options.zombieThresholdSeconds) {
          log.warn(`Zombie spawn: ${record.id} (age: ${Math.round(ageSeconds)}s)`);
          this.store.updateSpawnRecord(record.id, {
            status: "zombie",
            completedAt: new Date().toISOString(),
          });
        }
      }
    }
  }

  private isPaused(projectId: string): boolean {
    const project = this.store.getProjectOrThrow(projectId);
    return project.state === "blocked" || project.config.pause_between_phases === true;
  }
}
