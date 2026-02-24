<script lang="ts">
  import { timeAgo } from "../../lib/utils";

  interface Project {
    id: string;
    goal: string;
    state: string;
    config: {
      name: string;
      description: string;
      scope?: string;
    };
    createdAt: string;
    updatedAt: string;
  }

  let { project, stateLabel, stateColor, onRefresh, onClose }: {
    project: Project;
    stateLabel: string;
    stateColor: string;
    onRefresh: () => void;
    onClose?: () => void;
  } = $props();

  let refreshing = $state(false);

  async function handleRefresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      await onRefresh();
    } finally {
      refreshing = false;
    }
  }
</script>

<div class="project-header">
  <div class="header-content">
    <div class="project-info">
      <h1 class="project-name">{project.config.name}</h1>
      <p class="project-goal">{project.goal}</p>
      {#if project.config.description && project.config.description !== project.goal}
        <p class="project-description">{project.config.description}</p>
      {/if}
    </div>

    <div class="project-status">
      <div class="status-badge" style="--status-color: {stateColor}">
        <div class="status-indicator"></div>
        <span class="status-label">{stateLabel}</span>
      </div>
      <div class="header-actions">
        <button 
          class="refresh-btn" 
          class:refreshing 
          onclick={handleRefresh}
          title="Refresh project data"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path 
              d="M13.65 2.35C12.18 0.88 10.21 0 8 0C3.58 0 0 3.58 0 8s3.58 8 8 8c3.73 0 6.84-2.55 7.73-6h-2.08C12.78 12.04 10.66 14 8 14c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14 0.69 4.22 1.78L9 7h7V0l-2.35 2.35z" 
              fill="currentColor"
            />
          </svg>
        </button>
        {#if onClose}
          <button 
            class="close-btn" 
            onclick={onClose}
            title="Close planning dashboard"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <path 
                d="M12.854 4.854a.5.5 0 0 0-.708-.708L8 8.293 3.854 4.146a.5.5 0 1 0-.708.708L7.293 9l-4.147 4.146a.5.5 0 0 0 .708.708L8 9.707l4.146 4.147a.5.5 0 0 0 .708-.708L8.707 9l4.147-4.146z" 
                fill="currentColor"
              />
            </svg>
          </button>
        {/if}
      </div>
    </div>
  </div>

  <div class="project-meta">
    <span class="meta-item">
      <span class="meta-label">Created:</span>
      <span class="meta-value">{timeAgo(new Date(project.createdAt))}</span>
    </span>
    <span class="meta-item">
      <span class="meta-label">Updated:</span>
      <span class="meta-value">{timeAgo(new Date(project.updatedAt))}</span>
    </span>
    <span class="meta-item">
      <span class="meta-label">ID:</span>
      <span class="meta-value">{project.id.slice(0, 8)}...</span>
    </span>
  </div>

  {#if project.config.scope}
    <details class="scope-details">
      <summary>Scope</summary>
      <div class="scope-content">{project.config.scope}</div>
    </details>
  {/if}
</div>

<style>
  .project-header {
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
    padding: var(--space-5) var(--space-4);
    flex-shrink: 0;
  }

  .header-content {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-3);
  }

  .project-info {
    flex: 1;
    min-width: 0;
  }

  .project-name {
    font-size: var(--text-2xl);
    font-weight: 600;
    color: var(--text);
    margin: 0 0 var(--space-2) 0;
    line-height: 1.2;
  }

  .project-goal {
    font-size: var(--text-lg);
    color: var(--text-secondary);
    margin: 0 0 var(--space-1) 0;
    line-height: 1.4;
  }

  .project-description {
    font-size: var(--text-base);
    color: var(--text-muted);
    margin: 0;
    line-height: 1.4;
  }

  .project-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .status-badge {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--status-color, var(--text-muted));
    animation: pulse 2s ease infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .status-label {
    color: var(--text);
    white-space: nowrap;
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .refresh-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
    border-color: var(--accent);
  }

  .refresh-btn.refreshing {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .project-meta {
    display: flex;
    gap: var(--space-4);
    font-size: var(--text-xs);
  }

  .meta-item {
    display: flex;
    gap: var(--space-1);
  }

  .meta-label {
    color: var(--text-muted);
    font-weight: 500;
  }

  .meta-value {
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .scope-details {
    margin-top: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .scope-details summary {
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    transition: background var(--transition-quick);
  }

  .scope-details summary:hover {
    background: var(--surface-hover);
  }

  .scope-details[open] summary {
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    border-bottom: 1px solid var(--border);
  }

  .scope-content {
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .refresh-btn,
  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .refresh-btn:hover,
  .close-btn:hover {
    background: var(--surface-hover);
    border-color: var(--border-hover);
    color: var(--text);
  }

  .refresh-btn.refreshing {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  @media (max-width: 768px) {
    .project-header {
      padding: var(--space-4) var(--space-3);
    }

    .header-content {
      flex-direction: column;
      gap: var(--space-3);
    }

    .project-status {
      align-self: flex-start;
    }

    .project-meta {
      flex-direction: column;
      gap: var(--space-1);
    }

    .project-name {
      font-size: var(--text-xl);
    }

    .project-goal {
      font-size: var(--text-base);
    }
  }
</style>