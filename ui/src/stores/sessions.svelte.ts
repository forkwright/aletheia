import { fetchSessions } from "../lib/api";
import type { Session } from "../lib/types";

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);
let loading = $state(false);
let loadGeneration = 0;

export function getSessions(): Session[] {
  return sessions;
}

export function getActiveSession(): Session | null {
  return sessions.find((s) => s.id === activeSessionId) ?? null;
}

export function getActiveSessionId(): string | null {
  return activeSessionId;
}

export function isSessionsLoading(): boolean {
  return loading;
}

/**
 * Full session load — clears active selection and re-selects.
 * Used on agent switch and initial load.
 */
export async function loadSessions(nousId: string): Promise<void> {
  const gen = ++loadGeneration;
  activeSessionId = null; // Clear immediately to prevent stale session reads during fetch
  loading = true;
  try {
    const fetched = await fetchSessions(nousId);
    if (gen !== loadGeneration) return; // Stale — a newer loadSessions call superseded us
    sessions = filterUserSessions(fetched);
    autoSelect(nousId);
  } finally {
    if (gen === loadGeneration) loading = false;
  }
}

/**
 * Refresh session list without disrupting the active selection.
 * Used after turn completion to pick up new sessions without switching.
 * Preserves activeSessionId if it still exists in the refreshed list.
 */
export async function refreshSessions(nousId: string): Promise<void> {
  const gen = ++loadGeneration;
  const previousActiveId = activeSessionId; // Capture before async gap
  loading = true;
  try {
    const fetched = await fetchSessions(nousId);
    if (gen !== loadGeneration) return; // Stale — a newer call superseded us
    sessions = filterUserSessions(fetched);
    // Keep current selection if it's still in the list
    if (previousActiveId && sessions.some((s) => s.id === previousActiveId)) {
      activeSessionId = previousActiveId;
    } else {
      // Current session gone (archived/deleted) — re-select
      autoSelect(nousId);
    }
  } finally {
    if (gen === loadGeneration) loading = false;
  }
}

export function setActiveSession(id: string): void {
  activeSessionId = id;
}

export function getActiveSessionKey(): string {
  const session = getActiveSession();
  if (!session) return `web:${Date.now()}`;
  // Signal session keys are valid — the webchat should share the same conversation
  // thread as Signal for continuity. The server-side guard prevents cross-agent leakage.
  return session.sessionKey;
}

export function createNewSession(_nousId: string): string {
  const key = `web:${Date.now()}`;
  activeSessionId = null;
  return key;
}

// --- Internal helpers ---

function filterUserSessions(all: Session[]): Session[] {
  return all.filter((s) =>
    !s.sessionKey.startsWith("cron:") &&
    !s.sessionKey.startsWith("agent:") &&
    !s.sessionKey.startsWith("prosoche"),
  );
}

function autoSelect(nousId: string): void {
  if (sessions.length > 0) {
    const signal = sessions.find((s) => s.sessionKey.startsWith("signal:") && s.nousId === nousId);
    activeSessionId = signal?.id ?? sessions[0]!.id;
  } else {
    activeSessionId = null;
  }
}
