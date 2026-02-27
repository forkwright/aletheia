<script lang="ts">
  import {
    getOpenTabs,
    getActiveTabPath,
    switchTab,
    closeTab,
  } from "../../stores/files.svelte";

  let { onTabSelect }: { onTabSelect: (path: string) => void } = $props();

  function handleClose(e: MouseEvent, path: string) {
    e.stopPropagation();
    const tab = getOpenTabs().find((t) => t.path === path);
    if (tab?.dirty && !confirm(`Discard unsaved changes to ${tab.name}?`)) return;
    closeTab(path);
  }

  function handleMiddleClick(e: MouseEvent, path: string) {
    if (e.button === 1) {
      e.preventDefault();
      handleClose(e, path);
    }
  }

  function handleTabClick(path: string) {
    switchTab(path);
    onTabSelect(path);
  }
</script>

{#if getOpenTabs().length > 0}
  <div class="tab-bar" role="tablist">
    {#each getOpenTabs() as tab (tab.path)}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div
        class="tab"
        class:active={getActiveTabPath() === tab.path}
        class:dirty={tab.dirty}
        class:stale={tab.stale}
        role="tab"
        aria-selected={getActiveTabPath() === tab.path}
        title={tab.path}
        onclick={() => handleTabClick(tab.path)}
        onauxclick={(e) => handleMiddleClick(e, tab.path)}
      >
        <span class="tab-name">{tab.name}</span>
        {#if tab.dirty}
          <span class="tab-dot dirty-dot" title="Unsaved changes"></span>
        {/if}
        {#if tab.stale}
          <span class="tab-dot stale-dot" title="Modified externally by agent"></span>
        {/if}
        <button
          class="tab-close"
          onclick={(e) => handleClose(e, tab.path)}
          aria-label="Close {tab.name}"
        >×</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .tab-bar {
    display: flex;
    align-items: stretch;
    gap: 0;
    overflow-x: auto;
    overflow-y: hidden;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
    min-height: 30px;
    scrollbar-width: thin;
  }
  .tab-bar::-webkit-scrollbar {
    height: 3px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px 4px 10px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: color 0.15s, border-color 0.15s;
  }
  .tab:hover {
    color: var(--text-secondary);
    background: var(--surface-hover);
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .tab-name {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dirty-dot {
    background: var(--status-warning);
  }
  .stale-dot {
    background: var(--status-info);
  }
  .tab-close {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    padding: 0 2px;
    line-height: 1;
    cursor: pointer;
    border-radius: var(--radius-sm);
    opacity: 0;
    transition: opacity 0.1s;
  }
  .tab:hover .tab-close {
    opacity: 1;
  }
  .tab-close:hover {
    color: var(--text);
    background: var(--surface);
  }
</style>
