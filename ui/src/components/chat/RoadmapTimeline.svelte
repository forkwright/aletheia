<script lang="ts">
  interface Milestone {
    id: string;
    name: string;
    type: "builtin" | "phase";
    status: "pending" | "active" | "complete" | "failed";
    order: number;
    goal?: string;
    requirements?: string[];
    requirementCount?: number;
  }

  let {
    milestones,
    detailed = false,
    onMilestoneClick = () => {}
  }: {
    milestones: Milestone[];
    currentState: string;
    detailed?: boolean;
    onMilestoneClick?: (id: string) => void;
  } = $props();

  let selectedMilestone = $state<Milestone | null>(null);

  function getStatusIcon(status: string): string {
    switch (status) {
      case "complete": return "✓";
      case "active": return "⟳";
      case "failed": return "✗";
      case "pending": return "○";
      default: return "○";
    }
  }

  function getStatusColor(status: string): string {
    switch (status) {
      case "complete": return "status-complete";
      case "active": return "status-active";
      case "failed": return "status-failed";
      case "pending": return "status-pending";
      default: return "status-pending";
    }
  }

  function handleMilestoneClick(milestone: Milestone): void {
    if (detailed) {
      selectedMilestone = selectedMilestone?.id === milestone.id ? null : milestone;
    } else {
      onMilestoneClick(milestone.id);
    }
  }

  function getProgressPercentage(): number {
    if (milestones.length === 0) return 0;
    const completedCount = milestones.filter(m => m.status === "complete").length;
    const activeCount = milestones.filter(m => m.status === "active").length;
    return Math.round(((completedCount + (activeCount * 0.5)) / milestones.length) * 100);
  }
</script>

<div class="roadmap-timeline">
  <div class="timeline-header">
    <h4>Project Roadmap</h4>
    <div class="progress-bar">
      <div class="progress-fill" style="width: {getProgressPercentage()}%"></div>
      <span class="progress-text">{getProgressPercentage()}% Complete</span>
    </div>
  </div>

  <div class="timeline-container">
    <div class="timeline-track">
      {#each milestones as milestone, index (milestone.id)}
        <div class="milestone-group">
          <button 
            class="milestone {getStatusColor(milestone.status)}" 
            class:clickable={detailed || milestone.type === "phase"}
            onclick={() => handleMilestoneClick(milestone)}
            title={detailed ? "Click for details" : ""}
          >
            <div class="milestone-icon">
              {getStatusIcon(milestone.status)}
            </div>
            <div class="milestone-label">
              <span class="milestone-name">{milestone.name}</span>
              {#if milestone.requirementCount !== undefined && milestone.requirementCount > 0}
                <span class="requirement-count">({milestone.requirementCount} req)</span>
              {/if}
            </div>
          </button>
          
          {#if index < milestones.length - 1}
            <div class="connector" class:complete={milestone.status === "complete"}></div>
          {/if}
        </div>
      {/each}
    </div>

    {#if detailed && selectedMilestone}
      <div class="milestone-details">
        <div class="details-header">
          <h5>{selectedMilestone.name}</h5>
          <span class="status-badge {getStatusColor(selectedMilestone.status)}">
            {selectedMilestone.status.charAt(0).toUpperCase() + selectedMilestone.status.slice(1)}
          </span>
        </div>
        
        {#if selectedMilestone.goal}
          <div class="details-section">
            <h6>Goal</h6>
            <p>{selectedMilestone.goal}</p>
          </div>
        {/if}
        
        {#if selectedMilestone.requirements && selectedMilestone.requirements.length > 0}
          <div class="details-section">
            <h6>Requirements</h6>
            <ul class="requirements-list">
              {#each selectedMilestone.requirements as req}
                <li>{req}</li>
              {/each}
            </ul>
          </div>
        {/if}
        
        <button class="close-details" onclick={() => selectedMilestone = null}>
          Close Details
        </button>
      </div>
    {/if}
  </div>

  {#if !detailed}
    <div class="timeline-legend">
      <div class="legend-item">
        <span class="legend-icon status-complete">✓</span>
        <span>Complete</span>
      </div>
      <div class="legend-item">
        <span class="legend-icon status-active">⟳</span>
        <span>In Progress</span>
      </div>
      <div class="legend-item">
        <span class="legend-icon status-failed">✗</span>
        <span>Failed</span>
      </div>
      <div class="legend-item">
        <span class="legend-icon status-pending">○</span>
        <span>Pending</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .roadmap-timeline {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .timeline-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
  }

  .timeline-header h4 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .progress-bar {
    position: relative;
    width: 200px;
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

  .timeline-container {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .timeline-track {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .milestone-group {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }

  .milestone {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--text);
    text-align: left;
    width: 100%;
    max-width: 400px;
    transition: background var(--transition-quick), border-color var(--transition-quick), transform var(--transition-quick);
    position: relative;
  }

  .milestone.clickable {
    cursor: pointer;
  }

  .milestone.clickable:hover {
    background: var(--surface-hover);
    transform: translateY(-1px);
  }

  .milestone.status-complete {
    border-left: 4px solid var(--status-success);
  }

  .milestone.status-active {
    border-left: 4px solid var(--accent);
    background: rgba(154, 123, 79, 0.05);
  }

  .milestone.status-failed {
    border-left: 4px solid var(--status-error);
    background: rgba(var(--status-error-rgb), 0.05);
  }

  .milestone.status-pending {
    border-left: 4px solid var(--border);
  }

  .milestone-icon {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    font-size: var(--text-sm);
    font-weight: 600;
    flex-shrink: 0;
  }

  .status-complete .milestone-icon {
    background: var(--status-success-bg);
    color: var(--status-success);
  }

  .status-active .milestone-icon {
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
  }

  .status-failed .milestone-icon {
    background: var(--status-error-bg);
    color: var(--status-error);
  }

  .status-pending .milestone-icon {
    background: var(--bg-elevated);
    color: var(--text-muted);
  }

  .milestone-label {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .milestone-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text);
  }

  .requirement-count {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .connector {
    width: 2px;
    height: 20px;
    background: var(--border);
    margin: 4px 0 4px 27px;
    transition: background var(--transition-quick);
  }

  .connector.complete {
    background: var(--status-success);
  }

  .milestone-details {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 20px;
    margin-top: 16px;
  }

  .details-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .details-header h5 {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
  }

  .status-badge {
    padding: 4px 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .status-badge.status-complete {
    background: var(--status-success-bg);
    color: var(--status-success);
  }

  .status-badge.status-active {
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
  }

  .status-badge.status-failed {
    background: var(--status-error-bg);
    color: var(--status-error);
  }

  .status-badge.status-pending {
    background: var(--surface);
    color: var(--text-muted);
  }

  .details-section {
    margin-bottom: 16px;
  }

  .details-section h6 {
    margin: 0 0 8px 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-secondary);
  }

  .details-section p {
    margin: 0;
    color: var(--text);
    line-height: 1.5;
  }

  .requirements-list {
    margin: 0;
    padding-left: 20px;
    color: var(--text);
  }

  .requirements-list li {
    margin-bottom: 4px;
    line-height: 1.4;
  }

  .close-details {
    padding: 8px 16px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: var(--text-sm);
    transition: background var(--transition-quick);
  }

  .close-details:hover {
    background: var(--surface-hover);
  }

  .timeline-legend {
    display: flex;
    justify-content: center;
    gap: 24px;
    padding: 16px;
    background: var(--bg-elevated);
    border-radius: var(--radius-md);
    flex-wrap: wrap;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .legend-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    font-size: var(--text-xs);
    font-weight: 600;
  }

  @media (max-width: 768px) {
    .timeline-header {
      flex-direction: column;
      align-items: flex-start;
    }

    .progress-bar {
      width: 100%;
    }

    .milestone {
      max-width: none;
    }

    .timeline-legend {
      grid-template-columns: repeat(2, 1fr);
      gap: 12px;
    }

    .details-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }
  }
</style>