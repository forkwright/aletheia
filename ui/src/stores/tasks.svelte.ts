/**
 * Reactive task store — fetches from /api/tasks, supports optimistic CRUD.
 * SSE events trigger debounced refresh (reuses planning pattern).
 */
import { authFetch } from "../components/planning/api";
import { onGlobalEvent } from "../lib/events.svelte";

// ─── Types ───────────────────────────────────────────────────

export interface Task {
  id: string;
  projectId: string | null;
  phaseId: string | null;
  parentId: string | null;
  taskId: string;
  title: string;
  description: string;
  status: "pending" | "active" | "done" | "failed" | "skipped" | "blocked";
  priority: "critical" | "high" | "medium" | "low";
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

export interface TaskCounts {
  pending: number;
  active: number;
  done: number;
  failed: number;
  skipped: number;
  blocked: number;
}

// ─── State ───────────────────────────────────────────────────

let tasks = $state<Task[]>([]);
let dailyTasks = $state<Task[]>([]);
let counts = $state<TaskCounts>({ pending: 0, active: 0, done: 0, failed: 0, skipped: 0, blocked: 0 });
let loading = $state(false);
let error = $state<string | null>(null);
let currentProjectId = $state<string | null>(null);

// Debounce SSE refresh
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleRefresh() {
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => {
    if (currentProjectId) loadTasks(currentProjectId);
    loadDailyTasks();
  }, 300);
}

// ─── Getters ─────────────────────────────────────────────────

export function getTasks(): Task[] { return tasks; }
export function getDailyTasks(): Task[] { return dailyTasks; }
export function getTaskCounts(): TaskCounts { return counts; }
export function isTasksLoading(): boolean { return loading; }
export function getTasksError(): string | null { return error; }

// ─── Loaders ─────────────────────────────────────────────────

export async function loadTasks(projectId?: string): Promise<void> {
  try {
    loading = true;
    error = null;
    currentProjectId = projectId ?? null;

    const url = projectId ? `/api/tasks?projectId=${encodeURIComponent(projectId)}` : "/api/tasks";
    const [tasksRes, countsRes] = await Promise.all([
      authFetch(url),
      authFetch(projectId ? `/api/tasks/counts?projectId=${encodeURIComponent(projectId)}` : "/api/tasks/counts"),
    ]);

    if (tasksRes.ok) {
      const data = await tasksRes.json() as { tasks: Task[] };
      tasks = data.tasks ?? [];
    }
    if (countsRes.ok) {
      const data = await countsRes.json() as { counts: TaskCounts };
      counts = data.counts ?? counts;
    }
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
  } finally {
    loading = false;
  }
}

export async function loadDailyTasks(): Promise<void> {
  try {
    const res = await authFetch("/api/tasks/daily");
    if (res.ok) {
      const data = await res.json() as { tasks: Task[] };
      dailyTasks = data.tasks ?? [];
    }
  } catch {
    // Silently fail daily refresh
  }
}

// ─── Mutations ───────────────────────────────────────────────

export async function createTask(opts: {
  title: string;
  description?: string;
  priority?: Task["priority"];
  projectId?: string;
  phaseId?: string;
  parentId?: string;
  action?: string;
  verify?: string;
  tags?: string[];
  blockedBy?: string[];
}): Promise<Task | null> {
  try {
    const res = await authFetch("/api/tasks", {
      method: "POST",
      body: JSON.stringify(opts),
    });
    if (!res.ok) throw new Error(`Create failed: ${res.status}`);
    const task = await res.json() as Task;
    tasks = [...tasks, task];
    return task;
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function updateTask(
  id: string,
  updates: Partial<Pick<Task, "title" | "description" | "status" | "priority" | "action" | "verify" | "assignee" | "tags" | "blockedBy">>,
): Promise<Task | null> {
  // Optimistic
  const idx = tasks.findIndex(t => t.id === id || t.taskId === id);
  const prev = idx >= 0 ? { ...tasks[idx]! } : null;
  if (idx >= 0) {
    tasks = tasks.map((t, i) => i === idx ? { ...t, ...updates } as Task : t);
  }

  try {
    const res = await authFetch(`/api/tasks/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(updates),
    });
    if (!res.ok) throw new Error(`Update failed: ${res.status}`);
    const updated = await res.json() as Task;
    tasks = tasks.map(t => t.id === updated.id ? updated : t);
    return updated;
  } catch (err) {
    // Rollback
    if (prev && idx >= 0) {
      tasks = tasks.map((t, i) => i === idx ? prev : t);
    }
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function completeTask(id: string): Promise<Task | null> {
  // Optimistic
  const idx = tasks.findIndex(t => t.id === id || t.taskId === id);
  const prev = idx >= 0 ? { ...tasks[idx]! } : null;
  if (idx >= 0) {
    tasks = tasks.map((t, i) => i === idx ? { ...t, status: "done" as const, completedAt: new Date().toISOString() } : t);
  }

  try {
    const res = await authFetch(`/api/tasks/${encodeURIComponent(id)}/complete`, { method: "POST" });
    if (!res.ok) throw new Error(`Complete failed: ${res.status}`);
    const updated = await res.json() as Task;
    tasks = tasks.map(t => t.id === updated.id ? updated : t);
    return updated;
  } catch (err) {
    if (prev && idx >= 0) {
      tasks = tasks.map((t, i) => i === idx ? prev : t);
    }
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function deleteTask(id: string): Promise<boolean> {
  const prev = [...tasks];
  tasks = tasks.filter(t => t.id !== id && t.taskId !== id);

  try {
    const res = await authFetch(`/api/tasks/${encodeURIComponent(id)}`, { method: "DELETE" });
    if (!res.ok) throw new Error(`Delete failed: ${res.status}`);
    return true;
  } catch (err) {
    tasks = prev;
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

// ─── SSE Subscription ────────────────────────────────────────

let sseUnsub: (() => void) | null = null;

export function subscribeTaskEvents(): () => void {
  if (sseUnsub) sseUnsub();

  sseUnsub = onGlobalEvent((event, _data) => {
    if (event.startsWith("task:")) {
      scheduleRefresh();
    }
  });

  return () => {
    if (sseUnsub) { sseUnsub(); sseUnsub = null; }
    if (refreshTimer) { clearTimeout(refreshTimer); refreshTimer = null; }
  };
}
