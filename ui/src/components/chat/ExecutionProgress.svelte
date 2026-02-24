<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";

  interface PlanEntry {
    phaseId: string;
    name: string;
    status: string;
    waveNumber: number | null;
    startedAt: string | null;
    completedAt: string | null;
    error: string | null;
  }

  interface ExecutionData {
    projectId: string;
    state: string;
    activeWave: number | null;
    plans: PlanEntry[];
    activePlanIds: string[];
    startedAt: string | null;
    completedAt: string | null;
  }

  let { execution }: {
    execution: ExecutionData;
  } = $props();

  function statusLabel(status: string): string {
    switch (status) {
      case "pending": return "Pending";
      case "running": return "Running";
      case "done": return "Done";
      case "failed": return "Failed";
      case "skipped": return "Skipped";
      case "zombie": return "Zombie";
      default: return status.charAt(0).toUpperCase() + status.slice(1);
    }
  }

  function getStatusIcon(status: string): string {
    switch (status) {
      case "done": return "✓";
      case "running": return "⟳";
      case "failed": return "✗";
      case "skipped": return "→";
      case "zombie": return "⚠";
      case "pending": return "○";
      default: return "○";
    }
  }

  function getStatusColor(status: string): string {
    switch (status) {
      case "done": return "status-done";
      case "running": return "status-running";
      case "failed": return "status-failed";
      case "skipped": return "status-skipped";
      case "zombie": return "status-zombie";
      case "pending": return "status-pending";
      default: return "status-pending";
    }
  }

  function formatDuration(startedAt: string | null, completedAt: string | null): string {
    if (!startedAt) return "";
    
    const start = new Date(startedAt);
    const end = completedAt ? new Date(completedAt) : new Date();
    const durationMs = end.getTime() - start.getTime();
    
    const seconds = Math.floor(durationMs / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    
    if (hours > 0) {
      return `${hours}h ${minutes % 60}m`;
    } else if (minutes > 0) {
      return `${minutes}m ${seconds % 60}s`;
    } else {
      return `${seconds}s`;
    }
  }

  function formatTimestamp(timestamp: string | null): string {
    if (!timestamp) return "";
    return new Date(timestamp).toLocaleTimeString();
  }

  let waveGroups = $derived(() => {
    // Group plans by wave number
    const groups = new Map<number, PlanEntry[]>();
    
    execution.plans.forEach(plan => {
      const wave = plan.waveNumber ?? 0;
      if (!groups.has(wave)) {
        groups.set(wave, []);
      }
      groups.get(wave)!.push(plan);
    });
    
    // Convert to sorted array
    return Array.from(groups.entries())
      .sort(([a], [b]) => a - b)
      .map(([waveNumber, plans]) => ({
        waveNumber,
        plans: plans.sort((a, b) => a.name.localeCompare(b.name)),
        isActive: execution.activeWave === waveNumber,
        isComplete: plans.every(p => p.status === "done" || p.status === "skipped"),
        hasFailed: plans.some(p => p.status === "failed" || p.status === "zombie"),
      }));
  });

  let overallStats = $derived(() => {
    const total = execution.plans.length;
    const done = execution.plans.filter(p => p.status === "done").length;
    const running = execution.plans.filter(p => p.status === "running").length;
    const failed = execution.plans.filter(p => p.status === "failed" || p.status === "zombie").length;
    const skipped = execution.plans.filter(p => p.status === "skipped").length;
    const pending = execution.plans.filter(p => p.status === "pending").length;
    
    return {
      total,
      done,
      running,
      failed,
      skipped,
      pending,
      progress: total > 0 ? Math.round((done / total) * 100) : 0,
    };
  });
</script>

<div class="execution-progress">
  <div class="progress-header">
    <h4>Execution Progress</h4>
    <div class="overall-progress">
      <div class="progress-bar">
        <div 
          class="progress-fill" 
          style="width: {overallStats.progress}%"
        ></div>
        <span class="progress-text">
          {overallStats.done}/{overallStats.total} Complete ({overallStats.progress}%)
        </span>
      </div>
    </div>
  </div>

  <div class="stats-summary">
    <div class="stat-item">
      <span class="stat-value done">{overallStats.done}</span>
      <span class="stat-label">Done</span>
    </div>
    <div class="stat-item">
      <span class="stat-value running">{overallStats.running}</span>
      <span class="stat-label">Running</span>
    </div>
    <div class="stat-item">
      <span class="stat-value failed">{overallStats.failed}</span>
      <span class="stat-label">Failed</span>
    </div>
    <div class="stat-item">
      <span class="stat-value pending">{overallStats.pending}</span>
      <span class="stat-label">Pending</span>
    </div>
  </div>

  {#if waveGroups.length === 0}
    <div class="empty-state">
      <span>No execution plans available</span>
    </div>
  {:else}
    <div class="waves-container">
      {#each waveGroups as waveGroup (waveGroup.waveNumber)}
        <div class="wave-group" class:active={waveGroup.isActive}>
          <div class="wave-header">
            <div class="wave-title">
              <span class="wave-icon">
                {#if waveGroup.isComplete}
                  ✓
                {:else if waveGroup.isActive}
                  ⟳
                {:else if waveGroup.hasFailed}
                  ✗
                {:else}
                  ○
                {/if}
              </span>
              <h5>Wave {waveGroup.waveNumber + 1}</h5>
              <span class="wave-status">
                {#if waveGroup.isComplete}
                  Complete
                {:else if waveGroup.isActive}
                  Running
                {:else if waveGroup.hasFailed}
                  Failed
                {:else}
                  Pending
                {/if}
              </span>
            </div>
            <div class="wave-progress">
              {waveGroup.plans.filter(p => p.status === "done").length}/{waveGroup.plans.length}
            </div>
          </div>
          
          <div class="plans-list">
            {#each waveGroup.plans as plan (plan.phaseId)}
              <div class="plan-item {getStatusColor(plan.status)}">
                <div class="plan-header">
                  <div class="plan-icon">
                    {#if plan.status === "running"}
                      <Spinner size={14} />
                    {:else}
                      {getStatusIcon(plan.status)}
                    {/if}
                  </div>
                  <div class="plan-info">
                    <span class="plan-name">{plan.name}</span>
                    <span class="plan-status">{statusLabel(plan.status)}</span>
                  </div>
                  {#if plan.startedAt}
                    <div class="plan-timing">
                      {#if plan.status === "running"}
                        <span class="duration">{formatDuration(plan.startedAt, null)}</span>
                      {:else if plan.completedAt}
                        <span class="duration">{formatDuration(plan.startedAt, plan.completedAt)}</span>
                      {/if}
                      <span class="timestamp">{formatTimestamp(plan.startedAt)}</span>
                    </div>
                  {/if}
                </div>
                
                {#if plan.error}
                  <div class="plan-error">
                    <span class="error-icon">⚠️</span>
                    <span class="error-message">{plan.error}</span>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .execution-progress {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .progress-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
  }

  .progress-header h4 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .overall-progress {
    flex: 1;
    max-width: 300px;
  }

  .progress-bar {
    position: relative;
    width: 100%;
    height: 24px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: linear-gradient(90deg, var(--status-success), var(--accent));
    transition: width 0.3s ease;
  }

  .progress-text {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  }

  .stats-summary {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(80px, 1fr));
    gap: 16px;
    padding: 16px;
    background: var(--bg-elevated);
    border-radius: var(--radius-md);
  }

  .stat-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .stat-value {
    font-size: var(--text-xl);
    font-weight: 700;
  }

  .stat-value.done { color: var(--status-success); }
  .stat-value.running { color: var(--accent); }
  .stat-value.failed { color: var(--status-error); }
  .stat-value.pending { color: var(--text-muted); }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .empty-state {
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 40px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .waves-container {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .wave-group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .wave-group.active {
    border-color: var(--accent);
    background: rgba(154, 123, 79, 0.05);
  }

  .wave-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
  }

  .wave-title {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .wave-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .wave-title h5 {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
  }

  .wave-status {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    background: var(--surface);
    padding: 2px 6px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--border);
  }

  .wave-progress {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    font-weight: 500;
  }

  .plans-list {
    display: flex;
    flex-direction: column;
  }

  .plan-item {
    padding: 12px 16px;
    border-bottom: 1px solid rgba(var(--border-rgb), 0.5);
    transition: background var(--transition-quick);
  }

  .plan-item:last-child {
    border-bottom: none;
  }

  .plan-item:hover {
    background: rgba(var(--surface-hover-rgb), 0.5);
  }

  .plan-item.status-running {
    background: rgba(154, 123, 79, 0.05);
  }

  .plan-item.status-failed,
  .plan-item.status-zombie {
    background: rgba(var(--status-error-rgb), 0.05);
  }

  .plan-header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .plan-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-sm);
    font-weight: 600;
    flex-shrink: 0;
  }

  .plan-info {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .plan-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .plan-status {
    font-size: var(--text-xs);
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .plan-timing {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    flex-shrink: 0;
    text-align: right;
  }

  .duration {
    font-size: var(--text-xs);
    color: var(--text);
    font-weight: 500;
  }

  .timestamp {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin-top: 2px;
  }

  .plan-error {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    padding: 8px;
    background: var(--status-error-bg);
    border: 1px solid var(--status-error-border);
    border-radius: var(--radius-sm);
  }

  .error-icon {
    flex-shrink: 0;
  }

  .error-message {
    font-size: var(--text-sm);
    color: var(--status-error);
    flex: 1;
  }

  @media (max-width: 768px) {
    .progress-header {
      flex-direction: column;
      align-items: stretch;
      gap: 12px;
    }

    .overall-progress {
      max-width: none;
    }

    .stats-summary {
      grid-template-columns: repeat(2, 1fr);
    }

    .wave-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }

    .plan-header {
      flex-wrap: wrap;
    }

    .plan-timing {
      flex-basis: 100%;
      align-items: flex-start;
      text-align: left;
      margin-top: 4px;
    }
  }
</style>