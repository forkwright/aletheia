<script lang="ts">
  import { timeAgo } from "../../lib/utils";

  interface ExecutionPlan {
    phaseId: string;
    name: string;
    status: "pending" | "running" | "done" | "failed" | "skipped" | "zombie";
    waveNumber: number | null;
    startedAt: string | null;
    completedAt: string | null;
    error: string | null;
  }

  let { plans, projectState }: { 
    plans: ExecutionPlan[];
    projectState: string;
  } = $props();

  let selectedWave = $state<number | null>(null);
  let expandedPlan = $state<string | null>(null);

  let waves = $derived.by(() => {
    const waveMap = new Map<number, ExecutionPlan[]>();
    
    plans.forEach(plan => {
      if (plan.waveNumber !== null) {
        if (!waveMap.has(plan.waveNumber)) {
          waveMap.set(plan.waveNumber, []);
        }
        waveMap.get(plan.waveNumber)!.push(plan);
      }
    });
    
    return Array.from(waveMap.entries())
      .sort(([a], [b]) => a - b)
      .map(([waveNumber, wavePlans]) => ({
        number: waveNumber,
        plans: wavePlans.sort((a, b) => a.name.localeCompare(b.name)),
        status: getWaveStatus(wavePlans)
      }));
  });

  let currentWave = $derived.by(() => {
    return waves.find(w => w.status === "running") || waves[waves.length - 1] || null;
  });

  function getWaveStatus(plans: ExecutionPlan[]): "pending" | "running" | "done" | "failed" | "mixed" {
    const statuses = plans.map(p => p.status);
    
    if (statuses.every(s => s === "done" || s === "skipped")) return "done";
    if (statuses.some(s => s === "failed")) return "failed";
    if (statuses.some(s => s === "running")) return "running";
    if (statuses.every(s => s === "pending")) return "pending";
    
    return "mixed";
  }

  function getPlanStatusColor(status: "pending" | "running" | "done" | "failed" | "skipped" | "zombie"): string {
    switch (status) {
      case "done": return "var(--status-success)";
      case "running": return "var(--status-active)";
      case "failed": return "var(--status-error)";
      case "zombie": return "var(--status-warning)";
      case "skipped": return "var(--text-muted)";
      case "pending": return "var(--text-muted)";
      default: return "var(--text-muted)";
    }
  }

  function getPlanStatusIcon(status: "pending" | "running" | "done" | "failed" | "skipped" | "zombie"): string {
    switch (status) {
      case "done": return "✅";
      case "running": return "🔄";
      case "failed": return "❌";
      case "zombie": return "⚠️";
      case "skipped": return "⏭️";
      case "pending": return "⏸️";
      default: return "⚪";
    }
  }

  function getWaveStatusColor(status: "pending" | "running" | "done" | "failed" | "mixed"): string {
    switch (status) {
      case "done": return "var(--status-success)";
      case "running": return "var(--status-active)";
      case "failed": return "var(--status-error)";
      case "mixed": return "var(--status-warning)";
      case "pending": return "var(--text-muted)";
      default: return "var(--text-muted)";
    }
  }

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

  function toggleExpanded(planId: string) {
    expandedPlan = expandedPlan === planId ? null : planId;
  }

  let overallProgress = $derived.by(() => {
    if (plans.length === 0) return { completed: 0, total: 0, percentage: 0 };
    
    const completed = plans.filter(p => p.status === "done" || p.status === "skipped").length;
    const total = plans.length;
    const percentage = Math.round((completed / total) * 100);
    
    return { completed, total, percentage };
  });
</script>

<div class="execution-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">⚡</span>
      Execution Status
      {#if currentWave}
        <span class="current-wave">Wave {currentWave.number}</span>
      {/if}
    </h2>
    
    {#if plans.length > 0}
      <div class="progress-summary">
        <div class="progress-bar">
          <div 
            class="progress-fill" 
            style="width: {overallProgress.percentage}%"
          ></div>
        </div>
        <span class="progress-text">
          {overallProgress.completed}/{overallProgress.total} ({overallProgress.percentage}%)
        </span>
      </div>
    {/if}
  </div>

  <div class="execution-container">
    {#if plans.length === 0}
      <div class="empty-execution">
        <span class="empty-icon">⚡</span>
        <span>No execution plans available</span>
        {#if projectState === "executing"}
          <small>Plans are being generated...</small>
        {/if}
      </div>
    {:else if waves.length === 0}
      <div class="no-waves">
        <span class="empty-icon">📦</span>
        <span>Plans exist but no waves are defined</span>
      </div>
    {:else}
      <!-- Wave Navigation -->
      <div class="wave-tabs">
        {#each waves as wave (wave.number)}
          <button 
            class="wave-tab"
            class:active={selectedWave === wave.number}
            class:current={currentWave?.number === wave.number}
            style="--wave-color: {getWaveStatusColor(wave.status)}"
            onclick={() => selectedWave = selectedWave === wave.number ? null : wave.number}
          >
            <span class="wave-number">Wave {wave.number}</span>
            <span class="wave-status">{wave.plans.length} plans</span>
            <span class="wave-indicator" style="background: {getWaveStatusColor(wave.status)}"></span>
          </button>
        {/each}
      </div>

      <!-- Wave Content -->
      <div class="wave-content">
        {#if selectedWave !== null}
          {@const wave = waves.find(w => w.number === selectedWave)}
          {#if wave}
            <div class="wave-plans">
              {#each wave.plans as plan (plan.phaseId + "-" + plan.name)}
                <div class="plan-item" class:expanded={expandedPlan === plan.phaseId + "-" + plan.name}>
                  <div
                    class="plan-main"
                    role="button"
                    tabindex="0"
                    onclick={() => toggleExpanded(plan.phaseId + "-" + plan.name)}
                    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggleExpanded(plan.phaseId + "-" + plan.name); } }}
                  >
                    <div class="plan-status-indicator" style="background: {getPlanStatusColor(plan.status)}">
                      {getPlanStatusIcon(plan.status)}
                    </div>
                    
                    <div class="plan-info">
                      <div class="plan-name">{plan.name}</div>
                      <div class="plan-meta">
                        <span class="plan-status">{statusLabel(plan.status)}</span>
                        {#if plan.startedAt}
                          <span class="plan-time">Started {timeAgo(new Date(plan.startedAt))}</span>
                        {/if}
                        {#if plan.completedAt}
                          <span class="plan-time">Completed {timeAgo(new Date(plan.completedAt))}</span>
                        {/if}
                      </div>
                    </div>
                    
                    {#if plan.error || (plan.status !== "pending" && plan.status !== "running")}
                      <div class="expand-arrow" class:rotated={expandedPlan === plan.phaseId + "-" + plan.name}>
                        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                          <path 
                            d="M6 12l4-4-4-4" 
                            stroke="currentColor" 
                            stroke-width="1.5" 
                            stroke-linecap="round" 
                            stroke-linejoin="round"
                          />
                        </svg>
                      </div>
                    {/if}
                  </div>
                  
                  {#if expandedPlan === plan.phaseId + "-" + plan.name}
                    <div class="plan-details">
                      {#if plan.error}
                        <div class="error-section">
                          <strong>Error:</strong>
                          <pre class="error-text">{plan.error}</pre>
                        </div>
                      {/if}
                      
                      <div class="timing-section">
                        {#if plan.startedAt}
                          <div class="timing-item">
                            <strong>Started:</strong> 
                            {new Date(plan.startedAt).toLocaleString()}
                          </div>
                        {/if}
                        {#if plan.completedAt}
                          <div class="timing-item">
                            <strong>Completed:</strong> 
                            {new Date(plan.completedAt).toLocaleString()}
                          </div>
                        {/if}
                        {#if plan.startedAt && plan.completedAt}
                          {@const duration = new Date(plan.completedAt).getTime() - new Date(plan.startedAt).getTime()}
                          {@const minutes = Math.floor(duration / (1000 * 60))}
                          {@const seconds = Math.floor((duration % (1000 * 60)) / 1000)}
                          <div class="timing-item">
                            <strong>Duration:</strong>
                            {#if minutes > 0}{minutes}m {/if}{seconds}s
                          </div>
                        {/if}
                      </div>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          <!-- Show current/latest wave by default -->
          {#if currentWave}
            <div class="auto-wave-display">
              <h3>Current Wave: {currentWave.number}</h3>
              <div class="wave-plans">
                {#each currentWave.plans.slice(0, 5) as plan}
                  <div class="plan-summary">
                    <span class="plan-status-dot" style="background: {getPlanStatusColor(plan.status)}"></span>
                    <span class="plan-name">{plan.name}</span>
                    <span class="plan-status-label">{statusLabel(plan.status)}</span>
                  </div>
                {/each}
                {#if currentWave.plans.length > 5}
                  <div class="more-plans">
                    +{currentWave.plans.length - 5} more plans
                  </div>
                {/if}
              </div>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .execution-section {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .section-header {
    margin-bottom: var(--space-3);
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
    margin: 0 0 var(--space-2) 0;
  }

  .title-icon {
    font-size: var(--text-xl);
  }

  .current-wave {
    background: var(--status-active);
    color: white;
    font-size: var(--text-xs);
    font-weight: 600;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-pill);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .progress-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .progress-bar {
    flex: 1;
    height: 4px;
    background: var(--surface);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--status-success);
    transition: width var(--transition-quick);
  }

  .progress-text {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .execution-container {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .empty-execution,
  .no-waves {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-6);
    color: var(--text-muted);
    font-size: var(--text-sm);
    text-align: center;
  }

  .empty-execution small {
    font-size: var(--text-xs);
    color: var(--text-muted);
    opacity: 0.8;
  }

  .empty-icon {
    font-size: var(--text-lg);
  }

  .wave-tabs {
    display: flex;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
    overflow-x: auto;
    padding-bottom: var(--space-1);
  }

  .wave-tab {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-quick);
    position: relative;
    white-space: nowrap;
  }

  .wave-tab:hover {
    background: var(--surface-hover);
  }

  .wave-tab.active {
    background: var(--accent-muted);
    border-color: var(--accent);
    color: var(--accent);
  }

  .wave-tab.current {
    background: color-mix(in srgb, var(--status-active) 15%, transparent);
    border-color: var(--status-active);
  }

  .wave-number {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
  }

  .wave-status {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .wave-indicator {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .wave-content {
    flex: 1;
    overflow-y: auto;
  }

  .wave-plans {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .plan-item {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .plan-item.expanded {
    border-color: var(--accent);
  }

  .plan-main {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .plan-main:hover {
    background: var(--surface);
  }

  .plan-status-indicator {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-size: var(--text-xs);
    flex-shrink: 0;
  }

  .plan-info {
    flex: 1;
    min-width: 0;
  }

  .plan-name {
    font-weight: 600;
    color: var(--text);
    margin-bottom: var(--space-1);
  }

  .plan-meta {
    display: flex;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .plan-status {
    font-weight: 500;
  }

  .plan-time {
    color: var(--text-muted);
  }

  .expand-arrow {
    color: var(--text-muted);
    transition: transform var(--transition-quick);
    flex-shrink: 0;
  }

  .expand-arrow.rotated {
    transform: rotate(90deg);
    color: var(--accent);
  }

  .plan-details {
    padding: var(--space-3);
    background: var(--surface);
    border-top: 1px solid var(--border);
    font-size: var(--text-sm);
  }

  .error-section {
    margin-bottom: var(--space-3);
  }

  .error-section strong {
    color: var(--status-error);
    display: block;
    margin-bottom: var(--space-1);
  }

  .error-text {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    color: var(--status-error);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    overflow-x: auto;
  }

  .timing-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .timing-item {
    display: flex;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .timing-item strong {
    color: var(--text);
    min-width: 80px;
  }

  .auto-wave-display h3 {
    font-size: var(--text-base);
    color: var(--text);
    margin: 0 0 var(--space-3) 0;
    font-weight: 600;
  }

  .plan-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--surface);
    border-radius: var(--radius-sm);
  }

  .plan-status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .plan-summary .plan-name {
    flex: 1;
    font-size: var(--text-sm);
    font-weight: 500;
    margin: 0;
  }

  .plan-status-label {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .more-plans {
    text-align: center;
    padding: var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-style: italic;
  }

  @media (max-width: 768px) {
    .wave-tabs {
      gap: var(--space-1);
    }

    .wave-tab {
      padding: var(--space-1) var(--space-2);
    }

    .plan-main {
      padding: var(--space-2);
      gap: var(--space-2);
    }

    .plan-meta {
      flex-direction: column;
      gap: var(--space-1);
    }
  }
</style>