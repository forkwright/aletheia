<script lang="ts">
  import {
    getTasks, getDailyTasks, getTaskCounts, isTasksLoading, getTasksError,
    loadTasks, loadDailyTasks, createTask, updateTask, completeTask, deleteTask,
    subscribeTaskEvents,
    type Task,
  } from "../../stores/tasks.svelte";
  import Spinner from "../shared/Spinner.svelte";

  let { projectId }: { projectId?: string } = $props();

  // View mode: project tasks or daily
  let viewMode = $state<"project" | "daily">("project");
  let showCompleted = $state(false);

  // Quick-add state
  let showAddForm = $state(false);
  let newTitle = $state("");
  let newPriority = $state<Task["priority"]>("medium");
  let adding = $state(false);

  // Derived state from store
  let tasks = $derived(viewMode === "daily" ? getDailyTasks() : getTasks());
  let counts = $derived(getTaskCounts());
  let loading = $derived(isTasksLoading());
  let error = $derived(getTasksError());

  let filteredTasks = $derived.by(() => {
    if (showCompleted) return tasks;
    return tasks.filter(t => t.status !== "done" && t.status !== "skipped");
  });

  let total = $derived(Object.values(counts).reduce((a, b) => a + b, 0));
  let doneCount = $derived(counts.done + counts.skipped);
  let progressPercent = $derived(total > 0 ? Math.round((doneCount / total) * 100) : 0);

  // Load on mount
  $effect(() => {
    if (viewMode === "daily") {
      loadDailyTasks();
    } else if (projectId) {
      loadTasks(projectId);
    } else {
      loadTasks();
    }
  });

  // Subscribe to SSE events
  $effect(() => {
    const unsub = subscribeTaskEvents();
    return unsub;
  });

  // Helpers
  function priorityColor(p: Task["priority"]): string {
    switch (p) {
      case "critical": return "var(--status-error)";
      case "high": return "var(--status-warning)";
      case "medium": return "var(--text-muted)";
      case "low": return "var(--text-dim, var(--text-muted))";
    }
  }

  function statusIcon(s: Task["status"]): string {
    switch (s) {
      case "done": return "✅";
      case "active": return "🔄";
      case "blocked": return "⛔";
      case "failed": return "❌";
      case "skipped": return "⏭️";
      case "pending": return "⏳";
    }
  }

  async function handleAdd() {
    if (!newTitle.trim() || adding) return;
    adding = true;
    try {
      await createTask({
        title: newTitle.trim(),
        priority: newPriority,
        ...(viewMode === "project" && projectId ? { projectId } : {}),
      });
      newTitle = "";
      newPriority = "medium";
      showAddForm = false;
    } finally {
      adding = false;
    }
  }

  async function handleComplete(taskId: string) {
    await completeTask(taskId);
  }

  async function handleDelete(taskId: string, title: string) {
    if (!confirm(`Delete task "${title}"?`)) return;
    await deleteTask(taskId);
  }

  async function handleStatusChange(taskId: string, newStatus: Task["status"]) {
    await updateTask(taskId, { status: newStatus });
  }
</script>

<div class="task-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">📝</span>
      Tasks
      {#if total > 0}
        <span class="task-progress">{doneCount}/{total} ({progressPercent}%)</span>
      {/if}
    </h2>
    <div class="header-actions">
      <div class="view-toggle">
        <button
          class="toggle-btn"
          class:active={viewMode === "project"}
          onclick={() => viewMode = "project"}
        >Project</button>
        <button
          class="toggle-btn"
          class:active={viewMode === "daily"}
          onclick={() => viewMode = "daily"}
        >Daily</button>
      </div>
      <button class="add-btn" onclick={() => showAddForm = !showAddForm}>
        {showAddForm ? "✕" : "+"}
      </button>
    </div>
  </div>

  <!-- Quick Add Form -->
  {#if showAddForm}
    <div class="add-form">
      <input
        type="text"
        class="add-input"
        placeholder="Task title..."
        bind:value={newTitle}
        onkeydown={(e) => { if (e.key === "Enter") handleAdd(); if (e.key === "Escape") showAddForm = false; }}
      />
      <div class="add-row">
        <select class="priority-select" bind:value={newPriority}>
          <option value="critical">🔴 Critical</option>
          <option value="high">🟠 High</option>
          <option value="medium">⚪ Medium</option>
          <option value="low">🔵 Low</option>
        </select>
        <button class="add-submit" onclick={handleAdd} disabled={adding || !newTitle.trim()}>
          {adding ? "…" : "Add"}
        </button>
      </div>
    </div>
  {/if}

  <!-- Progress Bar -->
  {#if total > 0}
    <div class="progress-bar">
      <div class="progress-fill" style="width: {progressPercent}%"></div>
    </div>
  {/if}

  <!-- Filter -->
  <div class="filter-row">
    <label class="show-completed">
      <input type="checkbox" bind:checked={showCompleted} />
      <span>Show completed ({counts.done + counts.skipped})</span>
    </label>
    {#if counts.blocked > 0}
      <span class="blocked-badge">⛔ {counts.blocked} blocked</span>
    {/if}
  </div>

  <!-- Task List -->
  <div class="task-list-container">
    {#if loading && tasks.length === 0}
      <div class="loading-state">
        <Spinner size={20} />
        <span>Loading tasks...</span>
      </div>
    {:else if error}
      <div class="error-state">
        <span>⚠️ {error}</span>
      </div>
    {:else if filteredTasks.length === 0}
      <div class="empty-state">
        <span class="empty-icon">📋</span>
        <span>{showCompleted ? "No tasks yet" : "All tasks complete!"}</span>
      </div>
    {:else}
      <div class="task-list">
        {#each filteredTasks as task (task.id)}
          <div
            class="task-row"
            class:done={task.status === "done" || task.status === "skipped"}
            class:blocked={task.status === "blocked"}
            style="padding-left: calc(var(--space-3) + {task.depth * 16}px)"
          >
            <!-- Complete checkbox -->
            {#if task.status !== "done" && task.status !== "skipped"}
              <button
                class="complete-btn"
                onclick={() => handleComplete(task.id)}
                title="Complete task"
              >○</button>
            {:else}
              <span class="complete-icon">✓</span>
            {/if}

            <div class="task-content">
              <div class="task-header">
                <span class="task-id">{task.taskId}</span>
                <span class="task-title">{task.title}</span>
                <span class="priority-dot" style="background: {priorityColor(task.priority)}" title={task.priority}></span>
              </div>
              {#if task.description}
                <div class="task-description">{task.description}</div>
              {/if}
              {#if task.blockedBy.length > 0}
                <div class="task-deps">⛔ Blocked by: {task.blockedBy.join(", ")}</div>
              {/if}
            </div>

            <div class="task-actions">
              <select
                class="status-select"
                value={task.status}
                onclick={(e) => e.stopPropagation()}
                onchange={(e) => handleStatusChange(task.id, (e.currentTarget as HTMLSelectElement).value as Task["status"])}
              >
                <option value="pending">{statusIcon("pending")} Pending</option>
                <option value="active">{statusIcon("active")} Active</option>
                <option value="blocked">{statusIcon("blocked")} Blocked</option>
                <option value="done">{statusIcon("done")} Done</option>
                <option value="skipped">{statusIcon("skipped")} Skipped</option>
                <option value="failed">{statusIcon("failed")} Failed</option>
              </select>
              <button
                class="delete-btn"
                onclick={() => handleDelete(task.id, task.title)}
                title="Delete"
              >🗑</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .task-section {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-3);
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
    margin: 0;
  }

  .title-icon { font-size: var(--text-xl); }

  .task-progress {
    color: var(--text-muted);
    font-weight: 400;
    font-size: var(--text-sm);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .view-toggle {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .toggle-btn {
    background: var(--surface);
    border: none;
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-muted);
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .toggle-btn.active {
    background: var(--accent);
    color: white;
  }

  .toggle-btn:not(.active):hover {
    background: var(--surface-hover);
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-lg);
    font-weight: 600;
    cursor: pointer;
    line-height: 1;
  }

  .add-btn:hover { background: var(--accent-hover); }

  .add-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    margin-bottom: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
  }

  .add-input {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }

  .add-input:focus { outline: none; border-color: var(--accent); }

  .add-row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .priority-select {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    flex: 1;
  }

  .add-submit {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
  }

  .add-submit:disabled { opacity: 0.5; cursor: not-allowed; }

  .progress-bar {
    height: 4px;
    background: var(--surface);
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: var(--space-2);
  }

  .progress-fill {
    height: 100%;
    background: var(--status-success);
    transition: width 0.3s ease;
    border-radius: 2px;
  }

  .filter-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
    font-size: var(--text-xs);
  }

  .show-completed {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--text-muted);
    cursor: pointer;
  }

  .show-completed input { margin: 0; }

  .blocked-badge {
    color: var(--status-error);
    font-weight: 600;
  }

  .task-list-container {
    flex: 1;
    overflow-y: auto;
  }

  .loading-state, .error-state, .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-6);
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .empty-icon { font-size: var(--text-xl); }

  .task-list {
    display: flex;
    flex-direction: column;
  }

  .task-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    transition: background var(--transition-quick);
  }

  .task-row:hover {
    background: var(--surface);
  }

  .task-row.done {
    opacity: 0.5;
  }

  .task-row.blocked {
    background: color-mix(in srgb, var(--status-error) 5%, transparent);
  }

  .complete-btn {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border);
    border-radius: 50%;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: all var(--transition-quick);
  }

  .complete-btn:hover {
    border-color: var(--status-success);
    color: var(--status-success);
  }

  .complete-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--status-success);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }

  .task-content {
    flex: 1;
    min-width: 0;
  }

  .task-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .task-id {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    color: var(--text-muted);
    white-space: nowrap;
  }

  .task-title {
    font-size: var(--text-sm);
    color: var(--text);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .task-row.done .task-title {
    text-decoration: line-through;
  }

  .priority-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .task-description {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .task-deps {
    font-size: var(--text-2xs);
    color: var(--status-error);
    margin-top: 2px;
  }

  .task-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
    opacity: 0;
    transition: opacity var(--transition-quick);
  }

  .task-row:hover .task-actions {
    opacity: 1;
  }

  .status-select {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 2px var(--space-1);
    font-size: var(--text-2xs);
    cursor: pointer;
  }

  .delete-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--text-2xs);
    transition: all var(--transition-quick);
  }

  .delete-btn:hover {
    background: color-mix(in srgb, var(--status-error) 10%, transparent);
    border-color: var(--status-error);
    color: var(--status-error);
  }

  @media (max-width: 768px) {
    .task-actions { opacity: 1; }
    .view-toggle { display: none; }
  }
</style>
