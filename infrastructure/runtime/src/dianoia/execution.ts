// ExecutionOrchestrator — wave-based plan execution, dependency graph, cascade-skip, spawn records
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type { PlanningPhase, SpawnRecord } from "./types.js";
import type { PhasePlan } from "./roadmap.js";

const log = createLogger("dianoia:execution");
const ZOMBIE_THRESHOLD_SECONDS = 600; // 2x default 300s plan timeout

export function computeWaves(phases: PlanningPhase[]): PlanningPhase[][] {
  // Unit of parallelism is the PlanningPhase (plan).
  // Uses PhasePlan.dependencies (plan-to-plan), NOT PlanStep.dependsOn (step-to-step within a plan).
  const idSet = new Set(phases.map((p) => p.id));
  const deps = new Map<string, Set<string>>();
  for (const phase of phases) {
    const plan = phase.plan as PhasePlan | null;
    const planDeps = (plan?.dependencies ?? []).filter((d) => idSet.has(d));
    deps.set(phase.id, new Set(planDeps));
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
  const sortedWaves = [...byWave.keys()].sort((a, b) => a - b);
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

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
  ) {
    this.store = new PlanningStore(db);
  }

  async executePhase(
    projectId: string,
    toolContext: ToolContext,
  ): Promise<{ waveCount: number; failed: number; skipped: number }> {
    const project = this.store.getProjectOrThrow(projectId);
    const allPhases = this.store.listPhases(projectId);
    const waves = computeWaves(allPhases);

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
        break;
      }

      const wave = waves[waveIndex]!;
      const activePlans = wave.filter(
        (p) =>
          !skippedIds.has(p.id) &&
          !existingRecords.some((r) => r.phaseId === p.id && r.status === "done"),
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

      const tasks = activePlans.map((plan) => ({
        role: "coder" as const,
        task: buildExecutionPrompt(plan, project.goal),
        timeoutSeconds: 300,
      }));

      let output: {
        results: Array<{ status: string; result?: string; error?: string; durationMs: number }>;
      };
      try {
        const raw = await this.dispatchTool.execute({ tasks }, toolContext);
        output = JSON.parse(raw as string) as typeof output;
      } catch (err) {
        // Dispatch itself failed — mark all as failed
        output = {
          results: activePlans.map(() => ({
            status: "error",
            error: String(err),
            durationMs: 0,
          })),
        };
      }

      for (let i = 0; i < activePlans.length; i++) {
        const plan = activePlans[i]!;
        const spawnId = spawnIds[i]!;
        const result = output.results[i];

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
  ].join("\n");
}
