<script lang="ts">
  import { authFetch } from "./api";

  let { projectId, targetType, targetId, targetLabel = "" }: {
    projectId: string;
    targetType: "requirement" | "phase" | "project" | "discussion";
    targetId: string;
    targetLabel?: string;
  } = $props();

  interface Annotation {
    id: string;
    author: string;
    content: string;
    resolved: boolean;
    createdAt: string;
    updatedAt: string;
  }

  let annotations = $state<Annotation[]>([]);
  let newContent = $state("");
  let loading = $state(false);
  let showResolved = $state(false);

  async function fetchAnnotations() {
    try {
      const url = `/api/planning/projects/${projectId}/annotations?targetType=${targetType}&targetId=${encodeURIComponent(targetId)}&includeResolved=${showResolved}`;
      const res = await authFetch(url);
      if (res.ok) {
        const data = await res.json() as { annotations: Annotation[] };
        annotations = data.annotations;
      }
    } catch { /* best effort */ }
  }

  async function addAnnotation() {
    const content = newContent.trim();
    if (!content) return;
    loading = true;
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/annotations`, {
        method: "POST",
        body: JSON.stringify({ targetType, targetId, content }),
      });
      if (res.ok) {
        const created = await res.json() as Annotation;
        annotations = [created, ...annotations];
        newContent = "";
      }
    } catch { /* best effort */ }
    loading = false;
  }

  async function toggleResolved(ann: Annotation) {
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/annotations/${ann.id}`, {
        method: "PATCH",
        body: JSON.stringify({ resolved: !ann.resolved }),
      });
      if (res.ok) {
        const updated = await res.json() as Annotation;
        annotations = annotations.map(a => a.id === updated.id ? updated : a);
      }
    } catch { /* best effort */ }
  }

  async function deleteAnnotation(id: string) {
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/annotations/${id}`, {
        method: "DELETE",
      });
      if (res.ok) {
        annotations = annotations.filter(a => a.id !== id);
      }
    } catch { /* best effort */ }
  }

  function timeAgo(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      addAnnotation();
    }
  }

  $effect(() => {
    // Refetch when target or resolved filter changes
    void targetId;
    void showResolved;
    fetchAnnotations();
  });
</script>

<div class="annotation-panel">
  <div class="panel-header">
    <span class="panel-title">💬 Notes{targetLabel ? ` — ${targetLabel}` : ""}</span>
    <label class="resolved-toggle">
      <input type="checkbox" bind:checked={showResolved} />
      Show resolved
    </label>
  </div>

  <div class="compose">
    <textarea
      bind:value={newContent}
      placeholder="Add a note..."
      rows="2"
      onkeydown={handleKeydown}
    ></textarea>
    <button class="add-btn" onclick={addAnnotation} disabled={loading || !newContent.trim()}>
      {loading ? "..." : "Add"}
    </button>
  </div>

  {#if annotations.length === 0}
    <div class="empty">No notes yet</div>
  {:else}
    <div class="annotation-list">
      {#each annotations as ann (ann.id)}
        <div class="annotation" class:resolved={ann.resolved}>
          <div class="ann-header">
            <span class="ann-author">{ann.author}</span>
            <span class="ann-time">{timeAgo(ann.createdAt)}</span>
          </div>
          <div class="ann-content">{ann.content}</div>
          <div class="ann-actions">
            <button class="action-btn" onclick={() => toggleResolved(ann)}>
              {ann.resolved ? "↩ Reopen" : "✓ Resolve"}
            </button>
            <button class="action-btn delete" onclick={() => deleteAnnotation(ann.id)}>
              ✕
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .annotation-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    background: var(--bg-secondary, #1a1a2e);
    border-radius: 6px;
    border: 1px solid var(--border, #333);
    max-height: 300px;
    overflow-y: auto;
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .panel-title {
    font-weight: 600;
    font-size: 0.85rem;
    color: var(--text-primary, #e0e0e0);
  }

  .resolved-toggle {
    font-size: 0.75rem;
    color: var(--text-secondary, #999);
    display: flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
  }

  .resolved-toggle input {
    cursor: pointer;
  }

  .compose {
    display: flex;
    gap: 6px;
    align-items: flex-end;
  }

  .compose textarea {
    flex: 1;
    resize: vertical;
    background: var(--bg-tertiary, #0f0f23);
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    color: var(--text-primary, #e0e0e0);
    padding: 6px 8px;
    font-size: 0.8rem;
    font-family: inherit;
  }

  .compose textarea:focus {
    outline: none;
    border-color: var(--accent, #6c63ff);
  }

  .add-btn {
    background: var(--accent, #6c63ff);
    color: white;
    border: none;
    border-radius: 4px;
    padding: 6px 12px;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .add-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .empty {
    color: var(--text-secondary, #666);
    font-size: 0.8rem;
    text-align: center;
    padding: 8px;
  }

  .annotation-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .annotation {
    background: var(--bg-tertiary, #0f0f23);
    border-radius: 4px;
    padding: 8px;
    border-left: 3px solid var(--accent, #6c63ff);
  }

  .annotation.resolved {
    opacity: 0.6;
    border-left-color: var(--text-secondary, #666);
  }

  .ann-header {
    display: flex;
    justify-content: space-between;
    margin-bottom: 4px;
  }

  .ann-author {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--accent, #6c63ff);
  }

  .ann-time {
    font-size: 0.7rem;
    color: var(--text-secondary, #666);
  }

  .ann-content {
    font-size: 0.8rem;
    color: var(--text-primary, #e0e0e0);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .ann-actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }

  .action-btn {
    background: none;
    border: none;
    color: var(--text-secondary, #999);
    font-size: 0.7rem;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: 3px;
  }

  .action-btn:hover {
    color: var(--text-primary, #e0e0e0);
    background: var(--bg-secondary, #1a1a2e);
  }

  .action-btn.delete:hover {
    color: #ff4444;
  }
</style>
