/**
 * Task API routes — /api/planning/tasks
 *
 * CRUD endpoints + daily task list + bulk operations.
 * Emits SSE events on mutations for UI sync.
 */
import { Hono } from "hono";
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import type { RouteDeps, RouteRefs } from "../pylon/routes/deps.js";
import { TaskStore } from "./task-store.js";
import type { TaskFilter, CreateTaskOpts, UpdateTaskOpts, TaskStatus, TaskPriority } from "./task-store.js";

const log = createLogger("pylon:tasks");

export function taskRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();

  let _store: TaskStore | null = null;
  function store(): TaskStore {
    if (!_store) {
      try {
        const db = deps.store.getDb();
        _store = new TaskStore(db);
        _store.initSchema();
      } catch {
        throw new Error("Task store not available");
      }
    }
    return _store;
  }

  // ─── List / Filter ───────────────────────────────────────

  app.get("/api/tasks", (c) => {
    try {
      const filter: TaskFilter = {};
      const projectId = c.req.query("projectId");
      const phaseId = c.req.query("phaseId");
      const parentId = c.req.query("parentId");
      const status = c.req.query("status");
      const priority = c.req.query("priority");
      const assignee = c.req.query("assignee");
      const tag = c.req.query("tag");

      if (projectId) filter.projectId = projectId;
      if (phaseId) filter.phaseId = phaseId;
      if (parentId !== undefined) filter.parentId = parentId === "null" ? null : parentId;
      if (status) filter.status = status.includes(",") ? status.split(",") as TaskStatus[] : status as TaskStatus;
      if (priority) filter.priority = priority.includes(",") ? priority.split(",") as TaskPriority[] : priority as TaskPriority;
      if (assignee) filter.assignee = assignee;
      if (tag) filter.tag = tag;

      const tasks = store().listTasks(Object.keys(filter).length > 0 ? filter : undefined);
      return c.json({ tasks, count: tasks.length });
    } catch (err) {
      log.error("Failed to list tasks", { error: err });
      return c.json({ error: "Failed to list tasks" }, 500);
    }
  });

  // ─── Daily Tasks ─────────────────────────────────────────

  app.get("/api/tasks/daily", (c) => {
    try {
      const tasks = store().getDailyTasks();
      return c.json({ tasks, count: tasks.length });
    } catch (err) {
      log.error("Failed to get daily tasks", { error: err });
      return c.json({ error: "Failed to get daily tasks" }, 500);
    }
  });

  // ─── Get Single Task ────────────────────────────────────

  app.get("/api/tasks/:id", (c) => {
    try {
      const id = c.req.param("id");
      const task = store().getTask(id) ?? store().getTaskByTaskId(id);
      if (!task) return c.json({ error: "Task not found" }, 404);
      return c.json(task);
    } catch (err) {
      log.error("Failed to get task", { error: err });
      return c.json({ error: "Failed to get task" }, 500);
    }
  });

  // ─── Get Children ───────────────────────────────────────

  app.get("/api/tasks/:id/children", (c) => {
    try {
      const id = c.req.param("id");
      const children = store().getChildren(id);
      return c.json({ tasks: children, count: children.length });
    } catch (err) {
      log.error("Failed to get children", { error: err });
      return c.json({ error: "Failed to get children" }, 500);
    }
  });

  // ─── Create ──────────────────────────────────────────────

  app.post("/api/tasks", async (c) => {
    try {
      const body = await c.req.json() as CreateTaskOpts;
      if (!body.title?.trim()) {
        return c.json({ error: "title is required" }, 400);
      }

      const task = store().createTask(body);
      eventBus.emit("task:created", {
        taskId: task.taskId, title: task.title, projectId: task.projectId,
      });
      return c.json(task, 201);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error("Failed to create task", { error: err });
      if (msg.includes("NOT_FOUND") || msg.includes("MAX_DEPTH")) {
        return c.json({ error: msg }, 400);
      }
      return c.json({ error: "Failed to create task" }, 500);
    }
  });

  // ─── Update ──────────────────────────────────────────────

  app.patch("/api/tasks/:id", async (c) => {
    try {
      const id = c.req.param("id");
      const body = await c.req.json() as UpdateTaskOpts;
      const task = store().updateTask(id, body);
      eventBus.emit("task:updated", {
        taskId: task.taskId, changes: Object.keys(body), status: task.status,
      });
      return c.json(task);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error("Failed to update task", { error: err });
      if (msg.includes("NOT_FOUND")) return c.json({ error: "Task not found" }, 404);
      if (msg.includes("CYCLE")) return c.json({ error: msg }, 400);
      return c.json({ error: "Failed to update task" }, 500);
    }
  });

  // ─── Complete ────────────────────────────────────────────

  app.post("/api/tasks/:id/complete", (c) => {
    try {
      const id = c.req.param("id");
      const task = store().completeTask(id);
      eventBus.emit("task:completed", {
        taskId: task.taskId, title: task.title, projectId: task.projectId,
      });
      return c.json(task);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error("Failed to complete task", { error: err });
      if (msg.includes("NOT_FOUND")) return c.json({ error: "Task not found" }, 404);
      return c.json({ error: "Failed to complete task" }, 500);
    }
  });

  // ─── Delete ──────────────────────────────────────────────

  app.delete("/api/tasks/:id", (c) => {
    try {
      const id = c.req.param("id");
      const task = store().getTask(id) ?? store().getTaskByTaskId(id);
      if (!task) return c.json({ error: "Task not found" }, 404);

      store().deleteTask(task.id);
      eventBus.emit("task:deleted", {
        taskId: task.taskId, title: task.title, projectId: task.projectId,
      });
      return c.json({ success: true, deleted: task.taskId });
    } catch (err) {
      log.error("Failed to delete task", { error: err });
      return c.json({ error: "Failed to delete task" }, 500);
    }
  });

  // ─── Bulk: Create from Phase Plan ───────────────────────

  app.post("/api/tasks/from-phase", async (c) => {
    try {
      const body = await c.req.json() as {
        projectId: string;
        phaseId: string;
        tasks: Array<{
          title: string;
          description?: string;
          action?: string;
          verify?: string;
          files?: string[];
          mustHaves?: string[];
          priority?: "critical" | "high" | "medium" | "low";
          blockedBy?: string[];
        }>;
      };

      if (!body.projectId || !body.phaseId || !body.tasks?.length) {
        return c.json({ error: "projectId, phaseId, and tasks[] are required" }, 400);
      }

      const tasks = store().createFromPhasePlan(body.projectId, body.phaseId, body.tasks);
      eventBus.emit("task:bulk-created", {
        projectId: body.projectId, phaseId: body.phaseId, count: tasks.length,
      });
      return c.json({ tasks, count: tasks.length }, 201);
    } catch (err) {
      log.error("Failed to create tasks from phase", { error: err });
      return c.json({ error: "Failed to create tasks from phase" }, 500);
    }
  });

  // ─── Task Counts ────────────────────────────────────────

  app.get("/api/tasks/counts", (c) => {
    try {
      const projectId = c.req.query("projectId");
      const counts = store().getTaskCounts(projectId ?? undefined);
      const total = Object.values(counts).reduce((a, b) => a + b, 0);
      return c.json({ counts, total });
    } catch (err) {
      log.error("Failed to get task counts", { error: err });
      return c.json({ error: "Failed to get task counts" }, 500);
    }
  });

  // ─── Markdown Export ────────────────────────────────────

  app.get("/api/tasks/markdown", (c) => {
    try {
      const projectId = c.req.query("projectId");
      const md = store().toMarkdown(projectId ?? undefined);
      return c.text(md);
    } catch (err) {
      log.error("Failed to generate markdown", { error: err });
      return c.json({ error: "Failed to generate markdown" }, 500);
    }
  });

  return app;
}
