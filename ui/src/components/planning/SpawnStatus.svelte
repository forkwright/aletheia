<script lang="ts">
  import { authFetch } from "./api";
  import { timeAgo } from "../../lib/utils";

  interface SpawnRecord {
    id: string;
    projectId: string;
    phaseId: string;
    agentSessionId: string;
    status: "pending" | "running" | "complete" | "failed" | "done" | "skipped" | "zombie";
    result: string | null;
    wave: number;
    waveNumber: number;
    startedAt: string | null;
    completedAt: string | null;
    errorMessage: string | null;
    createdAt: string;
    updatedAt: string;
  }

  interface SpawnSummary {
    total: number;
    running: number;
    complete: number;
    failed: number;
    pending: number;
  }

  let { projectId }: { projectId: string } = $props();

  let spawns = $state<SpawnRecord[]>([]);
  let summary = $state<SpawnSummary>({ total: 0, running: 0, complete: 0, failed: 0, pending: 0 });
  let error = $state<string | null>(null);
  let loading = $state(true);
  let expandedSpawn = $state<string | null>(null);
  let collapsed = $state(false);

  async function loadSpawns() {
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/spawns`);
      if (!res.ok) {
        error = `Failed to load spawns (${res.status})`;
        return;
      }
      const data = await res.json() as { spawns: SpawnRecord[]; summary: SpawnSummary };
      spawns = data.spawns ?? [];
      summary = data.summary ?? { total: 0, running: 0, complete: 0, failed: 0, pending: 0 };
      error = null;
    } catch (e) {
      error = `Failed to load spawns: ${e}`;
    } finally {
      loading = false;
    }
  }

  // Poll when there are running agents
  let pollInterval = $state<ReturnType<typeof setInterval> | null>(null);

  $effect(() => {
    loadSpawns();
    return () => {
      if (pollInterval) clearInterval(pollInterval);
    };
  });

  $effect(() => {
    if (summary.running > 0 && !pollInterval) {
      pollInterval = setInterval(loadSpawns, 3000);
    } else if (summary.running === 0 && pollInterval) {
      clearInterval(pollInterval);
      pollInterval = null;
    }
  });

  let byWave = $derived.by(() => {
    const waveMap = new Map<number, SpawnRecord[]>();
    spawns.forEach(s => {
      const w = s.waveNumber ?? s.wave ?? 0;
      if (!waveMap.has(w)) waveMap.set(w, []);
      waveMap.get(w)!.push(s);
    });
    return Array.from(waveMap.entries())
      .sort(([a], [b]) => a - b)
      .map(([wave, records]) => ({
        wave,
        records: records.sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
        running: records.filter(r => r.status === "running").length,
        done: records.filter(r => r.status === "complete" || r.status === "done").length,
        failed: records.filter(r => r.status === "failed").length,
      }));
  });

  function statusColor(status: SpawnRecord["status"]): string {
    switch (status) {
      case "running": return "var(--accent-blue, #60a5fa)";
      case "complete": case "done": return "var(--accent-green, #4ade80)";
      case "failed": return "var(--accent-red, #f87171)";
      case "pending": return "var(--text-tertiary, #666)";
      case "skipped": return "var(--text-tertiary, #888)";
      case "zombie": return "var(--accent-orange, #fb923c)";
      default: return "var(--text-secondary, #999)";
    }
  }

  function statusIcon(status: SpawnRecord["status"]): string {
    switch (status) {
      case "running": return "⟳";
      case "complete": case "done": return "✓";
      case "failed": return "✕";
      case "pending": return "○";
      case "skipped": return "⊘";
      case "zombie": return "⚠";
      default: return "?";
    }
  }

  function agentName(sessionId: string): string {
    // Extract readable name from session ID (e.g. "spawn:coder:abc123" → "coder")
    const parts = sessionId.split(":");
    return parts.length >= 2 ? parts[1] : sessionId.slice(0, 12);
  }
</script>

{#if summary.total > 0}
  <div class="spawn-status">
    <button class="spawn-header" onclick={() => collapsed = !collapsed}>
      <span class="header-title">
        <span class="header-icon">🤖</span>
        Sub-Agents
      </span>

      <span class="pills">
        {#if summary.running > 0}
          <span class="pill running">{summary.running} active</span>
        {/if}
        {#if summary.complete > 0}
          <span class="pill complete">{summary.complete} done</span>
        {/if}
        {#if summary.failed > 0}
          <span class="pill failed">{summary.failed} failed</span>
        {/if}
        {#if summary.pending > 0}
          <span class="pill pending">{summary.pending} queued</span>
        {/if}
      </span>

      <span class="chevron" class:rotated={!collapsed}>▸</span>
    </button>

    {#if !collapsed}
      <div class="spawn-body">
        {#if error}
          <div class="error">{error}</div>
        {:else if loading}
          <div class="loading">Loading sub-agents…</div>
        {:else}
          {#each byWave as { wave, records, running, done, failed }}
            <div class="wave-group">
              <div class="wave-label">
                Wave {wave}
                <span class="wave-counts">
                  {#if running > 0}<span class="mini running">{running}⟳</span>{/if}
                  {#if done > 0}<span class="mini complete">{done}✓</span>{/if}
                  {#if failed > 0}<span class="mini failed">{failed}✕</span>{/if}
                </span>
              </div>

              {#each records as spawn}
                <button
                  class="spawn-row"
                  class:expanded={expandedSpawn === spawn.id}
                  onclick={() => expandedSpawn = expandedSpawn === spawn.id ? null : spawn.id}
                >
                  <span class="status-dot" style="color: {statusColor(spawn.status)}">
                    {statusIcon(spawn.status)}
                  </span>
                  <span class="agent-name">{agentName(spawn.agentSessionId)}</span>
                  <span class="status-label" style="color: {statusColor(spawn.status)}">{spawn.status}</span>
                  {#if spawn.startedAt}
                    <span class="time-ago">{timeAgo(new Date(spawn.startedAt))}</span>
                  {/if}
                </button>

                {#if expandedSpawn === spawn.id}
                  <div class="spawn-details">
                    <div class="detail-row">
                      <span class="label">Session:</span>
                      <span class="value mono">{spawn.agentSessionId}</span>
                    </div>
                    <div class="detail-row">
                      <span class="label">Phase:</span>
                      <span class="value">{spawn.phaseId}</span>
                    </div>
                    {#if spawn.startedAt}
                      <div class="detail-row">
                        <span class="label">Started:</span>
                        <span class="value">{new Date(spawn.startedAt).toLocaleTimeString()}</span>
                      </div>
                    {/if}
                    {#if spawn.completedAt}
                      <div class="detail-row">
                        <span class="label">Completed:</span>
                        <span class="value">{new Date(spawn.completedAt).toLocaleTimeString()}</span>
                      </div>
                    {/if}
                    {#if spawn.result}
                      <div class="detail-row detail-result">
                        <span class="label">Result:</span>
                        <pre class="value result-text">{spawn.result}</pre>
                      </div>
                    {/if}
                    {#if spawn.errorMessage}
                      <div class="detail-row detail-error">
                        <span class="label">Error:</span>
                        <pre class="value error-text">{spawn.errorMessage}</pre>
                      </div>
                    {/if}
                  </div>
                {/if}
              {/each}
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .spawn-status {
    border: 1px solid var(--border-color, #333);
    border-radius: 8px;
    overflow: hidden;
  }

  .spawn-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 10px 14px;
    background: var(--bg-secondary, #1a1a1a);
    border: none;
    color: var(--text-primary, #e0e0e0);
    cursor: pointer;
    font-size: 13px;
    text-align: left;
  }

  .spawn-header:hover {
    background: var(--bg-tertiary, #222);
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }

  .header-icon {
    font-size: 14px;
  }

  .pills {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }

  .pill {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .pill.running {
    background: rgba(96, 165, 250, 0.15);
    color: var(--accent-blue, #60a5fa);
  }

  .pill.complete {
    background: rgba(74, 222, 128, 0.15);
    color: var(--accent-green, #4ade80);
  }

  .pill.failed {
    background: rgba(248, 113, 113, 0.15);
    color: var(--accent-red, #f87171);
  }

  .pill.pending {
    background: rgba(156, 163, 175, 0.15);
    color: var(--text-tertiary, #888);
  }

  .chevron {
    font-size: 12px;
    transition: transform 0.15s ease;
    color: var(--text-tertiary, #666);
  }

  .chevron.rotated {
    transform: rotate(90deg);
  }

  .spawn-body {
    padding: 4px 0;
    background: var(--bg-primary, #111);
  }

  .wave-group {
    padding: 4px 0;
  }

  .wave-group + .wave-group {
    border-top: 1px solid var(--border-color, #2a2a2a);
  }

  .wave-label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 14px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary, #888);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .wave-counts {
    display: flex;
    gap: 6px;
  }

  .mini {
    font-size: 10px;
    font-weight: 500;
  }

  .mini.running { color: var(--accent-blue, #60a5fa); }
  .mini.complete { color: var(--accent-green, #4ade80); }
  .mini.failed { color: var(--accent-red, #f87171); }

  .spawn-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 14px 6px 24px;
    background: transparent;
    border: none;
    color: var(--text-primary, #e0e0e0);
    cursor: pointer;
    font-size: 12px;
    text-align: left;
  }

  .spawn-row:hover {
    background: var(--bg-secondary, #1a1a1a);
  }

  .spawn-row.expanded {
    background: var(--bg-secondary, #1a1a1a);
  }

  .status-dot {
    font-size: 13px;
    width: 16px;
    text-align: center;
    flex-shrink: 0;
  }

  .agent-name {
    font-weight: 500;
    flex: 1;
  }

  .status-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .time-ago {
    font-size: 11px;
    color: var(--text-tertiary, #666);
    white-space: nowrap;
  }

  .spawn-details {
    padding: 8px 14px 8px 48px;
    background: var(--bg-tertiary, #161616);
    border-top: 1px solid var(--border-color, #2a2a2a);
  }

  .detail-row {
    display: flex;
    gap: 8px;
    padding: 3px 0;
    font-size: 12px;
  }

  .detail-row .label {
    color: var(--text-tertiary, #888);
    min-width: 70px;
    flex-shrink: 0;
  }

  .detail-row .value {
    color: var(--text-secondary, #ccc);
    word-break: break-all;
  }

  .mono {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .detail-result,
  .detail-error {
    flex-direction: column;
    gap: 4px;
  }

  .result-text,
  .error-text {
    margin: 0;
    padding: 6px 8px;
    border-radius: 4px;
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    white-space: pre-wrap;
    max-height: 200px;
    overflow-y: auto;
  }

  .result-text {
    background: rgba(74, 222, 128, 0.08);
    border: 1px solid rgba(74, 222, 128, 0.15);
  }

  .error-text {
    background: rgba(248, 113, 113, 0.08);
    border: 1px solid rgba(248, 113, 113, 0.15);
    color: var(--accent-red, #f87171);
  }

  .error {
    padding: 8px 14px;
    color: var(--accent-red, #f87171);
    font-size: 12px;
  }

  .loading {
    padding: 12px 14px;
    color: var(--text-tertiary, #888);
    font-size: 12px;
    text-align: center;
  }
</style>
