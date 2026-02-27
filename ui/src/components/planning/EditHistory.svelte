<script lang="ts">
  import { authFetch } from "./api";

  let { projectId, targetType, targetId }: {
    projectId: string;
    targetType?: string;
    targetId?: string;
  } = $props();

  interface Edit {
    id: string;
    targetType: string;
    targetId: string;
    field: string;
    oldValue: string | null;
    newValue: string | null;
    author: string;
    createdAt: string;
  }

  let edits = $state<Edit[]>([]);
  let expanded = $state(false);

  async function fetchHistory() {
    try {
      let url = `/api/planning/projects/${projectId}/history?limit=50`;
      if (targetType) url += `&targetType=${targetType}`;
      if (targetId) url += `&targetId=${encodeURIComponent(targetId)}`;
      const res = await authFetch(url);
      if (res.ok) {
        const data = await res.json() as { edits: Edit[] };
        edits = data.edits;
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

  function truncate(val: string | null, len = 40): string {
    if (!val) return "—";
    return val.length > len ? val.slice(0, len) + "…" : val;
  }

  function authorIcon(author: string): string {
    if (author === "user") return "👤";
    if (author === "agent" || author === "system") return "🤖";
    return "📝";
  }

  $effect(() => {
    if (expanded) {
      void projectId;
      void targetId;
      fetchHistory();
    }
  });
</script>

<div class="edit-history">
  <button class="toggle-btn" onclick={() => { expanded = !expanded; }}>
    <span class="icon">{expanded ? "▾" : "▸"}</span>
    <span>Change History</span>
    {#if edits.length > 0}
      <span class="count">{edits.length}</span>
    {/if}
  </button>

  {#if expanded}
    {#if edits.length === 0}
      <div class="empty">No changes recorded</div>
    {:else}
      <div class="edit-list">
        {#each edits as edit (edit.id)}
          <div class="edit-row">
            <span class="edit-icon">{authorIcon(edit.author)}</span>
            <div class="edit-detail">
              <span class="edit-author">{edit.author}</span>
              changed <span class="field-name">{edit.field}</span>
              {#if edit.targetType !== targetType || edit.targetId !== targetId}
                on <span class="target-ref">{edit.targetType}:{truncate(edit.targetId, 12)}</span>
              {/if}
            </div>
            <div class="edit-values">
              {#if edit.oldValue}
                <span class="old-val">{truncate(edit.oldValue)}</span>
                <span class="arrow">→</span>
              {/if}
              <span class="new-val">{truncate(edit.newValue)}</span>
            </div>
            <span class="edit-time">{timeAgo(edit.createdAt)}</span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .edit-history {
    border: 1px solid var(--border, #333);
    border-radius: 6px;
    background: var(--bg-secondary, #1a1a2e);
    overflow: hidden;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary, #999);
    font-size: 0.8rem;
    cursor: pointer;
    text-align: left;
  }

  .toggle-btn:hover {
    color: var(--text-primary, #e0e0e0);
    background: var(--bg-tertiary, #0f0f23);
  }

  .icon {
    font-size: 0.7rem;
  }

  .count {
    background: var(--accent, #6c63ff);
    color: white;
    font-size: 0.65rem;
    padding: 1px 5px;
    border-radius: 8px;
    margin-left: auto;
  }

  .empty {
    color: var(--text-secondary, #666);
    font-size: 0.8rem;
    text-align: center;
    padding: 12px;
  }

  .edit-list {
    max-height: 240px;
    overflow-y: auto;
    border-top: 1px solid var(--border, #333);
  }

  .edit-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border, #222);
    font-size: 0.75rem;
  }

  .edit-row:last-child {
    border-bottom: none;
  }

  .edit-icon {
    font-size: 0.8rem;
    flex-shrink: 0;
  }

  .edit-detail {
    color: var(--text-secondary, #999);
    flex: 1;
    min-width: 0;
  }

  .edit-author {
    font-weight: 600;
    color: var(--text-primary, #e0e0e0);
  }

  .field-name {
    color: var(--accent, #6c63ff);
    font-family: monospace;
    font-size: 0.7rem;
  }

  .target-ref {
    color: var(--text-secondary, #666);
    font-family: monospace;
    font-size: 0.7rem;
  }

  .edit-values {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.7rem;
    flex-shrink: 0;
    max-width: 200px;
  }

  .old-val {
    color: #ff6b6b;
    text-decoration: line-through;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .arrow {
    color: var(--text-secondary, #666);
    flex-shrink: 0;
  }

  .new-val {
    color: #51cf66;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .edit-time {
    color: var(--text-secondary, #666);
    font-size: 0.65rem;
    flex-shrink: 0;
    white-space: nowrap;
  }
</style>
