<script lang="ts">
  interface ExecutionSnapshot {
    projectId: string;
    state: string;
    activeWave: number | null;
    plans: PlanEntry[];
    activePlanIds: string[];
    startedAt: string | null;
    completedAt: string | null;
  }

  interface PlanEntry {
    phaseId: string;
    name: string;
    status: string;
    waveNumber: number | null;
    startedAt: string | null;
    completedAt: string | null;
    error: string | null;
  }

  let { execution }: { execution: ExecutionSnapshot } = $props();

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

  function formatDuration(start: string | null, end: string | null): string {
    if (!start) return "";
    
    const startTime = new Date(start);
    const endTime = end ? new Date(end) : new Date();
    const diffMs = endTime.getTime() - startTime.getTime();
    
    const minutes = Math.floor(diffMs / 60000);
    const seconds = Math.floor((diffMs % 60000) / 1000);
    
    if (minutes > 0) {
      return `${minutes}m ${seconds}s`;
    }
    return `${seconds}s`;
  }

  // Group plans by wave
  let waveGroups = $derived.by(() => {
    const waves = new Map<number, PlanEntry[]>();
    
    for (const plan of execution.plans) {
      const wave = plan.waveNumber ?? 0;
      if (!waves.has(wave)) {
        waves.set(wave, []);
      }
      waves.get(wave)!.push(plan);
    }
    
    return Array.from(waves.entries())
      .sort(([a], [b]) => a - b)
      .map(([wave, plans]) => ({
        wave,
        plans: plans.sort((a, b) => a.name.localeCompare(b.name)),
        status: getWaveStatus(plans)
      }));
  });

  function getWaveStatus(plans: PlanEntry[]): "pending" | "running" | "done" | "failed" | "mixed" {
    const statuses = new Set(plans.map(p => p.status));
    
    if (statuses.has("failed")) return "failed";
    if (statuses.has("running")) return "running";
    if (statuses.size === 1) {
      const status = Array.from(statuses)[0];
      return ["pending", "done"].includes(status) ? status as "pending" | "done" : "mixed";
    }
    return "mixed";
  }

  function waveStatusColor(status: string): string {
    switch (status) {
      case "pending": return "#64748b"; // gray
      case "running": return "#f59e0b"; // amber
      case "done": return "#10b981"; // green
      case "failed": return "#ef4444"; // red
      case "mixed": return "#8b5cf6"; // purple
      default: return "#64748b";
    }
  }

  function planStatusColor(status: string): string {
    switch (status) {
      case "pending": return "#64748b";
      case "running": return "#f59e0b";
      case "done": return "#10b981";
      case "failed": return "#ef4444";
      case "skipped": return "#64748b";
      case "zombie": return "#e87c3e";
      default: return "#64748b";
    }
  }

  let overallProgress = $derived.by(() => {
    const total = execution.plans.length;
    if (total === 0) return 0;
    
    const completed = execution.plans.filter(p => ["done", "skipped"].includes(p.status)).length;
    return Math.round((completed / total) * 100);
  });

  let totalDuration = $derived(() => {
    if (!execution.startedAt) return null;
    return formatDuration(execution.startedAt, execution.completedAt);
  });
</script>

<div class="execution-wave-status">
  <div class="execution-header">
    <h3>Wave Execution</h3>
    <div class="execution-summary">
      <div class="progress-summary">
        <span class="progress-text">{overallProgress}% complete</span>
        <div class="progress-bar">
          <div class="progress-fill" style="width: {overallProgress}%"></div>
        </div>
      </div>
      {#if totalDuration}
        <span class="duration">Duration: {totalDuration}</span>
      {/if}
    </div>
  </div>

  {#if execution.plans.length === 0}
    <div class="empty-state">
      <span class="empty-icon">⚡</span>
      <p>No execution plans yet</p>
    </div>
  {:else}
    <div class="waves-container">
      {#each waveGroups as { wave, plans, status } (wave)}
        <div class="wave-group">
          <div class="wave-header">
            <div class="wave-info">
              <h4 class="wave-title">Wave {wave + 1}</h4>
              <span class="wave-count">{plans.length} plan{plans.length !== 1 ? 's' : ''}</span>
            </div>
            <span 
              class="wave-status-badge" 
              style="background-color: {waveStatusColor(status)}"
            >
              {statusLabel(status)}
            </span>
          </div>

          <div class="plans-grid">
            {#each plans as plan (plan.phaseId)}
              <div class="plan-item" class:current={execution.activePlanIds.includes(plan.phaseId)}>
                <div class="plan-header">
                  <div class="plan-info">
                    <span class="plan-name">{plan.name}</span>
                    {#if plan.startedAt}
                      <span class="plan-duration">
                        {formatDuration(plan.startedAt, plan.completedAt)}
                      </span>
                    {/if}
                  </div>
                  <span 
                    class="plan-status-badge" 
                    style="background-color: {planStatusColor(plan.status)}"
                  >
                    {statusLabel(plan.status)}
                  </span>
                </div>

                {#if plan.status === "running"}
                  <div class="plan-progress">
                    <div class="progress-spinner"></div>
                    <span class="progress-label">Executing...</span>
                  </div>
                {/if}

                {#if plan.error}
                  <div class="plan-error">
                    <span class="error-icon">⚠️</span>
                    <span class="error-text">{plan.error}</span>
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
  .execution-wave-status {
    background: var(--bg);
    border-radius: var(--radius-sm);
  }

  .execution-header {
    margin-bottom: 20px;
  }

  .execution-header h3 {
    margin: 0 0 12px 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .execution-summary {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }

  .progress-summary {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 200px;
  }

  .progress-text {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .progress-bar {
    flex: 1;
    height: 6px;
    background: var(--surface);
    border-radius: var(--radius-pill);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: var(--radius-pill);
    transition: width 0.3s ease;
  }

  .duration {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 24px;
    text-align: center;
    color: var(--text-muted);
  }

  .empty-icon {
    font-size: 2rem;
    margin-bottom: 8px;
  }

  .empty-state p {
    margin: 0;
    font-size: var(--text-sm);
  }

  .waves-container {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .wave-group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .wave-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
  }

  .wave-info {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .wave-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
  }

  .wave-count {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-weight: 500;
  }

  .wave-status-badge {
    font-size: var(--text-2xs);
    font-weight: 600;
    padding: 3px 8px;
    border-radius: var(--radius-pill);
    color: white;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .plans-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 1px;
    background: var(--border);
    padding: 0;
  }

  .plan-item {
    background: var(--surface);
    padding: 12px;
    transition: background var(--transition-quick);
  }

  .plan-item.current {
    background: rgba(154, 123, 79, 0.05);
    border: 1px solid var(--accent);
    margin: -1px;
  }

  .plan-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }

  .plan-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .plan-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .plan-duration {
    font-size: var(--text-2xs);
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .plan-status-badge {
    font-size: var(--text-2xs);
    font-weight: 600;
    padding: 2px 6px;
    border-radius: var(--radius-pill);
    color: white;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    flex-shrink: 0;
  }

  .plan-progress {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
  }

  .progress-spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--surface);
    border-top: 2px solid var(--accent);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  .progress-label {
    font-size: var(--text-2xs);
    color: var(--text-muted);
    font-style: italic;
  }

  .plan-error {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin-top: 6px;
    padding: 6px;
    background: var(--status-error-bg);
    border: 1px solid var(--status-error-border);
    border-radius: var(--radius-sm);
  }

  .error-icon {
    font-size: var(--text-xs);
    flex-shrink: 0;
  }

  .error-text {
    font-size: var(--text-2xs);
    color: var(--status-error);
    line-height: 1.3;
  }

  @media (max-width: 768px) {
    .execution-summary {
      flex-direction: column;
      align-items: stretch;
      gap: 8px;
    }

    .progress-summary {
      min-width: 0;
    }

    .plans-grid {
      grid-template-columns: 1fr;
    }

    .wave-header {
      flex-direction: column;
      align-items: stretch;
      gap: 8px;
    }

    .wave-info {
      justify-content: space-between;
    }

    .wave-status-badge {
      align-self: flex-end;
    }

    .plan-header {
      flex-direction: column;
      align-items: stretch;
      gap: 4px;
    }

    .plan-info {
      flex-direction: row;
      align-items: center;
      justify-content: space-between;
    }
  }
</style>