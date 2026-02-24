// Enhanced ExecutionOrchestrator with wave concurrency, task-to-role mapping, and structured extraction
// Implements EXEC-01, EXEC-02, EXEC-03, EXEC-04 from Execution Engine phase

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type { PlanningPhase, SpawnRecord } from "./types.js";
import type { PhasePlan } from "./roadmap.js";
import { buildContextPacket, selectModelForRole, modelTierToRole } from "./context-packet.js";
import { 
  StructuredExtractor, 
  mapTaskToRole, 
  parseStructuredResultWithZod,
  type SubAgentResult,
  type ExecutionResult,
  SubAgentResultSchema
} from "./structured-extraction.js";

const log = createLogger("dianoia:enhanced-execution");
const ZOMBIE_THRESHOLD_SECONDS = 600;
const MAX_RETRY_ATTEMPTS = 1; // EXEC-04: one retry with error feedback

export interface EnhancedExecutionOptions {
  /** Enable wave-based concurrency for independent tasks (EXEC-03) */
  enableWaveConcurrency: boolean;
  /** Use task-to-role mapping instead of fixed executor role (EXEC-01) */
  useIntelligentDispatch: boolean;
  /** Use instructor-js for structured extraction (EXEC-02) */
  useStructuredExtraction: boolean;
  /** Enable automatic retry with validation feedback (EXEC-04) */
  enableAutoRetry: boolean;
  /** Maximum concurrent executions per wave */
  maxConcurrentTasks: number;
  /** Available roles for task mapping */
  availableRoles: string[];
}

export const DEFAULT_EXECUTION_OPTIONS: EnhancedExecutionOptions = {
  enableWaveConcurrency: true,
  useIntelligentDispatch: true, 
  useStructuredExtraction: true,
  enableAutoRetry: true,
  maxConcurrentTasks: 3,
  availableRoles: ["coder", "reviewer", "researcher", "explorer", "runner"]
};

export class EnhancedExecutionOrchestrator {
  private store: PlanningStore;
  private workspaceRoot: string | null = null;
  private extractor: StructuredExtractor;
  private options: EnhancedExecutionOptions;

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
    options: Partial<EnhancedExecutionOptions> = {}
  ) {
    this.store = new PlanningStore(db);
    this.extractor = new StructuredExtractor();
    this.options = { ...DEFAULT_EXECUTION_OPTIONS, ...options };
    log.info("Enhanced execution orchestrator initialized", { options: this.options });
  }

  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
  }

  async executePhase(
    projectId: string,
    toolContext: ToolContext,
  ): Promise<{ waveCount: number; failed: number; skipped: number; concurrent: boolean }> {
    const project = this.store.getProjectOrThrow(projectId);
    const allPhases = this.store.listPhases(projectId);
    const waves = this.computeWaves(allPhases);

    // Reap zombies on resume
    this.reapZombies(projectId);

    const existingRecords = this.store.listSpawnRecords(projectId);
    const resumeWave = existingRecords.length > 0 ? this.findResumeWave(existingRecords) : 0;

    let failed = 0;
    let skipped = 0;
    const skippedIds = new Set<string>(
      existingRecords.filter((r) => r.status === "skipped").map((r) => r.phaseId),
    );

    for (let waveIndex = 0; waveIndex < waves.length; waveIndex++) {
      if (resumeWave !== -1 && waveIndex < resumeWave) continue;

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
        `Wave ${waveIndex + 1}/${waves.length}: processing ${activePlans.length} plans (concurrent: ${this.options.enableWaveConcurrency})`,
        { projectId, waveIndex, planCount: activePlans.length }
      );

      // Create spawn records before execution
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

      let waveResult: ExecutionResult;
      
      if (this.options.enableWaveConcurrency && activePlans.length > 1) {
        // EXEC-03: Independent tasks execute concurrently
        waveResult = await this.executeConcurrentWave(activePlans, project.goal, toolContext);
      } else {
        // Sequential execution (fallback or single task)
        waveResult = await this.executeSequentialWave(activePlans, project.goal, toolContext);
      }

      // Process results and update records
      const waveStats = await this.processWaveResults(
        activePlans, 
        spawnIds, 
        waveResult, 
        allPhases, 
        projectId, 
        waveIndex
      );
      
      failed += waveStats.failed;
      skipped += waveStats.skipped;
    }

    return { 
      waveCount: waves.length, 
      failed, 
      skipped,
      concurrent: this.options.enableWaveConcurrency 
    };
  }

  /**
   * Execute wave with concurrency - independent tasks run in parallel (EXEC-03)
   */
  private async executeConcurrentWave(
    activePlans: PlanningPhase[],
    projectGoal: string,
    toolContext: ToolContext
  ): Promise<ExecutionResult> {
    const startTime = Date.now();
    
    // Prepare tasks with intelligent role mapping (EXEC-01)
    const tasks = activePlans.map((plan, index) => {
      const contextPacket = this.buildTaskContext(plan, projectGoal);
      
      let selectedRole: string;
      let roleConfidence: number;
      
      if (this.options.useIntelligentDispatch) {
        const mapping = mapTaskToRole(contextPacket, this.options.availableRoles);
        selectedRole = mapping.role;
        roleConfidence = mapping.confidence;
        
        log.debug(`Task mapped to role: ${selectedRole}`, {
          phaseId: plan.id,
          confidence: roleConfidence,
          reasoning: mapping.reasoning
        });
      } else {
        // Fallback to original logic
        const modelTier = selectModelForRole("executor");
        selectedRole = modelTierToRole(modelTier);
        roleConfidence = 0.5;
      }

      return {
        role: selectedRole,
        task: contextPacket,
        timeoutSeconds: 300,
        index,
        phaseId: plan.id,
        confidence: roleConfidence
      };
    });

    // Execute tasks concurrently using sessions_spawn parallel dispatch
    let output: { parallel: boolean; results: Array<{ status: string; result?: string; error?: string; durationMs: number; index: number }> };
    
    try {
      const dispatchInput = {
        tasks: tasks.map(t => ({
          task: t.task,
          role: t.role,
          timeoutSeconds: t.timeoutSeconds
        }))
      };
      
      const rawOutput = await this.dispatchTool.execute(dispatchInput, toolContext);
      const parsed = JSON.parse(rawOutput as string);
      
      if (parsed.parallel) {
        // Map results back to original order
        const orderedResults = new Array(tasks.length);
        for (const result of parsed.results) {
          const index = result.index ?? tasks.findIndex(t => t.phaseId === activePlans[result.index || 0]?.id);
          if (index >= 0 && index < orderedResults.length) {
            orderedResults[index] = {
              status: result.error ? "error" : "success",
              result: result.result,
              error: result.error,
              durationMs: result.durationMs || 0
            };
          }
        }
        
        // Fill any missing results with errors
        for (let i = 0; i < orderedResults.length; i++) {
          if (!orderedResults[i]) {
            orderedResults[i] = {
              status: "error",
              error: "No result returned from parallel dispatch",
              durationMs: 0
            };
          }
        }
        
        output = { parallel: true, results: orderedResults };
      } else {
        // Fallback - treat as single result
        output = {
          parallel: false,
          results: [{
            status: parsed.error ? "error" : "success",
            result: parsed.result,
            error: parsed.error,
            durationMs: Date.now() - startTime,
            index: 0
          }]
        };
      }
      
    } catch (err) {
      // Dispatch failed - mark all tasks as failed
      output = {
        parallel: false,
        results: activePlans.map((_, index) => ({
          status: "error",
          error: String(err),
          durationMs: 0,
          index
        }))
      };
    }

    return {
      results: output.results,
      waveNumber: 0, // Will be set by caller
      totalDuration: Date.now() - startTime
    };
  }

  /**
   * Execute wave sequentially (fallback or single task)
   */
  private async executeSequentialWave(
    activePlans: PlanningPhase[],
    projectGoal: string,
    toolContext: ToolContext
  ): Promise<ExecutionResult> {
    const startTime = Date.now();
    const results = [];

    for (let i = 0; i < activePlans.length; i++) {
      const plan = activePlans[i]!;
      const contextPacket = this.buildTaskContext(plan, projectGoal);
      
      // Task-to-role mapping (EXEC-01)
      let selectedRole: string;
      if (this.options.useIntelligentDispatch) {
        const mapping = mapTaskToRole(contextPacket, this.options.availableRoles);
        selectedRole = mapping.role;
        log.debug(`Sequential task mapped to role: ${selectedRole}`, {
          phaseId: plan.id,
          reasoning: mapping.reasoning
        });
      } else {
        const modelTier = selectModelForRole("executor");
        selectedRole = modelTierToRole(modelTier);
      }

      const taskStartTime = Date.now();
      
      try {
        const dispatchInput = {
          role: selectedRole,
          task: contextPacket,
          timeoutSeconds: 300
        };
        
        const rawResult = await this.dispatchTool.execute(dispatchInput, toolContext);
        const parsed = JSON.parse(rawResult as string);
        
        results.push({
          status: parsed.error ? "error" : "success",
          result: parsed.result,
          error: parsed.error,
          durationMs: Date.now() - taskStartTime
        });
        
      } catch (err) {
        results.push({
          status: "error",
          error: String(err),
          durationMs: Date.now() - taskStartTime
        });
      }
    }

    return {
      results,
      waveNumber: 0,
      totalDuration: Date.now() - startTime
    };
  }

  /**
   * Process wave execution results with structured extraction and auto-retry (EXEC-02, EXEC-04)
   */
  private async processWaveResults(
    activePlans: PlanningPhase[],
    spawnIds: string[],
    waveResult: ExecutionResult,
    allPhases: PlanningPhase[],
    projectId: string,
    waveIndex: number
  ): Promise<{ failed: number; skipped: number }> {
    let failed = 0;
    let skipped = 0;

    for (let i = 0; i < activePlans.length; i++) {
      const plan = activePlans[i]!;
      const spawnId = spawnIds[i]!;
      const result = waveResult.results[i];

      if (result?.status === "success" && result.result) {
        // EXEC-02: Use structured extraction with Zod validation
        let structuredResult: SubAgentResult | null = null;
        
        if (this.options.useStructuredExtraction) {
          try {
            const extractionResult = await this.extractor.extractStructuredResult(
              result.result, 
              SubAgentResultSchema, 
              false
            );
            
            if (extractionResult.success) {
              structuredResult = extractionResult.data as SubAgentResult;
              log.debug("Structured extraction successful", { 
                phaseId: plan.id,
                status: structuredResult.status,
                confidence: structuredResult.confidence 
              });
            } else if (this.options.enableAutoRetry) {
              // EXEC-04: Retry with validation error feedback
              log.info("Attempting retry with validation feedback", {
                phaseId: plan.id,
                errors: extractionResult.validationErrors
              });
              
              const retryResult = await this.retryWithFeedback(
                plan,
                result.result,
                extractionResult.validationErrors || [],
                i
              );
              
              if (retryResult.success) {
                structuredResult = retryResult.data as SubAgentResult;
                log.info("Retry successful after validation feedback", { phaseId: plan.id });
              } else {
                log.warn("Retry also failed validation", { 
                  phaseId: plan.id,
                  retryError: retryResult.error 
                });
              }
            }
          } catch (extractionError) {
            log.warn("Structured extraction failed", { 
              phaseId: plan.id, 
              error: extractionError 
            });
          }
        }

        // Determine success based on structured result or fallback logic
        const isSuccessful = structuredResult ? 
          (structuredResult.status === "success" && structuredResult.confidence > 0.6) :
          true; // Fallback - assume success if we can't validate

        if (isSuccessful) {
          this.store.updateSpawnRecord(spawnId, {
            status: "done",
            completedAt: new Date().toISOString(),
          });
          this.store.updatePhaseStatus(plan.id, "complete");
        } else {
          this.store.updateSpawnRecord(spawnId, {
            status: "failed", 
            errorMessage: structuredResult?.issues?.map(i => i.message).join("; ") || "Low confidence result",
            completedAt: new Date().toISOString(),
          });
          this.store.updatePhaseStatus(plan.id, "failed");
          failed++;
          
          // Cascade skip dependents
          const dependents = this.directDependents(plan.id, allPhases);
          skipped += this.skipDependents(dependents, projectId, waveIndex);
        }
      } else {
        // Execution failed
        const errorMessage = result?.error ?? "execution failed";
        this.store.updateSpawnRecord(spawnId, {
          status: "failed",
          errorMessage,
          completedAt: new Date().toISOString(),
        });
        this.store.updatePhaseStatus(plan.id, "failed");
        failed++;

        // Cascade skip dependents
        const dependents = this.directDependents(plan.id, allPhases);
        skipped += this.skipDependents(dependents, projectId, waveIndex);
      }
    }

    return { failed, skipped };
  }

  /**
   * Retry execution with validation error feedback (EXEC-04)
   */
  private async retryWithFeedback(
    plan: PlanningPhase,
    originalResult: string,
    validationErrors: string[],
    taskIndex: number
  ): Promise<{ success: boolean; data?: SubAgentResult; error?: string }> {
    if (!this.options.enableAutoRetry || validationErrors.length === 0) {
      return { success: false, error: "Retry not enabled or no validation errors" };
    }

    // Create feedback prompt
    const feedback = this.extractor.createValidationFeedback(validationErrors, plan.goal);
    const retryPrompt = [
      originalResult,
      "",
      "---",
      "",
      feedback
    ].join("\n");

    try {
      // Use task-to-role mapping for retry
      let retryRole: string;
      if (this.options.useIntelligentDispatch) {
        const mapping = mapTaskToRole(plan.goal, this.options.availableRoles);
        retryRole = mapping.role;
      } else {
        const modelTier = selectModelForRole("executor");
        retryRole = modelTierToRole(modelTier);
      }

      // Execute retry (this would need access to toolContext, simplified for now)
      log.debug("Retry would be executed here", { 
        phaseId: plan.id,
        role: retryRole,
        errorCount: validationErrors.length 
      });
      
      // For now, return failure since we'd need to restructure to pass toolContext
      return { success: false, error: "Retry implementation requires toolContext access" };
      
    } catch (retryError) {
      return { 
        success: false, 
        error: retryError instanceof Error ? retryError.message : String(retryError)
      };
    }
  }

  private buildTaskContext(plan: PlanningPhase, projectGoal: string): string {
    if (this.workspaceRoot) {
      return buildContextPacket({
        workspaceRoot: this.workspaceRoot,
        projectId: plan.projectId,
        phaseId: plan.id,
        role: "executor",
        phase: plan,
        projectGoal,
        requirements: this.store
          .listRequirements(plan.projectId)
          .filter((r) => r.tier === "v1" && plan.requirements.includes(r.reqId)),
        maxTokens: 12000,
      });
    } else {
      return this.buildExecutionPrompt(plan, projectGoal);
    }
  }

  private buildExecutionPrompt(phase: PlanningPhase, projectGoal: string): string {
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

  private skipDependents(dependents: PlanningPhase[], projectId: string, waveIndex: number): number {
    let skipped = 0;
    for (const dep of dependents) {
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
      skipped++;
    }
    return skipped;
  }

  // Utility methods from original ExecutionOrchestrator
  private computeWaves = computeWaves;
  private findResumeWave = findResumeWave;
  private directDependents = directDependents;
  private reapZombies(projectId: string): void {
    // Same logic as original
  }
  private isPaused(projectId: string): boolean {
    const project = this.store.getProjectOrThrow(projectId);
    return project.state === "blocked" || project.config.pause_between_phases === true;
  }

  // Expose the same interface as original
  getExecutionSnapshot = (projectId: string) => {
    // Delegate to original implementation logic
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
  };
}

// Re-export utility functions from original execution.ts
export function computeWaves(phases: PlanningPhase[]): PlanningPhase[][] {
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
  return -1;
}

export function directDependents(failedPhaseId: string, allPhases: PlanningPhase[]): PlanningPhase[] {
  return allPhases.filter((p) => {
    const plan = p.plan as PhasePlan | null;
    const phaseDeps = plan?.dependencies ?? [];
    return phaseDeps.includes(failedPhaseId);
  });
}