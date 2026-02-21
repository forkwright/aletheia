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
    const all = await fetchSessions(nousId);
    // Filter out background/system sessions
    sessions = all.filter((s) =>
      !s.sessionKey.startsWith("cron:") &&
      !s.sessionKey.startsWith("agent:") &&
      !s.sessionKey.startsWith("prosoche"),
    );
    // Auto-select: prefer the Signal session for continuity (shared with phone), then most recent
    if (sessions.length > 0) {
      const current = sessions.find((s) => s.id === activeSessionId);
      if (!current) {
        const signal = sessions.find((s) => s.sessionKey.startsWith("signal:") && s.nousId === nousId);
        activeSessionId = signal?.id ?? sessions[0]!.id;
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

export function refreshSessions(nousId: string): void {
  loadSessions(nousId);
}
