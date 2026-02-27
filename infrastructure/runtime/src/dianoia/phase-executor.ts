// PhaseExecutor — task-aware phase execution that replaces the "dispatch whole phase" pattern
//
// Instead of sending the entire phase plan as a single prompt to a sub-agent (which results
// in ~400-token plan descriptions instead of actual code), this executor:
//
// 1. Reads tasks from TaskStore for the phase
// 2. Orders tasks by dependency (blocked tasks wait)
// 3. Executes each task through TaskExecutor (verify + review + commit)
// 4. Tracks results per-task and aggregates to phase result
//
// Falls back to the old execution path if no tasks exist for the phase.

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { TaskStore, type Task } from "./task-store.js";
import { PlanningStore } from "./store.js";
import {
  TaskExecutor,
  DEFAULT_DEVIATION_RULES,
  type TaskExecutionResult,
  type TaskExecutorConfig,
} from "./task-executor.js";

const log = createLogger("dianoia:phase-executor");

export interface PhaseExecutionResult {
  phaseId: string;
  phaseName: string;
  taskResults: TaskExecutionResult[];
  succeeded: number;
  failed: number;
  skipped: number;
  commits: string[];
  totalDuration: number;
}

export interface PhaseExecutorConfig {
  workspaceRoot: string;
  maxReviewRounds: number;
  enableGitCommits: boolean;
  enableReview: boolean;
}

/**
 * Execute a phase's tasks in dependency order with verification and review.
 *
 * Uses `sessions_dispatch` tool indirectly through a dispatch function wrapper,
 * so the actual sub-agent spawning goes through the existing infrastructure.
 */
export class PhaseExecutor {
  private taskStore: TaskStore;
  private planningStore: PlanningStore;
  private taskExecutor: TaskExecutor;

  constructor(
    db: Database.Database,
    config: PhaseExecutorConfig,
  ) {
    this.taskStore = new TaskStore(db);
    this.planningStore = new PlanningStore(db);

    const execConfig: TaskExecutorConfig = {
      workspaceRoot: config.workspaceRoot,
      maxReviewRounds: config.maxReviewRounds,
      enableGitCommits: config.enableGitCommits,
      enableReview: config.enableReview,
      deviationRules: DEFAULT_DEVIATION_RULES,
    };
    this.taskExecutor = new TaskExecutor(execConfig);
  }

  /**
   * Execute all tasks for a phase in dependency order.
   *
   * @param projectId - Planning project ID
   * @param phaseId - Phase to execute
   * @param dispatchFn - Function that dispatches a prompt to a sub-agent
   * @param reviewFn - Optional function that dispatches a review prompt
   */
  async executePhase(
    projectId: string,
    phaseId: string,
    dispatchFn: (prompt: string, role: string, timeoutSeconds: number) => Promise<string>,
    reviewFn?: (prompt: string) => Promise<string>,
  ): Promise<PhaseExecutionResult> {
    const start = Date.now();
    const phase = this.planningStore.getPhaseOrThrow(phaseId);
    const project = this.planningStore.getProjectOrThrow(projectId);

    // Get tasks for this phase, ordered by dependency
    const tasks = this.taskStore.listTasks({
      projectId,
      phaseId,
    });

    if (tasks.length === 0) {
      log.warn(`No tasks found for phase ${phaseId} — phase has no executable work`);
      return {
        phaseId,
        phaseName: phase.name,
        taskResults: [],
        succeeded: 0,
        failed: 0,
        skipped: 0,
        commits: [],
        totalDuration: Date.now() - start,
      };
    }

    log.info(`Executing phase "${phase.name}" with ${tasks.length} tasks`);

    // Sort tasks: unblocked first, then by priority
    const orderedTasks = orderTasksByDependency(tasks);
    const taskResults: TaskExecutionResult[] = [];
    const commits: string[] = [];
    // Track by human-readable taskId (PROJ-001) since blockedBy uses taskId
    const completedTaskIds = new Set<string>();
    const failedTaskIds = new Set<string>();

    for (const task of orderedTasks) {
      // Check if any blocker failed (blockedBy stores taskId strings)
      const blockerFailed = task.blockedBy.some(dep => failedTaskIds.has(dep));
      if (blockerFailed) {
        log.info(`Skipping task ${task.taskId} — blocked by failed dependency`);
        taskResults.push({
          taskId: task.taskId,
          status: "skipped",
          verification: null,
          reviewRounds: [],
          commitHash: null,
          duration: 0,
          error: "Blocked by failed dependency",
        });
        this.taskStore.updateTask(task.id, { status: "skipped" });
        failedTaskIds.add(task.taskId); // Cascade: dependents of skipped tasks also skip
        continue;
      }

      // Check if blockers are complete
      const blockersComplete = task.blockedBy.every(dep => completedTaskIds.has(dep));
      if (!blockersComplete) {
        log.info(`Skipping task ${task.taskId} — waiting for incomplete dependencies`);
        taskResults.push({
          taskId: task.taskId,
          status: "skipped",
          verification: null,
          reviewRounds: [],
          commitHash: null,
          duration: 0,
          error: "Dependencies not yet complete",
        });
        continue;
      }

      // Mark task as active
      this.taskStore.updateTask(task.id, { status: "active" });

      // Execute
      const result = await this.taskExecutor.executeTask(
        task,
        project.goal,
        phase.goal,
        phase.name,
        dispatchFn,
        reviewFn,
      );

      taskResults.push(result);

      if (result.status === "success") {
        completedTaskIds.add(task.taskId);
        this.taskStore.completeTask(task.id);
        if (result.commitHash) commits.push(result.commitHash);
      } else if (result.status === "failed") {
        failedTaskIds.add(task.taskId);
        this.taskStore.updateTask(task.id, { status: "failed" });
      } else if (result.status === "skipped") {
        this.taskStore.updateTask(task.id, { status: "skipped" });
      }

      // Emit execution progress for file sync daemon
      eventBus.emit("planning:execution-progress", {
        projectId: phase.projectId,
        phaseId,
        step: `task-${taskResults.length}/${orderedTasks.length}`,
        status: result.status,
        taskId: task.taskId,
        commitHash: result.commitHash ?? null,
      });
    }

    const succeeded = taskResults.filter(r => r.status === "success").length;
    const failed = taskResults.filter(r => r.status === "failed").length;
    const skipped = taskResults.filter(r => r.status === "skipped").length;

    log.info(
      `Phase "${phase.name}" complete: ${succeeded} succeeded, ${failed} failed, ${skipped} skipped, ${commits.length} commits`,
    );

    return {
      phaseId,
      phaseName: phase.name,
      taskResults,
      succeeded,
      failed,
      skipped,
      commits,
      totalDuration: Date.now() - start,
    };
  }

  /**
   * Check if a phase has tasks in TaskStore.
   * If not, the old execution path should be used.
   */
  hasTasksForPhase(projectId: string, phaseId: string): boolean {
    const tasks = this.taskStore.listTasks({ projectId, phaseId });
    return tasks.length > 0;
  }
}

/**
 * Order tasks so dependencies come before dependents.
 * Uses Kahn's algorithm (topological sort) with priority as tiebreaker.
 *
 * Note: blockedBy stores human-readable taskId strings (e.g., "PROJ-001"),
 * so we index by taskId for the dependency graph.
 */
export function orderTasksByDependency(tasks: Task[]): Task[] {
  // Index by taskId (human-readable) since blockedBy uses those
  const taskByTaskId = new Map(tasks.map(t => [t.taskId, t]));
  const inDegree = new Map<string, number>();
  const adjacency = new Map<string, string[]>();

  // Initialize
  for (const task of tasks) {
    inDegree.set(task.taskId, 0);
    adjacency.set(task.taskId, []);
  }

  // Build graph: dep taskId → dependent taskId
  for (const task of tasks) {
    for (const dep of task.blockedBy) {
      if (taskByTaskId.has(dep)) {
        adjacency.get(dep)!.push(task.taskId);
        inDegree.set(task.taskId, (inDegree.get(task.taskId) ?? 0) + 1);
      }
    }
  }

  // Priority ordering for tiebreaking
  const priorityOrder = { critical: 0, high: 1, medium: 2, low: 3 };

  // Kahn's algorithm with priority queue
  const queue = tasks
    .filter(t => (inDegree.get(t.taskId) ?? 0) === 0)
    .toSorted((a, b) => (priorityOrder[a.priority] ?? 2) - (priorityOrder[b.priority] ?? 2));

  const result: Task[] = [];

  while (queue.length > 0) {
    const task = queue.shift()!;
    result.push(task);

    for (const dependentTaskId of adjacency.get(task.taskId) ?? []) {
      const newDegree = (inDegree.get(dependentTaskId) ?? 1) - 1;
      inDegree.set(dependentTaskId, newDegree);
      if (newDegree === 0) {
        const dependent = taskByTaskId.get(dependentTaskId);
        if (dependent) {
          // Insert in priority order
          const insertIdx = queue.findIndex(
            q => (priorityOrder[q.priority] ?? 2) > (priorityOrder[dependent.priority] ?? 2),
          );
          if (insertIdx === -1) queue.push(dependent);
          else queue.splice(insertIdx, 0, dependent);
        }
      }
    }
  }

  // Append any remaining tasks (cycles — shouldn't happen with validated deps)
  for (const task of tasks) {
    if (!result.some(r => r.taskId === task.taskId)) {
      result.push(task);
    }
  }

  return result;
}
