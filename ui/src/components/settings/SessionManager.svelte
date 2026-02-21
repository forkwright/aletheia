<script lang="ts">
  import { onMount } from "svelte";
  import { getAccessToken } from "../../lib/auth";
  import { formatTimeSince } from "../../lib/format";

  interface AuthSession {
    sessionId: string;
    createdAt: string;
    lastUsedAt: string;
    expiresAt: string;
    ip?: string;
    userAgent?: string;
  }

  let sessions = $state<AuthSession[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function fetchSessions() {
    loading = true;
    error = null;
    try {
      const token = getAccessToken();
      const res = await fetch("/api/auth/sessions", {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        credentials: "include",
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      sessions = data.sessions ?? [];
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to load sessions";
    } finally {
      loading = false;
    }
  }

  async function revokeSession(sessionId: string) {
    const token = getAccessToken();
    try {
      await fetch(`/api/auth/revoke/${sessionId}`, {
        method: "POST",
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        credentials: "include",
      });
      sessions = sessions.filter((s) => s.sessionId !== sessionId);
    } catch {
      error = "Failed to revoke session";
    }
  }

  async function revokeAllOther() {
    const currentSessions = [...sessions];
    for (const s of currentSessions.slice(1)) {
      await revokeSession(s.sessionId);
    }
  }

  function parseUA(ua?: string): string {
    if (!ua) return "Unknown device";
    if (ua.includes("Firefox")) return "Firefox";
    if (ua.includes("Chrome")) return "Chrome";
    if (ua.includes("Safari")) return "Safari";
    if (ua.includes("curl")) return "curl";
    return ua.slice(0, 30);
  }

  onMount(fetchSessions);
</script>

<div class="session-manager">
  {#if loading}
    <div class="session-loading">Loading sessions...</div>
  {:else if error}
    <div class="session-error">{error}</div>
  {:else if sessions.length === 0}
    <div class="session-empty">No active sessions</div>
  {:else}
    <div class="session-list">
      {#each sessions as session, i (session.sessionId)}
        <div class="session-row">
          <div class="session-info">
            <span class="session-device">{parseUA(session.userAgent)}</span>
            {#if session.ip}
              <span class="session-ip">{session.ip}</span>
            {/if}
            <span class="session-time">Last used {formatTimeSince(session.lastUsedAt)}</span>
            {#if i === 0}
              <span class="session-current">current</span>
            {/if}
          </div>
          {#if i > 0}
            <button class="revoke-btn" onclick={() => revokeSession(session.sessionId)}>
              Revoke
            </button>
          {/if}
        </div>
      {/each}
    </div>
    {#if sessions.length > 1}
      <button class="revoke-all-btn" onclick={revokeAllOther}>
        Revoke all other sessions
      </button>
    {/if}
  {/if}
</div>

<style>
  .session-manager {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .session-loading, .session-error, .session-empty {
    font-size: 12px;
    color: var(--text-muted);
    padding: 4px 0;
  }
  .session-error {
    color: var(--red);
  }
  .session-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .session-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
    font-size: 12px;
  }
  .session-row + .session-row {
    border-top: 1px solid rgba(48, 54, 61, 0.4);
  }
  .session-info {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
  }
  .session-device {
    color: var(--text);
    font-weight: 500;
  }
  .session-ip {
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .session-time {
    color: var(--text-muted);
  }
  .session-current {
    background: var(--accent);
    color: #fff;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 8px;
    font-weight: 500;
  }
  .revoke-btn {
    background: none;
    border: 1px solid var(--red);
    color: var(--red);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    font-size: 11px;
  }
  .revoke-btn:hover {
    background: rgba(248, 81, 73, 0.1);
  }
  .revoke-all-btn {
    background: none;
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    margin-top: 4px;
  }
  .revoke-all-btn:hover {
    color: var(--red);
    border-color: var(--red);
  }
</style>
