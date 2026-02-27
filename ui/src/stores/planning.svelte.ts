// Planning data store — reactive state for Dianoia planning data
// Handles fetch, mutation, optimistic updates, and SSE-driven refresh.
import { onGlobalEvent } from "../lib/events.svelte";
import { authFetch } from "../components/planning/api";

// ─── Types ───────────────────────────────────────────────────

export interface Requirement {
  id: string;
  reqId: string;
  description: string;
  category: string;
  tier: "v1" | "v2" | "out-of-scope";
  rationale: string | null;
  status: "pending" | "validated" | "skipped";
  createdAt: string;
  updatedAt: string;
}

export interface Phase {
  id: string;
  name: string;
  goal: string;
  status: "pending" | "executing" | "complete" | "failed" | "skipped";
  phaseOrder: number;
  requirements: string[];
  successCriteria: string[];
  dependencies: string[];
  verificationResult: unknown | null;
  createdAt: string;
  updatedAt: string;
}

export interface DiscussionQuestion {
  id: string;
  question: string;
  options: Array<{ label: string; rationale: string }>;
  recommendation: string | null;
  decision: string | null;
  userNote: string | null;
  status: "pending" | "answered" | "skipped";
  createdAt: string;
  updatedAt: string;
}

export interface Project {
  id: string;
  nousId: string;
  goal: string;
  state: string;
  config: Record<string, unknown>;
  projectContext: unknown | null;
  createdAt: string;
  updatedAt: string;
}

// ─── Reactive State ──────────────────────────────────────────

let currentProjectId = $state<string | null>(null);
let project = $state<Project | null>(null);
let requirements = $state<Requirement[]>([]);
let phases = $state<Phase[]>([]);
let discussions = $state<DiscussionQuestion[]>([]);
let loading = $state(false);
let error = $state<string | null>(null);

// Debounce SSE refresh — coalesce rapid event bursts
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
const REFRESH_DEBOUNCE_MS = 300;

// ─── Public Accessors (readonly reactive) ────────────────────

export function getProject(): Project | null { return project; }
export function getRequirements(): Requirement[] { return requirements; }
export function getPhases(): Phase[] { return phases; }
export function getDiscussions(): DiscussionQuestion[] { return discussions; }
export function isLoading(): boolean { return loading; }
export function getError(): string | null { return error; }
export function getCurrentProjectId(): string | null { return currentProjectId; }

// ─── Load / Refresh ──────────────────────────────────────────

export async function loadProject(projectId: string): Promise<void> {
  currentProjectId = projectId;
  loading = true;
  error = null;

  try {
    const [projRes, reqsRes, phasesRes] = await Promise.all([
      authFetch(`/api/planning/projects/${projectId}`),
      authFetch(`/api/planning/projects/${projectId}/requirements`),
      authFetch(`/api/planning/projects/${projectId}/phases`),
    ]);

    if (!projRes.ok) throw new Error(`Failed to load project: ${projRes.status}`);
    project = await projRes.json() as Project;

    if (reqsRes.ok) {
      const data = await reqsRes.json() as { requirements: Requirement[] };
      requirements = data.requirements ?? [];
    }

    if (phasesRes.ok) {
      const data = await phasesRes.json() as { phases: Phase[] };
      phases = [...(data.phases ?? [])].sort((a: Phase, b: Phase) => a.phaseOrder - b.phaseOrder);
    }
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
  } finally {
    loading = false;
  }
}

export async function loadDiscussions(projectId: string, phaseId: string): Promise<void> {
  try {
    const res = await authFetch(`/api/planning/projects/${projectId}/discuss?phaseId=${encodeURIComponent(phaseId)}`);
    if (res.ok) {
      const data = await res.json() as { questions: DiscussionQuestion[] };
      discussions = data.questions ?? [];
    }
  } catch (err) {
    console.warn("[planning store] Failed to load discussions:", err);
  }
}

/** Refresh all data for the current project (debounced for SSE bursts) */
function scheduleRefresh(): void {
  if (!currentProjectId) return;
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    if (currentProjectId) loadProject(currentProjectId);
  }, REFRESH_DEBOUNCE_MS);
}

export function clearProject(): void {
  currentProjectId = null;
  project = null;
  requirements = [];
  phases = [];
  discussions = [];
  error = null;
}

// ─── Mutations (optimistic) ──────────────────────────────────

export async function updateRequirement(
  reqIdentifier: string,
  updates: Partial<Pick<Requirement, "tier" | "rationale" | "description" | "category" | "reqId" | "status">>,
): Promise<Requirement | null> {
  if (!currentProjectId) return null;

  // Optimistic: apply locally
  const idx = requirements.findIndex(r => r.id === reqIdentifier || r.reqId === reqIdentifier);
  const prev = idx >= 0 ? { ...requirements[idx]! } : null;
  if (idx >= 0) {
    requirements = requirements.map((r, i) =>
      i === idx ? { ...r, ...updates } as Requirement : r
    );
  }

  try {
    const headers: Record<string, string> = {};
    // SYNC-05: Send If-Unmodified-Since for conflict detection
    if (prev?.updatedAt) {
      headers["If-Unmodified-Since"] = prev.updatedAt;
    }
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/requirements/${encodeURIComponent(reqIdentifier)}`, {
      method: "PATCH",
      headers,
      body: JSON.stringify(updates),
    });
    if (res.status === 409) {
      // Conflict: server has newer version — reload from server
      if (prev && idx >= 0) {
        requirements = requirements.map((r, i) => i === idx ? prev : r);
      }
      error = "Conflict: this item was modified by another session. Refreshing...";
      scheduleRefresh();
      return null;
    }
    if (!res.ok) throw new Error(`PATCH failed: ${res.status}`);
    const updated = await res.json() as Requirement;
    // Replace with server response (authoritative)
    requirements = requirements.map(r => r.id === updated.id ? updated : r);
    return updated;
  } catch (err) {
    // Rollback
    if (prev && idx >= 0) {
      requirements = requirements.map((r, i) => i === idx ? prev : r);
    }
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function createRequirement(
  data: { description: string; category: string; tier?: Requirement["tier"]; rationale?: string; reqId?: string },
): Promise<Requirement | null> {
  if (!currentProjectId) return null;

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/requirements`, {
      method: "POST",
      body: JSON.stringify(data),
    });
    if (!res.ok) throw new Error(`POST failed: ${res.status}`);
    const created = await res.json() as Requirement;
    requirements = [...requirements, created];
    return created;
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function deleteRequirement(reqIdentifier: string): Promise<boolean> {
  if (!currentProjectId) return false;

  const idx = requirements.findIndex(r => r.id === reqIdentifier || r.reqId === reqIdentifier);
  const prev = idx >= 0 ? requirements[idx]! : null;
  if (idx >= 0) {
    requirements = requirements.filter((_, i) => i !== idx);
  }

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/requirements/${encodeURIComponent(reqIdentifier)}`, {
      method: "DELETE",
    });
    if (!res.ok) throw new Error(`DELETE failed: ${res.status}`);
    return true;
  } catch (err) {
    // Rollback
    if (prev) {
      requirements = [...requirements.slice(0, idx), prev, ...requirements.slice(idx)];
    }
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

export async function updatePhase(
  phaseId: string,
  updates: Partial<Pick<Phase, "name" | "goal" | "successCriteria" | "requirements">>,
): Promise<Phase | null> {
  if (!currentProjectId) return null;

  const idx = phases.findIndex(p => p.id === phaseId);
  const prev = idx >= 0 ? { ...phases[idx]! } : null;
  if (idx >= 0) {
    phases = phases.map((p, i) => i === idx ? { ...p, ...updates } as Phase : p);
  }

  try {
    const headers: Record<string, string> = {};
    // SYNC-05: Conflict detection
    if (prev?.updatedAt) {
      headers["If-Unmodified-Since"] = prev.updatedAt;
    }
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/phases/${encodeURIComponent(phaseId)}`, {
      method: "PATCH",
      headers,
      body: JSON.stringify(updates),
    });
    if (res.status === 409) {
      if (prev && idx >= 0) {
        phases = phases.map((p, i) => i === idx ? prev : p);
      }
      error = "Conflict: this phase was modified by another session. Refreshing...";
      scheduleRefresh();
      return null;
    }
    if (!res.ok) throw new Error(`PATCH failed: ${res.status}`);
    const updated = await res.json() as Phase;
    phases = phases.map(p => p.id === updated.id ? updated : p);
    return updated;
  } catch (err) {
    if (prev && idx >= 0) {
      phases = phases.map((p, i) => i === idx ? prev : p);
    }
    error = err instanceof Error ? err.message : String(err);
    return null;
  }
}

export async function deletePhase(phaseId: string): Promise<boolean> {
  if (!currentProjectId) return false;

  const idx = phases.findIndex(p => p.id === phaseId);
  const prev = idx >= 0 ? phases[idx]! : null;
  if (idx >= 0) {
    phases = phases.filter((_, i) => i !== idx);
  }

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/phases/${encodeURIComponent(phaseId)}`, {
      method: "DELETE",
    });
    if (!res.ok) throw new Error(`DELETE failed: ${res.status}`);
    return true;
  } catch (err) {
    if (prev) {
      phases = [...phases.slice(0, idx), prev, ...phases.slice(idx)];
    }
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

export async function reorderPhase(phaseId: string, newOrder: number): Promise<boolean> {
  if (!currentProjectId) return false;

  const prevPhases = [...phases];

  // Optimistic: simulate reorder locally
  const idx = phases.findIndex(p => p.id === phaseId);
  if (idx >= 0) {
    const moved = phases[idx]!;
    const without = phases.filter((_, i) => i !== idx);
    without.splice(newOrder, 0, moved);
    phases = without.map((p, i) => ({ ...p, phaseOrder: i }));
  }

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/phases/${encodeURIComponent(phaseId)}/reorder`, {
      method: "POST",
      body: JSON.stringify({ newOrder }),
    });
    if (!res.ok) throw new Error(`Reorder failed: ${res.status}`);
    // Refresh from server to get authoritative order
    const data = await res.json() as { phases: Array<{ id: string; name: string; phaseOrder: number; status: string }> };
    // Merge server order into existing phase data
    const orderMap = new Map(data.phases.map(p => [p.id, p.phaseOrder]));
    phases = [...phases.map(p => ({ ...p, phaseOrder: orderMap.get(p.id) ?? p.phaseOrder }))]
      .sort((a: Phase, b: Phase) => a.phaseOrder - b.phaseOrder);
    return true;
  } catch (err) {
    phases = prevPhases;
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

export async function answerDiscussion(
  questionId: string,
  decision: string,
  userNote?: string,
): Promise<boolean> {
  if (!currentProjectId) return false;

  // Optimistic
  discussions = discussions.map(q =>
    q.id === questionId ? { ...q, decision, userNote: userNote ?? null, status: "answered" as const } : q
  );

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/discuss/answer`, {
      method: "POST",
      body: JSON.stringify({ questionId, decision, userNote }),
    });
    if (!res.ok) throw new Error(`Answer failed: ${res.status}`);
    return true;
  } catch (err) {
    // Rollback: reload from server
    if (currentProjectId) {
      const activePhaseId = discussions[0]?.id ? phases.find(p => p.status === "pending" || p.status === "executing")?.id : undefined;
      if (activePhaseId) loadDiscussions(currentProjectId, activePhaseId);
    }
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

export async function skipDiscussion(questionId: string): Promise<boolean> {
  if (!currentProjectId) return false;

  discussions = discussions.map(q =>
    q.id === questionId ? { ...q, status: "skipped" as const } : q
  );

  try {
    const res = await authFetch(`/api/planning/projects/${currentProjectId}/discuss/skip`, {
      method: "POST",
      body: JSON.stringify({ questionId }),
    });
    if (!res.ok) throw new Error(`Skip failed: ${res.status}`);
    return true;
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
    return false;
  }
}

// ─── SSE Subscription ────────────────────────────────────────

let sseUnsub: (() => void) | null = null;

export function subscribePlanningEvents(): () => void {
  if (sseUnsub) sseUnsub();

  sseUnsub = onGlobalEvent((event, _data) => {
    if (!currentProjectId) return;

    // Any planning event triggers a debounced refresh
    if (event.startsWith("planning:")) {
      scheduleRefresh();
    }
  });

  return () => {
    if (sseUnsub) { sseUnsub(); sseUnsub = null; }
    if (refreshTimer) { clearTimeout(refreshTimer); refreshTimer = null; }
  };
}
