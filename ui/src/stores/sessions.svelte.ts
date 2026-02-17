import { fetchSessions } from "../lib/api";
import type { Session } from "../lib/types";

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);
let loading = $state(false);

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

export async function loadSessions(nousId: string): Promise<void> {
  loading = true;
  try {
    sessions = await fetchSessions(nousId);
    // Auto-select first session if none selected or current doesn't belong to this agent
    if (sessions.length > 0) {
      const current = sessions.find((s) => s.id === activeSessionId);
      if (!current) {
        activeSessionId = sessions[0]!.id;
      }
    } else {
      activeSessionId = null;
    }
  } finally {
    loading = false;
  }
}

export function setActiveSession(id: string): void {
  activeSessionId = id;
}

export function getActiveSessionKey(): string {
  const session = getActiveSession();
  return session?.sessionKey ?? `web:${Date.now()}`;
}

export function createNewSession(_nousId: string): string {
  const key = `web:${Date.now()}`;
  activeSessionId = null;
  return key;
}

export function refreshSessions(nousId: string): void {
  loadSessions(nousId);
}
