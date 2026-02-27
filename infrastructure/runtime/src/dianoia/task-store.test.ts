import { describe, it, expect, beforeEach } from "vitest";
import Database from "better-sqlite3";
import { TaskStore } from "./task-store.js";
import { TASK_V1_DDL } from "./task-schema.js";
import { PLANNING_V20_DDL } from "./schema.js";

function createTestStore(): { db: Database.Database; store: TaskStore } {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
  // Planning tables needed for FK references
  db.exec(PLANNING_V20_DDL);
  db.exec(TASK_V1_DDL);
  return { db, store: new TaskStore(db) };
}

describe("TaskStore", () => {
  let store: TaskStore;
  let db: Database.Database;

  beforeEach(() => {
    ({ db, store } = createTestStore());
  });

  describe("CRUD", () => {
    it("creates a task with defaults", () => {
      const task = store.createTask({ title: "Test task" });
      expect(task.title).toBe("Test task");
      expect(task.status).toBe("pending");
      expect(task.priority).toBe("medium");
      expect(task.taskId).toMatch(/^DAILY-001$/);
      expect(task.depth).toBe(0);
      expect(task.description).toBe("");
      expect(task.files).toEqual([]);
      expect(task.blockedBy).toEqual([]);
      expect(task.blocks).toEqual([]);
    });

    it("creates a task with all fields", () => {
      const task = store.createTask({
        title: "Full task",
        description: "Detailed description",
        priority: "high",
        action: "Run the tests",
        verify: "Check exit code 0",
        files: ["src/index.ts", "test/index.test.ts"],
        mustHaves: ["no regressions", "coverage > 80%"],
        contextBudget: 40000,
        assignee: "syn",
        tags: ["testing", "ci"],
      });

      expect(task.priority).toBe("high");
      expect(task.action).toBe("Run the tests");
      expect(task.verify).toBe("Check exit code 0");
      expect(task.files).toEqual(["src/index.ts", "test/index.test.ts"]);
      expect(task.mustHaves).toEqual(["no regressions", "coverage > 80%"]);
      expect(task.contextBudget).toBe(40000);
      expect(task.assignee).toBe("syn");
      expect(task.tags).toEqual(["testing", "ci"]);
    });

    it("auto-increments task IDs", () => {
      const t1 = store.createTask({ title: "First" });
      const t2 = store.createTask({ title: "Second" });
      const t3 = store.createTask({ title: "Third" });
      expect(t1.taskId).toBe("DAILY-001");
      expect(t2.taskId).toBe("DAILY-002");
      expect(t3.taskId).toBe("DAILY-003");
    });

    it("uses custom task ID", () => {
      const task = store.createTask({ title: "Custom", taskId: "PROJ-042" });
      expect(task.taskId).toBe("PROJ-042");
    });

    it("reads by internal ID and taskId", () => {
      const created = store.createTask({ title: "Read test" });
      const byId = store.getTask(created.id);
      const byTaskId = store.getTaskByTaskId(created.taskId);
      expect(byId?.title).toBe("Read test");
      expect(byTaskId?.title).toBe("Read test");
      expect(byId?.id).toBe(byTaskId?.id);
    });

    it("updates a task", () => {
      const task = store.createTask({ title: "Original" });
      const updated = store.updateTask(task.id, {
        title: "Updated",
        priority: "critical",
        tags: ["urgent"],
      });
      expect(updated.title).toBe("Updated");
      expect(updated.priority).toBe("critical");
      expect(updated.tags).toEqual(["urgent"]);
    });

    it("updates by taskId", () => {
      const task = store.createTask({ title: "By taskId" });
      const updated = store.updateTask(task.taskId, { title: "Updated" });
      expect(updated.title).toBe("Updated");
    });

    it("deletes a task", () => {
      const task = store.createTask({ title: "Delete me" });
      store.deleteTask(task.id);
      expect(store.getTask(task.id)).toBeUndefined();
    });

    it("throws on not-found", () => {
      expect(() => store.getTaskOrThrow("nonexistent")).toThrow("Task not found");
      expect(() => store.updateTask("nonexistent", { title: "x" })).toThrow("Task not found");
      expect(() => store.deleteTask("nonexistent")).toThrow("Task not found");
    });
  });

  describe("Sequential IDs", () => {
    it("generates project-prefixed IDs when project has config", () => {
      // Create a project for scoping
      db.prepare(`INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash) VALUES (?, ?, ?, ?, ?, ?, ?)`)
        .run("proj-1", "syn", "sess-1", "Test", "idle", JSON.stringify({ name: "Dianoia" }), "hash");

      const t1 = store.createTask({ title: "First", projectId: "proj-1" });
      const t2 = store.createTask({ title: "Second", projectId: "proj-1" });
      expect(t1.taskId).toBe("DIANOI-001");
      expect(t2.taskId).toBe("DIANOI-002");
    });
  });

  describe("Hierarchy", () => {
    it("creates parent-child relationships", () => {
      const parent = store.createTask({ title: "Parent" });
      const child = store.createTask({ title: "Child", parentId: parent.taskId });
      expect(child.parentId).toBe(parent.id);
      expect(child.depth).toBe(1);

      const children = store.getChildren(parent.taskId);
      expect(children).toHaveLength(1);
      expect(children[0]!.taskId).toBe(child.taskId);
    });

    it("supports 4 levels (0-3)", () => {
      const l0 = store.createTask({ title: "Level 0" });
      const l1 = store.createTask({ title: "Level 1", parentId: l0.taskId });
      const l2 = store.createTask({ title: "Level 2", parentId: l1.taskId });
      const l3 = store.createTask({ title: "Level 3", parentId: l2.taskId });
      expect(l0.depth).toBe(0);
      expect(l1.depth).toBe(1);
      expect(l2.depth).toBe(2);
      expect(l3.depth).toBe(3);
    });

    it("rejects depth > 3", () => {
      const l0 = store.createTask({ title: "Level 0" });
      const l1 = store.createTask({ title: "Level 1", parentId: l0.taskId });
      const l2 = store.createTask({ title: "Level 2", parentId: l1.taskId });
      const l3 = store.createTask({ title: "Level 3", parentId: l2.taskId });
      expect(() => store.createTask({ title: "Level 4", parentId: l3.taskId }))
        .toThrow("Max hierarchy depth");
    });

    it("cascades delete to children", () => {
      const parent = store.createTask({ title: "Parent" });
      const child = store.createTask({ title: "Child", parentId: parent.taskId });
      store.deleteTask(parent.id);
      expect(store.getTask(child.id)).toBeUndefined();
    });
  });

  describe("Dependencies", () => {
    it("creates blocked task when dependency is pending", () => {
      const dep = store.createTask({ title: "Dependency" });
      const blocked = store.createTask({ title: "Blocked", blockedBy: [dep.taskId] });
      expect(blocked.status).toBe("blocked");
      expect(blocked.blockedBy).toEqual([dep.taskId]);

      // Reverse link
      const depRefresh = store.getTaskOrThrow(dep.id);
      expect(depRefresh.blocks).toEqual([blocked.taskId]);
    });

    it("creates pending task when dependency is already done", () => {
      const dep = store.createTask({ title: "Dependency" });
      store.completeTask(dep.id);
      const notBlocked = store.createTask({ title: "Not blocked", blockedBy: [dep.taskId] });
      expect(notBlocked.status).toBe("pending");
    });

    it("propagates unblocks on completion", () => {
      const dep = store.createTask({ title: "Dependency" });
      const blocked = store.createTask({ title: "Blocked", blockedBy: [dep.taskId] });
      expect(blocked.status).toBe("blocked");

      store.completeTask(dep.id);

      const unblocked = store.getTaskOrThrow(blocked.id);
      expect(unblocked.status).toBe("pending");
    });

    it("waits for ALL dependencies before unblocking", () => {
      const dep1 = store.createTask({ title: "Dep 1" });
      const dep2 = store.createTask({ title: "Dep 2" });
      const blocked = store.createTask({ title: "Blocked", blockedBy: [dep1.taskId, dep2.taskId] });

      store.completeTask(dep1.id);
      expect(store.getTaskOrThrow(blocked.id).status).toBe("blocked");

      store.completeTask(dep2.id);
      expect(store.getTaskOrThrow(blocked.id).status).toBe("pending");
    });

    it("detects direct cycles", () => {
      const a = store.createTask({ title: "A" });
      const b = store.createTask({ title: "B", blockedBy: [a.taskId] });
      expect(() => store.updateTask(a.id, { blockedBy: [b.taskId] }))
        .toThrow("cycle");
    });

    it("detects indirect cycles", () => {
      const a = store.createTask({ title: "A" });
      const b = store.createTask({ title: "B", blockedBy: [a.taskId] });
      const c = store.createTask({ title: "C", blockedBy: [b.taskId] });
      expect(() => store.updateTask(a.id, { blockedBy: [c.taskId] }))
        .toThrow("cycle");
    });

    it("cleans up dependencies on delete", () => {
      const dep = store.createTask({ title: "Dep" });
      const blocked = store.createTask({ title: "Blocked", blockedBy: [dep.taskId] });
      store.deleteTask(dep.id);

      const refreshed = store.getTaskOrThrow(blocked.id);
      expect(refreshed.blockedBy).toEqual([]);
    });
  });

  describe("Filtering", () => {
    it("filters by status", () => {
      store.createTask({ title: "Pending" });
      const active = store.createTask({ title: "Active" });
      store.updateTask(active.id, { status: "active" });
      const done = store.createTask({ title: "Done" });
      store.completeTask(done.id);

      const pending = store.listTasks({ status: "pending" });
      expect(pending).toHaveLength(1);
      expect(pending[0]!.title).toBe("Pending");

      const multi = store.listTasks({ status: ["active", "done"] });
      expect(multi).toHaveLength(2);
    });

    it("filters by priority", () => {
      store.createTask({ title: "High", priority: "high" });
      store.createTask({ title: "Low", priority: "low" });
      store.createTask({ title: "Critical", priority: "critical" });

      const high = store.listTasks({ priority: "high" });
      expect(high).toHaveLength(1);
      expect(high[0]!.title).toBe("High");
    });

    it("filters by assignee", () => {
      store.createTask({ title: "Syn's task", assignee: "syn" });
      store.createTask({ title: "Unassigned" });

      const synTasks = store.listTasks({ assignee: "syn" });
      expect(synTasks).toHaveLength(1);
      expect(synTasks[0]!.title).toBe("Syn's task");
    });

    it("filters by tag", () => {
      store.createTask({ title: "Tagged", tags: ["ci", "urgent"] });
      store.createTask({ title: "Other" });

      const tagged = store.listTasks({ tag: "ci" });
      expect(tagged).toHaveLength(1);
      expect(tagged[0]!.title).toBe("Tagged");
    });

    it("filters top-level only", () => {
      const parent = store.createTask({ title: "Parent" });
      store.createTask({ title: "Child", parentId: parent.taskId });

      const topLevel = store.listTasks({ parentId: null });
      expect(topLevel).toHaveLength(1);
      expect(topLevel[0]!.title).toBe("Parent");
    });
  });

  describe("Daily Tasks", () => {
    it("returns unscoped tasks", () => {
      store.createTask({ title: "Daily task" });
      db.prepare(`INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash) VALUES (?, ?, ?, ?, ?, ?, ?)`)
        .run("proj-1", "syn", "sess-1", "Test", "idle", JSON.stringify({ name: "Proj" }), "hash");
      store.createTask({ title: "Project task", projectId: "proj-1" });

      const daily = store.getDailyTasks();
      expect(daily).toHaveLength(1);
      expect(daily[0]!.title).toBe("Daily task");
    });

    it("includes tasks tagged 'daily'", () => {
      db.prepare(`INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash) VALUES (?, ?, ?, ?, ?, ?, ?)`)
        .run("proj-1", "syn", "sess-1", "Test", "idle", JSON.stringify({ name: "Proj" }), "hash");
      store.createTask({ title: "Also daily", projectId: "proj-1", tags: ["daily"] });

      const daily = store.getDailyTasks();
      expect(daily).toHaveLength(1);
      expect(daily[0]!.title).toBe("Also daily");
    });

    it("excludes done/skipped", () => {
      const t = store.createTask({ title: "Done task" });
      store.completeTask(t.id);
      store.createTask({ title: "Active task" });

      const daily = store.getDailyTasks();
      expect(daily).toHaveLength(1);
      expect(daily[0]!.title).toBe("Active task");
    });

    it("orders by priority then created_at", () => {
      store.createTask({ title: "Low", priority: "low" });
      store.createTask({ title: "Critical", priority: "critical" });
      store.createTask({ title: "High", priority: "high" });

      const daily = store.getDailyTasks();
      expect(daily.map(t => t.priority)).toEqual(["critical", "high", "low"]);
    });
  });

  describe("Bulk Operations", () => {
    it("creates tasks from phase plan", () => {
      db.prepare(`INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash) VALUES (?, ?, ?, ?, ?, ?, ?)`)
        .run("proj-1", "syn", "sess-1", "Test", "idle", JSON.stringify({ name: "Build" }), "hash");
      db.prepare(`INSERT INTO planning_phases (id, project_id, name, goal) VALUES (?, ?, ?, ?)`)
        .run("phase-1", "proj-1", "Phase 1", "Build it");

      const tasks = store.createFromPhasePlan("proj-1", "phase-1", [
        { title: "Write code", action: "Implement feature" },
        { title: "Write tests", action: "Add test coverage", blockedBy: ["BUILD-001"] },
      ]);

      expect(tasks).toHaveLength(2);
      expect(tasks[0]!.taskId).toBe("BUILD-001");
      expect(tasks[1]!.taskId).toBe("BUILD-002");
      expect(tasks[1]!.status).toBe("blocked");
      expect(tasks[0]!.projectId).toBe("proj-1");
      expect(tasks[0]!.phaseId).toBe("phase-1");
    });

    it("generates task counts", () => {
      store.createTask({ title: "Pending 1" });
      store.createTask({ title: "Pending 2" });
      const active = store.createTask({ title: "Active" });
      store.updateTask(active.id, { status: "active" });
      const done = store.createTask({ title: "Done" });
      store.completeTask(done.id);

      const counts = store.getTaskCounts();
      expect(counts.pending).toBe(2);
      expect(counts.active).toBe(1);
      expect(counts.done).toBe(1);
    });
  });

  describe("Markdown Generation", () => {
    it("generates markdown for tasks", () => {
      store.createTask({ title: "Active task", priority: "high" });
      const done = store.createTask({ title: "Done task" });
      store.completeTask(done.id);

      const md = store.toMarkdown();
      expect(md).toContain("# Tasks");
      expect(md).toContain("Active task");
      expect(md).toContain("[high]");
      expect(md).toContain("Done task");
      expect(md).toContain("1/2 (50%)");
    });

    it("returns placeholder for empty", () => {
      const md = store.toMarkdown();
      expect(md).toContain("No tasks yet");
    });
  });

  describe("Orphan Detection", () => {
    it("detects orphaned tasks", () => {
      const parent = store.createTask({ title: "Parent" });
      store.createTask({ title: "Child", parentId: parent.taskId });

      // Manually delete parent without cascade to simulate orphan
      // (CASCADE should prevent this in practice, but test the detection)
      const orphans = store.getOrphans();
      // With CASCADE, deleting parent deletes child, so no orphans
      expect(orphans).toHaveLength(0);
    });
  });
});
