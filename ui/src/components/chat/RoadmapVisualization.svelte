<script lang="ts">
  interface RoadmapPhase {
    id: string;
    name: string;
    goal: string;
    requirements: string[];
    dependencies: string[];
    status: "pending" | "planning" | "discussing" | "executing" | "verifying" | "complete";
    order: number;
  }

  let { 
    phases, 
    currentPhase 
  }: {
    phases: RoadmapPhase[];
    currentPhase?: string;
  } = $props();

  let expandedPhase = $state<string | null>(null);
  let selectedPhase = $state<string | null>(null);

  function toggleExpanded(phaseId: string) {
    expandedPhase = expandedPhase === phaseId ? null : phaseId;
  }

  function selectPhase(phaseId: string) {
    selectedPhase = selectedPhase === phaseId ? null : phaseId;
  }

  function statusColor(status: string): string {
    switch (status) {
      case "complete": return "#10b981"; // green
      case "executing": return "#f59e0b"; // amber
      case "verifying": return "#3b82f6"; // blue
      case "discussing": return "#8b5cf6"; // purple
      case "planning": return "#06b6d4"; // cyan
      case "pending": return "#64748b"; // gray
      default: return "#64748b";
    }
  }

  function statusIcon(status: string): string {
    switch (status) {
      case "complete": return "✓";
      case "executing": return "⚡";
      case "verifying": return "🔍";
      case "discussing": return "💬";
      case "planning": return "📝";
      case "pending": return "⏳";
      default: return "◯";
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "complete": return "Complete";
      case "executing": return "Executing";
      case "verifying": return "Verifying";
      case "discussing": return "Discussing";
      case "planning": return "Planning";
      case "pending": return "Pending";
      default: return status;
    }
  }

  let sortedPhases = $derived(phases.sort((a, b) => a.order - b.order));

  function getDependencyLines(): Array<{ fromPhase: string; toPhase: string; fromIndex: number; toIndex: number }> {
    const lines: Array<{ fromPhase: string; toPhase: string; fromIndex: number; toIndex: number }> = [];
    
    for (let i = 0; i < sortedPhases.length; i++) {
      const phase = sortedPhases[i];
      for (const depId of phase.dependencies) {
        const depIndex = sortedPhases.findIndex(p => p.id === depId);
        if (depIndex !== -1) {
          lines.push({
            fromPhase: depId,
            toPhase: phase.id,
            fromIndex: depIndex,
            toIndex: i
          });
        }
      }
    }
    
    return lines;
  }

  let dependencyLines = $derived(getDependencyLines());
</script>

<div class="roadmap-visualization">
  <div class="roadmap-header">
    <h3>Roadmap</h3>
    <div class="legend">
      <div class="legend-item">
        <span class="legend-icon" style="color: #10b981">✓</span>
        <span>Complete</span>
      </div>
      <div class="legend-item">
        <span class="legend-icon" style="color: #f59e0b">⚡</span>
        <span>Executing</span>
      </div>
      <div class="legend-item">
        <span class="legend-icon" style="color: #64748b">⏳</span>
        <span>Pending</span>
      </div>
    </div>
  </div>

  {#if phases.length === 0}
    <div class="empty-state">
      <span class="empty-icon">🗺️</span>
      <p>No roadmap phases defined yet</p>
    </div>
  {:else}
    <div class="timeline-container">
      <div class="timeline">
        <!-- Dependency lines (SVG overlay) -->
        {#if dependencyLines.length > 0}
          <svg class="dependency-lines" width="100%" height="100%">
            {#each dependencyLines as line}
              <line
                x1="90%"
                y1="{(line.fromIndex * 120) + 60}px"
                x2="10%"
                y2="{(line.toIndex * 120) + 60}px"
                stroke="#64748b"
                stroke-width="2"
                stroke-dasharray="4,4"
                opacity="0.5"
              />
            {/each}
          </svg>
        {/if}

        <!-- Phase items -->
        {#each sortedPhases as phase, index (phase.id)}
          <div class="timeline-item" class:current={currentPhase === phase.id}>
            <div class="timeline-marker">
              <div 
                class="status-circle" 
                style="background-color: {statusColor(phase.status)}"
              >
                {statusIcon(phase.status)}
              </div>
              {#if index < sortedPhases.length - 1}
                <div class="connector-line"></div>
              {/if}
            </div>
            
            <div class="phase-card" class:expanded={expandedPhase === phase.id}>
              <div 
                class="phase-header"
                onclick={() => toggleExpanded(phase.id)}
              >
                <div class="phase-info">
                  <h4 class="phase-name">{phase.name}</h4>
                  <p class="phase-goal">{phase.goal}</p>
                </div>
                <div class="phase-meta">
                  <span 
                    class="status-badge" 
                    style="background-color: {statusColor(phase.status)}"
                  >
                    {statusLabel(phase.status)}
                  </span>
                  <span class="expand-icon" class:rotated={expandedPhase === phase.id}>
                    ▼
                  </span>
                </div>
              </div>

              {#if expandedPhase === phase.id}
                <div class="phase-details">
                  {#if phase.requirements.length > 0}
                    <div class="details-section">
                      <h5>Requirements</h5>
                      <ul class="requirements-list">
                        {#each phase.requirements as reqId}
                          <li class="requirement-ref">{reqId}</li>
                        {/each}
                      </ul>
                    </div>
                  {/if}

                  {#if phase.dependencies.length > 0}
                    <div class="details-section">
                      <h5>Dependencies</h5>
                      <ul class="dependencies-list">
                        {#each phase.dependencies as depId}
                          {@const depPhase = phases.find(p => p.id === depId)}
                          <li class="dependency-ref">
                            {depPhase?.name || depId}
                          </li>
                        {/each}
                      </ul>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .roadmap-visualization {
    background: var(--bg);
    border-radius: var(--radius-sm);
  }

  .roadmap-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
    flex-wrap: wrap;
    gap: 12px;
  }

  .roadmap-header h3 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .legend {
    display: flex;
    gap: 16px;
    align-items: center;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .legend-icon {
    font-size: var(--text-sm);
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

  .timeline-container {
    position: relative;
    padding-left: 20px;
  }

  .timeline {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .dependency-lines {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    z-index: 1;
  }

  .timeline-item {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 16px;
    z-index: 2;
  }

  .timeline-item.current .phase-card {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .timeline-marker {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    flex-shrink: 0;
  }

  .status-circle {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-size: var(--text-sm);
    font-weight: 600;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    z-index: 3;
  }

  .connector-line {
    width: 2px;
    height: 60px;
    background: var(--border);
    margin-top: 8px;
  }

  .phase-card {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    transition: all var(--transition-quick);
  }

  .phase-card:hover {
    border-color: var(--border-hover);
  }

  .phase-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px;
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .phase-header:hover {
    background: var(--surface-hover);
  }

  .phase-info {
    flex: 1;
    min-width: 0;
  }

  .phase-name {
    margin: 0 0 4px 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
    line-height: 1.3;
  }

  .phase-goal {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--text-muted);
    line-height: 1.4;
  }

  .phase-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .status-badge {
    font-size: var(--text-2xs);
    font-weight: 600;
    padding: 3px 8px;
    border-radius: var(--radius-pill);
    color: white;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .expand-icon {
    font-size: var(--text-xs);
    color: var(--text-muted);
    transition: transform var(--transition-quick);
    transform: rotate(-90deg);
  }

  .expand-icon.rotated {
    transform: rotate(0deg);
  }

  .phase-details {
    padding: 0 16px 16px 16px;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border);
  }

  .details-section {
    margin-bottom: 12px;
  }

  .details-section:last-child {
    margin-bottom: 0;
  }

  .details-section h5 {
    margin: 0 0 6px 0;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .requirements-list,
  .dependencies-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .requirement-ref,
  .dependency-ref {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 2px 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .dependency-ref {
    background: rgba(139, 92, 246, 0.1);
    border-color: rgba(139, 92, 246, 0.3);
    color: #8b5cf6;
  }

  @media (max-width: 768px) {
    .roadmap-header {
      flex-direction: column;
      align-items: stretch;
    }

    .legend {
      justify-content: center;
    }

    .timeline-container {
      padding-left: 12px;
    }

    .timeline-item {
      gap: 8px;
    }

    .phase-header {
      flex-direction: column;
      align-items: stretch;
      gap: 8px;
    }

    .phase-meta {
      justify-content: space-between;
    }

    .connector-line {
      height: 40px;
    }

    .status-circle {
      width: 24px;
      height: 24px;
      font-size: var(--text-xs);
    }
  }
</style>