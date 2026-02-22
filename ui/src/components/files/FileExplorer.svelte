<script lang="ts">
  import type { FileTreeEntry } from "../../lib/types";
  import { highlightCode, inferLanguageFromPath } from "../../lib/markdown";
  import DOMPurify from "dompurify";
  import Spinner from "../shared/Spinner.svelte";
  import {
    getTreeEntries,
    getSelectedPath,
    getSelectedContent,
    isLoading,
    isFileLoading,
    getGitStatus,
    getGitStatuses,
    isExpanded,
    toggleDir,
    loadTree,
    selectFile,
    loadGitStatus,
    clearFileSelection,
  } from "../../stores/files.svelte";
  import { getActiveAgentId } from "../../stores/agents.svelte";
  import { onMount } from "svelte";

  let filterText = $state("");

  onMount(() => {
    const agentId = getActiveAgentId();
    loadTree(agentId ?? undefined);
    loadGitStatus(agentId ?? undefined);
  });

  function fileIcon(name: string): string {
    if (name.endsWith(".ts") || name.endsWith(".tsx")) return "TS";
    if (name.endsWith(".js") || name.endsWith(".jsx")) return "JS";
    if (name.endsWith(".svelte")) return "Sv";
    if (name.endsWith(".css")) return "Cs";
    if (name.endsWith(".json")) return "{}";
    if (name.endsWith(".md")) return "Md";
    if (name.endsWith(".sql")) return "SQ";
    if (name.endsWith(".py")) return "Py";
    if (name.endsWith(".sh")) return "Sh";
    if (name.endsWith(".yaml") || name.endsWith(".yml")) return "Ym";
    return "··";
  }

  function gitStatusClass(status: string): string {
    if (status.includes("M")) return "modified";
    if (status.includes("A") || status === "??") return "added";
    if (status.includes("D")) return "deleted";
    return "";
  }

  function matchesFilter(entry: FileTreeEntry, path: string): boolean {
    if (!filterText) return true;
    const lower = filterText.toLowerCase();
    if (entry.name.toLowerCase().includes(lower)) return true;
    if (path.toLowerCase().includes(lower)) return true;
    if (entry.type === "directory" && entry.children) {
      return entry.children.some(child => matchesFilter(child, `${path}/${child.name}`));
    }
    return false;
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
  }

  function handleFileClick(path: string) {
    const agentId = getActiveAgentId();
    selectFile(path, agentId ?? undefined);
  }

  function highlightFileContent(content: string, path: string): string {
    const lang = inferLanguageFromPath(path);
    if (lang) {
      return DOMPurify.sanitize(highlightCode(content, lang), { ADD_ATTR: ["class"] });
    }
    return content
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }
</script>

<div class="file-explorer">
  <div class="explorer-sidebar">
    <div class="explorer-header">
      <span class="explorer-title">Files</span>
      <button class="refresh-btn" onclick={() => {
        const agentId = getActiveAgentId();
        loadTree(agentId ?? undefined);
        loadGitStatus(agentId ?? undefined);
      }} title="Refresh" aria-label="Refresh file tree">↻</button>
    </div>
    <div class="filter-row">
      <input
        type="text"
        class="filter-input"
        placeholder="Filter files..."
        bind:value={filterText}
      />
    </div>
    <div class="tree-container">
      {#if isLoading()}
        <div class="tree-loading"><Spinner size={16} /> Loading...</div>
      {:else if getTreeEntries().length === 0}
        <div class="tree-empty">No files found</div>
      {:else}
        {#each getTreeEntries() as entry (entry.name)}
          {#if matchesFilter(entry, entry.name)}
            {@render treeNode(entry, entry.name, 0)}
          {/if}
        {/each}
      {/if}
    </div>
  </div>
  <div class="file-preview">
    {#if isFileLoading()}
      <div class="preview-loading"><Spinner size={16} /> Loading file...</div>
    {:else if getSelectedPath()}
      <div class="preview-header">
        <span class="preview-path">{getSelectedPath()}</span>
        <button class="close-btn" onclick={clearFileSelection} aria-label="Close file preview">×</button>
      </div>
      <pre class="preview-content">{@html highlightFileContent(getSelectedContent(), getSelectedPath()!)}</pre>
    {:else}
      <div class="preview-empty">Select a file to preview</div>
    {/if}
  </div>
</div>

{#snippet treeNode(entry: FileTreeEntry, path: string, depth: number)}
  {#if entry.type === "directory"}
    <button
      class="tree-item dir"
      style="padding-left: {12 + depth * 16}px"
      onclick={() => toggleDir(path)}
    >
      <span class="tree-icon">{isExpanded(path) ? "▾" : "▸"}</span>
      <span class="tree-name">{entry.name}</span>
    </button>
    {#if isExpanded(path) && entry.children}
      {#each entry.children as child (child.name)}
        {#if matchesFilter(child, `${path}/${child.name}`)}
          {@render treeNode(child, `${path}/${child.name}`, depth + 1)}
        {/if}
      {/each}
    {/if}
  {:else}
    {@const gitStatus = getGitStatus(path)}
    <button
      class="tree-item file"
      class:selected={getSelectedPath() === path}
      class:modified={gitStatus ? gitStatusClass(gitStatus) === "modified" : false}
      class:added={gitStatus ? gitStatusClass(gitStatus) === "added" : false}
      class:deleted={gitStatus ? gitStatusClass(gitStatus) === "deleted" : false}
      style="padding-left: {12 + depth * 16}px"
      onclick={() => handleFileClick(path)}
    >
      <span class="tree-file-icon">{fileIcon(entry.name)}</span>
      <span class="tree-name">{entry.name}</span>
      {#if entry.size != null}
        <span class="tree-size">{formatSize(entry.size)}</span>
      {/if}
      {#if gitStatus}
        <span class="git-indicator" class:modified={gitStatusClass(gitStatus) === "modified"} class:added={gitStatusClass(gitStatus) === "added"} class:deleted={gitStatusClass(gitStatus) === "deleted"}></span>
      {/if}
    </button>
  {/if}
{/snippet}

<style>
  .file-explorer {
    display: flex;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .explorer-sidebar {
    width: 280px;
    min-width: 200px;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
  }
  .explorer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .explorer-title {
    font-size: 13px;
    font-weight: 600;
  }
  .refresh-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 16px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }
  .refresh-btn:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .filter-row {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .filter-input {
    width: 100%;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 4px 8px;
    font-size: 12px;
    font-family: var(--font-sans);
  }
  .filter-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .tree-container {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }
  .tree-loading, .tree-empty {
    padding: 16px;
    color: var(--text-muted);
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .tree-item {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 3px 12px;
    background: none;
    border: none;
    color: var(--text);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
  }
  .tree-item:hover {
    background: var(--surface-hover);
  }
  .tree-item.selected {
    background: var(--surface);
    color: var(--accent);
  }
  .tree-icon {
    width: 12px;
    font-size: 10px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .tree-file-icon {
    width: 18px;
    font-size: 9px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    flex-shrink: 0;
    text-align: center;
  }
  .tree-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tree-item.dir .tree-name {
    font-weight: 500;
  }
  .tree-size {
    margin-left: auto;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .git-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    margin-left: 4px;
  }
  .git-indicator.modified { background: var(--yellow); }
  .git-indicator.added { background: var(--green); }
  .git-indicator.deleted { background: var(--red); }
  .tree-item.modified .tree-name { color: var(--yellow); }
  .tree-item.added .tree-name { color: var(--green); }
  .tree-item.deleted .tree-name { color: var(--red); }

  .file-preview {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }
  .preview-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
  }
  .preview-path {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    line-height: 1;
  }
  .close-btn:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .preview-content {
    flex: 1;
    overflow: auto;
    margin: 0;
    padding: 12px 16px;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text-secondary);
    background: var(--bg);
  }
  .preview-loading, .preview-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }

  /* hljs tokens in file preview */
  .preview-content :global(.hljs-keyword) { color: #ff7b72; }
  .preview-content :global(.hljs-string),
  .preview-content :global(.hljs-regexp) { color: #a5d6ff; }
  .preview-content :global(.hljs-number) { color: #79c0ff; }
  .preview-content :global(.hljs-comment) { color: #8b949e; }
  .preview-content :global(.hljs-built_in) { color: #ffa657; }
  .preview-content :global(.hljs-function),
  .preview-content :global(.hljs-title) { color: #d2a8ff; }
  .preview-content :global(.hljs-property) { color: #79c0ff; }
  .preview-content :global(.hljs-tag) { color: #7ee787; }
  .preview-content :global(.hljs-name) { color: #7ee787; }
  .preview-content :global(.hljs-attr) { color: #79c0ff; }

  @media (max-width: 768px) {
    .explorer-sidebar {
      width: 100%;
      max-height: 40vh;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .file-explorer {
      flex-direction: column;
    }
  }
</style>
