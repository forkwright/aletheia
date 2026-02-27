/**
 * TaskStore — CRUD + dependency management for dianoia tasks.
 *
 * Design principles:
 * - SQLite is primary for queries; files (TASKS.md) generated on demand for human reading
 * - Sequential human-readable IDs: PREFIX-001, PREFIX-002, ...
 * - 4-level hierarchy max (project → epic → task → subtask)
 * - Dependency cycle detection via DFS
 * - Optimistic status propagation: completing a task checks if blocked tasks can unblock
 */

import type Database from "better-sqlite3";
import { randomUUID } from "node:crypto";
import { TASK_V1_DDL } from "./task-schema.js";
import { PlanningError } from "../koina/errors.js";

// ─── Types ───────────────────────────────────────────────────

export interface Task {
  id: string;
  projectId: string | null;
  phaseId: string | null;
  parentId: string | null;
  taskId: string;       // human-readable: DAILY-001, PROJ-003
  title: string;
  description: string;
  status: TaskStatus;
  priority: TaskPriority;
  action: string | null;
  verify: string | null;
  files: string[];
  mustHaves: string[];
  contextBudget: number | null;
  blockedBy: string[];
  blocks: string[];
  depth: number;
  assignee: string | null;
  tags: string[];
  completedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export type TaskStatus = "pending" | "active" | "done" | "failed" | "skipped" | "blocked";
export type TaskPriority = "critical" | "high" | "medium" | "low";

export interface CreateTaskOpts {
  projectId?: string | null;
  phaseId?: string | null;
  parentId?: string | null;
  taskId?: string;        // auto-generated if omitted
  title: string;
  description?: string;
  priority?: TaskPriority;
  action?: string;
  verify?: string;
  files?: string[];
  mustHaves?: string[];
  contextBudget?: number;
  blockedBy?: string[];
  assignee?: string;
  tags?: string[];
}

export interface UpdateTaskOpts {
  title?: string;
  description?: string;
  status?: TaskStatus;
  priority?: TaskPriority;
  action?: string | null;
  verify?: string | null;
  files?: string[];
  mustHaves?: string[];
  contextBudget?: number | null;
  blockedBy?: string[];
  assignee?: string | null;
  tags?: string[];
}

export interface TaskFilter {
  projectId?: string;
  phaseId?: string;
  parentId?: string | null; // null = top-level only
  status?: TaskStatus | TaskStatus[];
  priority?: TaskPriority | TaskPriority[];
  assignee?: string;
  tag?: string;
}

const MAX_DEPTH = 3; // 0-indexed: project(0) → epic(1) → task(2) → subtask(3)

// ─── Store ───────────────────────────────────────────────────

export class TaskStore {
  constructor(private db: Database.Database) {}

  /** Initialize task tables (idempotent) */
  initSchema(): void {
    this.db.exec(TASK_V1_DDL);
  }

  // ─── Create ──────────────────────────────────────────────

  createTask(opts: CreateTaskOpts): Task {
    // Validate hierarchy depth
    if (opts.parentId) {
      const parent = this.getTask(opts.parentId) ?? this.getTaskByTaskId(opts.parentId);
      if (!parent) {
        throw new PlanningError(`Parent task not found: ${opts.parentId}`, {
          code: "TASK_PARENT_NOT_FOUND",
          context: { parentId: opts.parentId },
        });
      }
      if (parent.depth >= MAX_DEPTH) {
        throw new PlanningError(`Max hierarchy depth (${MAX_DEPTH + 1} levels) exceeded`, {
          code: "TASK_MAX_DEPTH",
          context: { parentId: opts.parentId, parentDepth: parent.depth },
        });
      }
    }

    // Validate dependencies don't create cycles
    if (opts.blockedBy?.length) {
      for (const dep of opts.blockedBy) {
        const depTask = this.getTaskByTaskId(dep);
        if (!depTask) {
          throw new PlanningError(`Dependency task not found: ${dep}`, {
            code: "TASK_DEP_NOT_FOUND",
            context: { dependency: dep },
          });
        }
      }
    }

    const id = randomUUID();
    const taskId = opts.taskId ?? this.nextTaskId(opts.projectId ?? "DAILY");
    const parentTask = opts.parentId
      ? (this.getTask(opts.parentId) ?? this.getTaskByTaskId(opts.parentId))
      : null;
    const depth = parentTask ? parentTask.depth + 1 : 0;
    const parentDbId = parentTask?.id ?? null;

    // Determine initial status
    const blockedBy = opts.blockedBy ?? [];
    const hasUnfinishedDeps = blockedBy.some(dep => {
      const t = this.getTaskByTaskId(dep);
      return t && t.status !== "done" && t.status !== "skipped";
    });
    const status = hasUnfinishedDeps ? "blocked" : "pending";

    this.db.prepare(`
      INSERT INTO planning_tasks (id, project_id, phase_id, parent_id, task_id, title, description,
        status, priority, action, verify, files, must_haves, context_budget,
        blocked_by, blocks, depth, assignee, tags)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      opts.projectId ?? null,
      opts.phaseId ?? null,
      parentDbId,
      taskId,
      opts.title,
      opts.description ?? "",
      status,
      opts.priority ?? "medium",
      opts.action ?? null,
      opts.verify ?? null,
      JSON.stringify(opts.files ?? []),
      JSON.stringify(opts.mustHaves ?? []),
      opts.contextBudget ?? null,
      JSON.stringify(blockedBy),
      JSON.stringify([]), // blocks computed from reverse lookups
      depth,
      opts.assignee ?? null,
      JSON.stringify(opts.tags ?? []),
    );

    // Update reverse dependency (blocks) on depended tasks
    for (const depTaskId of blockedBy) {
      this.addBlocksEntry(depTaskId, taskId);
    }

    return this.getTaskOrThrow(id);
  }

  // ─── Read ────────────────────────────────────────────────

  getTask(id: string): Task | undefined {
    const row = this.db.prepare("SELECT * FROM planning_tasks WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapTask(row) : undefined;
  }

  getTaskByTaskId(taskId: string): Task | undefined {
    const row = this.db.prepare("SELECT * FROM planning_tasks WHERE task_id = ?")
      .get(taskId) as Record<string, unknown> | undefined;
    return row ? this.mapTask(row) : undefined;
  }

  getTaskOrThrow(id: string): Task {
    const task = this.getTask(id);
    if (!task) {
      throw new PlanningError(`Task not found: ${id}`, {
        code: "TASK_NOT_FOUND", context: { id },
      });
    }
    return task;
  }

  listTasks(filter?: TaskFilter): Task[] {
    const where: string[] = [];
    const params: unknown[] = [];

    if (filter?.projectId !== undefined) {
      if (filter.projectId === "DAILY") {
        where.push("project_id IS NULL");
      } else {
        where.push("project_id = ?");
        params.push(filter.projectId);
      }
    }
    if (filter?.phaseId !== undefined) {
      where.push("phase_id = ?");
      params.push(filter.phaseId);
    }
    if (filter?.parentId !== undefined) {
      if (filter.parentId === null) {
        where.push("parent_id IS NULL");
      } else {
        // Resolve taskId to internal ID
        const parent = this.getTaskByTaskId(filter.parentId) ?? this.getTask(filter.parentId);
        if (parent) {
          where.push("parent_id = ?");
          params.push(parent.id);
        } else {
          return []; // parent not found = no results
        }
      }
    }
    if (filter?.status !== undefined) {
      const statuses = Array.isArray(filter.status) ? filter.status : [filter.status];
      where.push(`status IN (${statuses.map(() => "?").join(",")})`);
      params.push(...statuses);
    }
    if (filter?.priority !== undefined) {
      const priorities = Array.isArray(filter.priority) ? filter.priority : [filter.priority];
      where.push(`priority IN (${priorities.map(() => "?").join(",")})`);
      params.push(...priorities);
    }
    if (filter?.assignee !== undefined) {
      where.push("assignee = ?");
      params.push(filter.assignee);
    }
    if (filter?.tag !== undefined) {
      where.push("tags LIKE ?");
      params.push(`%"${filter.tag}"%`);
    }

    const sql = `SELECT * FROM planning_tasks${where.length ? " WHERE " + where.join(" AND ") : ""} ORDER BY depth ASC, created_at ASC`;
    const rows = this.db.prepare(sql).all(...params) as Array<Record<string, unknown>>;
    return rows.map(r => this.mapTask(r));
  }

  /** Get children of a task */
  getChildren(taskId: string): Task[] {
    const parent = this.getTaskByTaskId(taskId) ?? this.getTask(taskId);
    if (!parent) return [];
    const rows = this.db.prepare("SELECT * FROM planning_tasks WHERE parent_id = ? ORDER BY created_at ASC")
      .all(parent.id) as Array<Record<string, unknown>>;
    return rows.map(r => this.mapTask(r));
  }

  /** Daily tasks: no project, or tagged 'daily' */
  getDailyTasks(): Task[] {
    const rows = this.db.prepare(
      `SELECT * FROM planning_tasks 
       WHERE (project_id IS NULL OR tags LIKE '%"daily"%')
         AND status NOT IN ('done', 'skipped')
       ORDER BY 
         CASE priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 WHEN 'low' THEN 3 END,
         created_at ASC`
    ).all() as Array<Record<string, unknown>>;
    return rows.map(r => this.mapTask(r));
  }

  // ─── Update ──────────────────────────────────────────────

  updateTask(id: string, updates: UpdateTaskOpts): Task {
    const task = this.getTask(id) ?? (() => {
      const byTaskId = this.getTaskByTaskId(id);
      if (byTaskId) return byTaskId;
      throw new PlanningError(`Task not found: ${id}`, { code: "TASK_NOT_FOUND", context: { id } });
    })();

    const update = this.db.transaction(() => {
      const sets: string[] = [];
      const vals: unknown[] = [];

      if (updates.title !== undefined) { sets.push("title = ?"); vals.push(updates.title); }
      if (updates.description !== undefined) { sets.push("description = ?"); vals.push(updates.description); }
      if (updates.status !== undefined) { sets.push("status = ?"); vals.push(updates.status); }
      if (updates.priority !== undefined) { sets.push("priority = ?"); vals.push(updates.priority); }
      if (updates.action !== undefined) { sets.push("action = ?"); vals.push(updates.action); }
      if (updates.verify !== undefined) { sets.push("verify = ?"); vals.push(updates.verify); }
      if (updates.files !== undefined) { sets.push("files = ?"); vals.push(JSON.stringify(updates.files)); }
      if (updates.mustHaves !== undefined) { sets.push("must_haves = ?"); vals.push(JSON.stringify(updates.mustHaves)); }
      if (updates.contextBudget !== undefined) { sets.push("context_budget = ?"); vals.push(updates.contextBudget); }
      if (updates.assignee !== undefined) { sets.push("assignee = ?"); vals.push(updates.assignee); }
      if (updates.tags !== undefined) { sets.push("tags = ?"); vals.push(JSON.stringify(updates.tags)); }

      // Handle dependency changes
      if (updates.blockedBy !== undefined) {
        // Validate no cycles
        this.validateNoCycles(task.taskId, updates.blockedBy);

        // Remove old reverse entries
        for (const oldDep of task.blockedBy) {
          this.removeBlocksEntry(oldDep, task.taskId);
        }
        // Add new reverse entries
        for (const newDep of updates.blockedBy) {
          this.addBlocksEntry(newDep, task.taskId);
        }
        sets.push("blocked_by = ?");
        vals.push(JSON.stringify(updates.blockedBy));
      }

      // Status transitions
      if (updates.status === "done" || updates.status === "skipped") {
        sets.push("completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
      }

      if (sets.length === 0) return;
      sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
      vals.push(task.id);

      this.db.prepare(
        `UPDATE planning_tasks SET ${sets.join(", ")} WHERE id = ?`
      ).run(...vals);

      // If completing, propagate unblocks
      if (updates.status === "done" || updates.status === "skipped") {
        this.propagateUnblocks(task.taskId);
      }
    });
    update();

    return this.getTaskOrThrow(task.id);
  }

  /** Complete a task and propagate dependency unblocks */
  completeTask(id: string): Task {
    return this.updateTask(id, { status: "done" });
  }

  // ─── Delete ──────────────────────────────────────────────

  deleteTask(id: string): void {
    const task = this.getTask(id) ?? (() => {
      const byTaskId = this.getTaskByTaskId(id);
      if (byTaskId) return byTaskId;
      throw new PlanningError(`Task not found: ${id}`, { code: "TASK_NOT_FOUND", context: { id } });
    })();

    this.db.transaction(() => {
      // Remove from blockedBy/blocks of other tasks
      for (const dep of task.blockedBy) {
        this.removeBlocksEntry(dep, task.taskId);
      }
      for (const blocked of task.blocks) {
        this.removeBlockedByEntry(blocked, task.taskId);
      }

      // CASCADE handles children
      this.db.prepare("DELETE FROM planning_tasks WHERE id = ?").run(task.id);
    })();
  }

  // ─── Bulk Operations ────────────────────────────────────

  /** Create tasks from a phase plan (auto-scoped to project/phase) */
  createFromPhasePlan(projectId: string, phaseId: string, tasks: Array<{
    title: string;
    description?: string;
    action?: string;
    verify?: string;
    files?: string[];
    mustHaves?: string[];
    priority?: TaskPriority;
    blockedBy?: string[];
  }>): Task[] {
    return this.db.transaction(() => {
      return tasks.map(t => this.createTask({
        ...t,
        projectId,
        phaseId,
      }));
    })();
  }

  /** Count tasks by status for a project */
  getTaskCounts(projectId?: string): Record<TaskStatus, number> {
    const where = projectId ? "WHERE project_id = ?" : "";
    const params = projectId ? [projectId] : [];
    const rows = this.db.prepare(
      `SELECT status, COUNT(*) as count FROM planning_tasks ${where} GROUP BY status`
    ).all(...params) as Array<{ status: string; count: number }>;

    const counts: Record<TaskStatus, number> = { pending: 0, active: 0, done: 0, failed: 0, skipped: 0, blocked: 0 };
    for (const r of rows) {
      counts[r.status as TaskStatus] = r.count;
    }
    return counts;
  }

  // ─── ID Generation ──────────────────────────────────────

  /** Generate next sequential task ID for a prefix (e.g. "DAILY" → "DAILY-001") */
  nextTaskId(projectIdOrPrefix: string): string {
    // For project-scoped tasks, derive prefix from project name or use project ID
    const prefix = this.resolvePrefix(projectIdOrPrefix);
    const rows = this.db.prepare(
      "SELECT task_id FROM planning_tasks WHERE task_id LIKE ?"
    ).all(`${prefix}-%`) as Array<{ task_id: string }>;

    const nums = rows
      .map(r => {
        const match = r.task_id.match(new RegExp(`^${prefix}-(\\d+)$`));
        return match ? parseInt(match[1]!, 10) : 0;
      })
      .filter(n => !isNaN(n));

    const next = nums.length > 0 ? Math.max(...nums) + 1 : 1;
    return `${prefix}-${String(next).padStart(3, "0")}`;
  }

  private resolvePrefix(projectIdOrPrefix: string): string {
    if (projectIdOrPrefix === "DAILY") return "DAILY";
    // Try to get project name for prefix
    const row = this.db.prepare(
      "SELECT config FROM planning_projects WHERE id = ?"
    ).get(projectIdOrPrefix) as { config: string } | undefined;
    if (row) {
      try {
        const config = JSON.parse(row.config);
        if (config.name) {
          // Convert "My Project" → "MYPR" (first 4 chars of initials or first word)
          return config.name.replace(/[^A-Za-z0-9]/g, "").slice(0, 6).toUpperCase() || "TASK";
        }
      } catch { /* fall through */ }
    }
    return "TASK";
  }

  // ─── Dependencies ───────────────────────────────────────

  /** Check for dependency cycles using DFS */
  private validateNoCycles(taskId: string, newDeps: string[]): void {
    const visited = new Set<string>();

    const dfs = (current: string): boolean => {
      if (current === taskId) return true; // cycle!
      if (visited.has(current)) return false;
      visited.add(current);

      const task = this.getTaskByTaskId(current);
      if (!task) return false;

      for (const dep of task.blockedBy) {
        if (dfs(dep)) return true;
      }
      return false;
    };

    for (const dep of newDeps) {
      visited.clear();
      if (dfs(dep)) {
        throw new PlanningError(`Dependency cycle detected: ${taskId} → ${dep}`, {
          code: "TASK_CYCLE",
          context: { taskId, dependency: dep },
        });
      }
    }
  }

  /** When a task completes, check if any blocked tasks can be unblocked */
  private propagateUnblocks(completedTaskId: string): void {
    // Find all tasks that are blocked by this one
    const rows = this.db.prepare(
      `SELECT * FROM planning_tasks WHERE blocked_by LIKE ? AND status = 'blocked'`
    ).all(`%"${completedTaskId}"%`) as Array<Record<string, unknown>>;

    for (const row of rows) {
      const task = this.mapTask(row);
      // Check if ALL dependencies are now done/skipped
      const allDepsResolved = task.blockedBy.every(dep => {
        const depTask = this.getTaskByTaskId(dep);
        return depTask && (depTask.status === "done" || depTask.status === "skipped");
      });

      if (allDepsResolved) {
        this.db.prepare(
          `UPDATE planning_tasks SET status = 'pending', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`
        ).run(task.id);
      }
    }
  }

  /** Add taskId to the blocks array of depTaskId */
  private addBlocksEntry(depTaskId: string, taskId: string): void {
    const dep = this.getTaskByTaskId(depTaskId);
    if (!dep) return;
    const blocks = [...dep.blocks];
    if (!blocks.includes(taskId)) {
      blocks.push(taskId);
      this.db.prepare(
        `UPDATE planning_tasks SET blocks = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`
      ).run(JSON.stringify(blocks), dep.id);
    }
  }

  /** Remove taskId from the blocks array of depTaskId */
  private removeBlocksEntry(depTaskId: string, taskId: string): void {
    const dep = this.getTaskByTaskId(depTaskId);
    if (!dep) return;
    const blocks = dep.blocks.filter(b => b !== taskId);
    this.db.prepare(
      `UPDATE planning_tasks SET blocks = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`
    ).run(JSON.stringify(blocks), dep.id);
  }

  /** Remove depTaskId from the blockedBy array of taskId */
  private removeBlockedByEntry(taskId: string, depTaskId: string): void {
    const task = this.getTaskByTaskId(taskId);
    if (!task) return;
    const blockedBy = task.blockedBy.filter(b => b !== depTaskId);
    this.db.prepare(
      `UPDATE planning_tasks SET blocked_by = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`
    ).run(JSON.stringify(blockedBy), task.id);
  }

  // ─── Orphan Detection ───────────────────────────────────

  /** Find tasks whose parent has been deleted */
  getOrphans(): Task[] {
    const rows = this.db.prepare(
      `SELECT t.* FROM planning_tasks t
       LEFT JOIN planning_tasks p ON t.parent_id = p.id
       WHERE t.parent_id IS NOT NULL AND p.id IS NULL`
    ).all() as Array<Record<string, unknown>>;
    return rows.map(r => this.mapTask(r));
  }

  // ─── Markdown Generation ────────────────────────────────

  /** Generate TASKS.md content for a project */
  toMarkdown(projectId?: string): string {
    const tasks = this.listTasks(projectId ? { projectId } : undefined);
    if (tasks.length === 0) return "# Tasks\n\nNo tasks yet.\n";

    const counts = this.getTaskCounts(projectId);
    const total = Object.values(counts).reduce((a, b) => a + b, 0);
    const donePercent = total > 0 ? Math.round(((counts.done + counts.skipped) / total) * 100) : 0;

    const lines: string[] = [
      "# Tasks",
      "",
      `**Progress:** ${counts.done + counts.skipped}/${total} (${donePercent}%)`,
      `| Status | Count |`,
      `|--------|-------|`,
      ...Object.entries(counts).map(([s, c]) => `| ${s} | ${c} |`),
      "",
    ];

    // Group by status
    const grouped = new Map<string, Task[]>();
    for (const task of tasks) {
      const group = task.status;
      if (!grouped.has(group)) grouped.set(group, []);
      grouped.get(group)!.push(task);
    }

    const statusOrder: TaskStatus[] = ["active", "blocked", "pending", "done", "failed", "skipped"];
    for (const status of statusOrder) {
      const group = grouped.get(status);
      if (!group?.length) continue;

      const icon = status === "done" ? "✅" : status === "active" ? "🔄" : status === "blocked" ? "⛔" : status === "failed" ? "❌" : status === "pending" ? "⏳" : "⏭️";
      lines.push(`## ${icon} ${status.charAt(0).toUpperCase() + status.slice(1)}`);
      lines.push("");

      for (const task of group) {
        const indent = "  ".repeat(task.depth);
        const prio = task.priority !== "medium" ? ` [${task.priority}]` : "";
        lines.push(`${indent}- **${task.taskId}**: ${task.title}${prio}`);
        if (task.description) {
          lines.push(`${indent}  ${task.description}`);
        }
        if (task.blockedBy.length) {
          lines.push(`${indent}  *Blocked by: ${task.blockedBy.join(", ")}*`);
        }
      }
      lines.push("");
    }

    return lines.join("\n");
  }

  // ─── Internal ───────────────────────────────────────────

  private mapTask(row: Record<string, unknown>): Task {
    return {
      id: row["id"] as string,
      projectId: (row["project_id"] as string) ?? null,
      phaseId: (row["phase_id"] as string) ?? null,
      parentId: (row["parent_id"] as string) ?? null,
      taskId: row["task_id"] as string,
      title: row["title"] as string,
      description: (row["description"] as string) ?? "",
      status: row["status"] as TaskStatus,
      priority: row["priority"] as TaskPriority,
      action: (row["action"] as string) ?? null,
      verify: (row["verify"] as string) ?? null,
      files: JSON.parse((row["files"] as string) || "[]"),
      mustHaves: JSON.parse((row["must_haves"] as string) || "[]"),
      contextBudget: (row["context_budget"] as number) ?? null,
      blockedBy: JSON.parse((row["blocked_by"] as string) || "[]"),
      blocks: JSON.parse((row["blocks"] as string) || "[]"),
      depth: (row["depth"] as number) ?? 0,
      assignee: (row["assignee"] as string) ?? null,
      tags: JSON.parse((row["tags"] as string) || "[]"),
      completedAt: (row["completed_at"] as string) ?? null,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
    };
  }
}
