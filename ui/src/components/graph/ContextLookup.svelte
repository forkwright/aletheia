<script lang="ts">
  import type { EntityDetail } from "../../lib/types";

  let {
    searchQuery,
    searchResults,
    searchLoading,
    entityDetail,
    entityLoading,
    onSearch,
    onNodeClick,
    onClose,
  }: {
    searchQuery: string;
    searchResults: Array<{ id: string; labels: string[]; pagerank: number; community: number }>;
    searchLoading: boolean;
    entityDetail: EntityDetail | null;
    entityLoading: boolean;
    onSearch: (q: string) => void;
    onNodeClick: (id: string) => void;
    onClose: () => void;
  } = $props();

  let query = $state(searchQuery);
  let showDetail = $state(false);

  function handleSearch() {
    if (query.trim()) {
      onSearch(query.trim());
      showDetail = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleSearch();
    if (e.key === "Escape") onClose();
  }

  function selectEntity(id: string) {
    onNodeClick(id);
    showDetail = true;
  }
</script>

<div class="context-lookup">
  <div class="lookup-header">
    <span class="lookup-title">🔎 Context Lookup</span>
    <button class="close-btn" onclick={onClose}>&times;</button>
  </div>

  <div class="lookup-search">
    <input
      type="text"
      class="lookup-input"
      placeholder="What does the system know about…"
      bind:value={query}
      onkeydown={handleKeydown}
      autofocus
    />
    <button class="search-btn" onclick={handleSearch} disabled={searchLoading}>
      {searchLoading ? "…" : "→"}
    </button>
  </div>

  {#if searchResults.length > 0}
    <div class="results-list">
      <div class="results-header">{searchResults.length} entities found</div>
      {#each searchResults.slice(0, 15) as result}
        <button class="result-item" onclick={() => selectEntity(result.id)}>
          <span class="result-name">{result.id}</span>
          <div class="result-meta">
            {#if result.labels.length > 0}
              {#each result.labels.slice(0, 2) as label}
                <span class="result-label">{label}</span>
              {/each}
            {/if}
            <span class="result-pr">PR {result.pagerank.toFixed(4)}</span>
          </div>
        </button>
      {/each}
    </div>
  {/if}

  {#if showDetail && entityDetail && !entityLoading}
    <div class="detail-preview">
      <h4 class="detail-name">{entityDetail.name}</h4>
      <div class="detail-stats">
        <span>{entityDetail.relationship_count} connections</span>
        <span>·</span>
        <span>{entityDetail.memories.length} memories</span>
        <span>·</span>
        <span class="confidence-indicator">
          {entityDetail.confidence === "high" ? "🟢" : entityDetail.confidence === "medium" ? "🟡" : "🔴"}
          {entityDetail.confidence}
        </span>
      </div>
      {#if entityDetail.memories.length > 0}
        <div class="memory-preview">
          {#each entityDetail.memories.slice(0, 3) as mem}
            <div class="preview-memory">
              <span class="preview-score">{(mem.score * 100).toFixed(0)}%</span>
              <span class="preview-text">{mem.text}</span>
            </div>
          {/each}
          {#if entityDetail.memories.length > 3}
            <div class="more-indicator">+{entityDetail.memories.length - 3} more memories</div>
          {/if}
        </div>
      {/if}
      {#if entityDetail.relationships.length > 0}
        <div class="rel-preview">
          {#each entityDetail.relationships.slice(0, 5) as rel}
            <span class="rel-chip">
              {rel.direction === "out" ? "→" : "←"} {rel.type} {rel.target}
            </span>
          {/each}
        </div>
      {/if}
    </div>
  {:else if showDetail && entityLoading}
    <div class="detail-loading">Loading entity details…</div>
  {/if}
</div>

<style>
  .context-lookup {
    position: absolute;
    top: 60px;
    left: 12px;
    width: 340px;
    max-height: calc(100% - 80px);
    background: var(--bg-elevated, #161b22);
    border: 1px solid var(--border, #30363d);
    border-radius: var(--radius, 8px);
    z-index: 25;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .lookup-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border, #30363d);
  }

  .lookup-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text, #e6edf3);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted, #484f58);
    font-size: 16px;
    cursor: pointer;
    padding: 0 2px;
    line-height: 1;
  }
  .close-btn:hover { color: var(--text, #e6edf3); }

  .lookup-search {
    display: flex;
    gap: 4px;
    padding: 8px;
    border-bottom: 1px solid var(--border, #30363d);
  }

  .lookup-input {
    flex: 1;
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    border-radius: 4px;
    color: var(--text, #e6edf3);
    padding: 5px 8px;
    font-size: 12px;
    font-family: var(--font-sans, system-ui);
  }
  .lookup-input:focus { outline: none; border-color: var(--accent, #c9a84c); }

  .search-btn {
    background: var(--accent, #c9a84c);
    border: none;
    color: #0f1114;
    padding: 5px 10px;
    border-radius: 4px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .search-btn:disabled { opacity: 0.5; }

  .results-list {
    flex: 1;
    overflow-y: auto;
    max-height: 200px;
  }

  .results-header {
    padding: 4px 8px;
    font-size: 10px;
    color: var(--text-muted, #484f58);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .result-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border, #30363d);
    padding: 6px 8px;
    cursor: pointer;
    text-align: left;
    color: var(--text, #e6edf3);
  }
  .result-item:hover { background: var(--surface, #21262d); }

  .result-name {
    font-size: 12px;
    font-weight: 600;
  }

  .result-meta {
    display: flex;
    gap: 4px;
    align-items: center;
    font-size: 10px;
    color: var(--text-muted, #484f58);
  }

  .result-label {
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    padding: 0 4px;
    border-radius: 3px;
    font-size: 9px;
  }

  .result-pr {
    font-family: var(--font-mono, monospace);
  }

  .detail-preview {
    border-top: 1px solid var(--accent, #c9a84c);
    padding: 8px;
    overflow-y: auto;
    max-height: 300px;
  }

  .detail-name {
    font-size: 13px;
    margin: 0 0 4px;
    color: var(--text, #e6edf3);
  }

  .detail-stats {
    display: flex;
    gap: 6px;
    font-size: 11px;
    color: var(--text-secondary, #8b949e);
    margin-bottom: 8px;
  }

  .confidence-indicator {
    font-size: 11px;
  }

  .memory-preview {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 8px;
  }

  .preview-memory {
    display: flex;
    gap: 6px;
    align-items: flex-start;
  }

  .preview-score {
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    color: var(--text-muted, #484f58);
    min-width: 28px;
    text-align: right;
    flex-shrink: 0;
  }

  .preview-text {
    font-size: 11px;
    color: var(--text-secondary, #8b949e);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .more-indicator {
    font-size: 10px;
    color: var(--text-muted, #484f58);
    text-align: center;
    padding: 2px;
  }

  .rel-preview {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }

  .rel-chip {
    font-size: 10px;
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    padding: 1px 5px;
    border-radius: 3px;
    color: var(--text-secondary, #8b949e);
  }

  .detail-loading {
    text-align: center;
    padding: 12px;
    color: var(--text-muted, #484f58);
    font-size: 11px;
  }

  @media (max-width: 768px) {
    .context-lookup {
      width: calc(100% - 24px);
      left: 12px;
    }
  }
</style>
