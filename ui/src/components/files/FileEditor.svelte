<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { FileTreeEntry } from "../../lib/types";
  import { saveWorkspaceFile, fetchWorkspaceFile } from "../../lib/api";
  import {
    getTreeEntries,
    isLoading,
    isExpanded,
    toggleDir,
    loadTree,
    loadGitStatus,
    getGitStatus,
  } from "../../stores/files.svelte";
  import { getActiveAgentId } from "../../stores/agents.svelte";
  import Spinner from "../shared/Spinner.svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState, type Extension } from "@codemirror/state";
  import { keymap } from "@codemirror/view";
  import { javascript } from "@codemirror/lang-javascript";
  import { markdown } from "@codemirror/lang-markdown";
  import { json } from "@codemirror/lang-json";
  import { css } from "@codemirror/lang-css";
  import { html } from "@codemirror/lang-html";
  import { python } from "@codemirror/lang-python";
  import { yaml } from "@codemirror/lang-yaml";
  import { oneDark } from "@codemirror/theme-one-dark";

  let { onClose }: { onClose: () => void } = $props();

  let currentPath = $state<string | null>(null);
  let isDirty = $state(false);
  let saving = $state(false);
  let fileLoading = $state(false);
  let saveError = $state<string | null>(null);
  let treeVisible = $state(true);
  let filterText = $state("");
  let showPreview = $state(false);
  let previewHtml = $state("");

  let editorContainer: HTMLDivElement;
  let editorView: EditorView | null = null;
  let originalContent = "";

  onMount(() => {
    const agentId = getActiveAgentId();
    loadTree(agentId ?? undefined);
    loadGitStatus(agentId ?? undefined);

    const handler = (e: BeforeUnloadEvent) => {
      if (isDirty) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  });

  onDestroy(() => {
    editorView?.destroy();
  });

  function resolveLanguage(path: string): Extension {
    const ext = path.split(".").pop()?.toLowerCase();
    switch (ext) {
      case "ts": case "tsx": return javascript({ typescript: true, jsx: ext.endsWith("x") });
      case "js": case "jsx": case "mjs": return javascript({ jsx: ext.endsWith("x") });
      case "md": return markdown();
      case "json": return json();
      case "yaml": case "yml": return yaml();
      case "py": return python();
      case "css": return css();
      case "html": case "svelte": return html();
      default: return [];
    }
  }

  function initEditor(content: string, path: string) {
    editorView?.destroy();
    isDirty = false;
    saveError = null;

    const lang = resolveLanguage(path);
    const extensions: Extension[] = [
      basicSetup,
      lang,
      oneDark,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          isDirty = true;
          if (showPreview && isMarkdown(path)) {
            updatePreview();
          }
        }
      }),
      keymap.of([{
        key: "Mod-s",
        run: () => { save(); return true; },
      }]),
      EditorView.theme({
        "&": { height: "100%", fontSize: "13px" },
        ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-mono)" },
        ".cm-content": { padding: "8px 0" },
      }),
    ];

    const state = EditorState.create({ doc: content, extensions });
    editorView = new EditorView({ state, parent: editorContainer });
  }

  function isMarkdown(path: string): boolean {
    return path.endsWith(".md");
  }

  function updatePreview() {
    if (!editorView) return;
    const text = editorView.state.doc.toString();
    // Simple markdown → HTML (basic rendering for preview)
    previewHtml = text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/^### (.+)$/gm, "<h3>$1</h3>")
      .replace(/^## (.+)$/gm, "<h2>$1</h2>")
      .replace(/^# (.+)$/gm, "<h1>$1</h1>")
      .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
      .replace(/\*(.+?)\*/g, "<em>$1</em>")
      .replace(/`(.+?)`/g, "<code>$1</code>")
      .replace(/^- (.+)$/gm, "<li>$1</li>")
      .replace(/\n\n/g, "<br><br>")
      .replace(/\n/g, "<br>");
  }

  async function openFile(path: string) {
    if (isDirty && !confirm("Discard unsaved changes?")) return;

    fileLoading = true;
    currentPath = path;
    showPreview = false;
    try {
      const agentId = getActiveAgentId();
      const data = await fetchWorkspaceFile(path, agentId ?? undefined);
      originalContent = data.content;
      if (editorContainer) {
        initEditor(data.content, path);
      }
    } catch (err) {
      saveError = `Failed to load: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      fileLoading = false;
    }
  }

  async function save() {
    if (!currentPath || !editorView || saving) return;
    saving = true;
    saveError = null;
    try {
      const content = editorView.state.doc.toString();
      const agentId = getActiveAgentId();
      await saveWorkspaceFile(currentPath, content, agentId ?? undefined);
      originalContent = content;
      isDirty = false;
    } catch (err) {
      saveError = err instanceof Error ? err.message : String(err);
    } finally {
      saving = false;
    }
  }

  function fileIcon(name: string): string {
    if (name.endsWith(".ts") || name.endsWith(".tsx")) return "TS";
    if (name.endsWith(".js") || name.endsWith(".jsx")) return "JS";
    if (name.endsWith(".svelte")) return "Sv";
    if (name.endsWith(".css")) return "Cs";
    if (name.endsWith(".json")) return "{}";
    if (name.endsWith(".md")) return "Md";
    if (name.endsWith(".py")) return "Py";
    if (name.endsWith(".yaml") || name.endsWith(".yml")) return "Ym";
    return "··";
  }

  function matchesFilter(entry: FileTreeEntry, path: string): boolean {
    if (!filterText) return true;
    const lower = filterText.toLowerCase();
    if (entry.name.toLowerCase().includes(lower)) return true;
    if (path.toLowerCase().includes(lower)) return true;
    if (entry.type === "directory" && entry.children) {
      return entry.children.some((child) => matchesFilter(child, `${path}/${child.name}`));
    }
    return false;
  }

  function gitStatusClass(status: string): string {
    if (status.includes("M")) return "modified";
    if (status.includes("A") || status === "??") return "added";
    if (status.includes("D")) return "deleted";
    return "";
  }
</script>

<div class="file-editor">
  {#if treeVisible}
    <div class="editor-tree">
      <div class="tree-header">
        <span class="tree-title">Files</span>
        <button class="icon-btn" onclick={() => {
          const agentId = getActiveAgentId();
          loadTree(agentId ?? undefined);
          loadGitStatus(agentId ?? undefined);
        }} title="Refresh">↻</button>
        <button class="icon-btn" onclick={() => treeVisible = false} title="Hide tree">◀</button>
      </div>
      <div class="tree-filter">
        <input type="text" placeholder="Filter..." bind:value={filterText} />
      </div>
      <div class="tree-list">
        {#if isLoading()}
          <div class="tree-msg"><Spinner size={14} /> Loading...</div>
        {:else if getTreeEntries().length === 0}
          <div class="tree-msg">No files</div>
        {:else}
          {#each getTreeEntries() as entry (entry.name)}
            {#if matchesFilter(entry, entry.name)}
              {@render treeNode(entry, entry.name, 0)}
            {/if}
          {/each}
        {/if}
      </div>
    </div>
  {/if}
  <div class="editor-main">
    <div class="editor-toolbar">
      {#if !treeVisible}
        <button class="icon-btn" onclick={() => treeVisible = true} title="Show tree">▶</button>
      {/if}
      {#if currentPath}
        <span class="file-path">{currentPath}</span>
        {#if isDirty}
          <span class="dirty-dot" title="Unsaved changes"></span>
        {/if}
        {#if isMarkdown(currentPath)}
          <button
            class="toolbar-btn"
            class:active={showPreview}
            onclick={() => { showPreview = !showPreview; if (showPreview) updatePreview(); }}
          >Preview</button>
        {/if}
        <button class="toolbar-btn save-btn" onclick={save} disabled={!isDirty || saving}>
          {saving ? "Saving..." : "Save"}
        </button>
      {:else}
        <span class="file-path placeholder">No file open</span>
      {/if}
      <button class="icon-btn close-btn" onclick={onClose} title="Close editor">×</button>
    </div>
    {#if saveError}
      <div class="save-error">{saveError}</div>
    {/if}
    <div class="editor-content">
      {#if fileLoading}
        <div class="editor-loading"><Spinner size={16} /> Loading...</div>
      {:else if showPreview && currentPath && isMarkdown(currentPath)}
        <div class="preview-pane">{@html previewHtml}</div>
      {:else if currentPath}
        <div class="cm-wrapper" bind:this={editorContainer}></div>
      {:else}
        <div class="editor-empty">Select a file from the tree to edit</div>
      {/if}
    </div>
  </div>
</div>

{#snippet treeNode(entry: FileTreeEntry, path: string, depth: number)}
  {#if entry.type === "directory"}
    <button
      class="tree-item dir"
      style="padding-left: {8 + depth * 14}px"
      onclick={() => toggleDir(path)}
    >
      <span class="tree-chevron">{isExpanded(path) ? "▾" : "▸"}</span>
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
    {@const gs = getGitStatus(path)}
    <button
      class="tree-item file"
      class:active={currentPath === path}
      class:modified={gs ? gitStatusClass(gs) === "modified" : false}
      class:added={gs ? gitStatusClass(gs) === "added" : false}
      style="padding-left: {8 + depth * 14}px"
      onclick={() => openFile(path)}
    >
      <span class="tree-ficon">{fileIcon(entry.name)}</span>
      <span class="tree-name">{entry.name}</span>
    </button>
  {/if}
{/snippet}

<style>
  .file-editor {
    display: flex;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
  }

  /* Tree panel */
  .editor-tree {
    width: 200px;
    min-width: 160px;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--bg-elevated);
  }
  .tree-header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .tree-title {
    font-size: 12px;
    font-weight: 600;
    flex: 1;
  }
  .tree-filter {
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
  }
  .tree-filter input {
    width: 100%;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text);
    padding: 3px 6px;
    font-size: 11px;
    font-family: var(--font-sans);
  }
  .tree-filter input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .tree-list {
    flex: 1;
    overflow-y: auto;
    padding: 2px 0;
  }
  .tree-msg {
    padding: 12px;
    color: var(--text-muted);
    font-size: 11px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tree-item {
    display: flex;
    align-items: center;
    gap: 3px;
    width: 100%;
    padding: 2px 8px;
    background: none;
    border: none;
    color: var(--text);
    font-size: 11px;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
  }
  .tree-item:hover { background: var(--surface-hover); }
  .tree-item.active { background: var(--surface); color: var(--accent); }
  .tree-item.modified .tree-name { color: var(--yellow); }
  .tree-item.added .tree-name { color: var(--green); }
  .tree-chevron {
    width: 10px;
    font-size: 9px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .tree-ficon {
    width: 16px;
    font-size: 8px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    flex-shrink: 0;
    text-align: center;
  }
  .tree-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tree-item.dir .tree-name { font-weight: 500; }

  /* Main editor area */
  .editor-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .editor-toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    min-height: 32px;
  }
  .file-path {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .file-path.placeholder { color: var(--text-muted); }
  .dirty-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yellow);
    flex-shrink: 0;
  }
  .icon-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    padding: 2px 4px;
    border-radius: 3px;
    cursor: pointer;
    line-height: 1;
  }
  .icon-btn:hover { color: var(--text); background: var(--surface-hover); }
  .close-btn { margin-left: auto; font-size: 18px; }
  .toolbar-btn {
    background: none;
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 2px 8px;
    border-radius: 3px;
    font-size: 11px;
    cursor: pointer;
  }
  .toolbar-btn:hover { color: var(--text); background: var(--surface); }
  .toolbar-btn.active { color: var(--accent); border-color: var(--accent); }
  .toolbar-btn:disabled { opacity: 0.4; cursor: default; }
  .save-btn:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .save-btn:not(:disabled):hover {
    background: var(--accent);
    color: #fff;
  }
  .save-error {
    padding: 4px 8px;
    background: rgba(248, 81, 73, 0.1);
    border-bottom: 1px solid var(--red);
    color: var(--red);
    font-size: 12px;
  }
  .editor-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
  }
  .editor-loading, .editor-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }
  .cm-wrapper {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  .cm-wrapper :global(.cm-editor) {
    height: 100%;
  }
  .preview-pane {
    flex: 1;
    overflow: auto;
    padding: 16px 24px;
    font-size: 14px;
    line-height: 1.6;
    color: var(--text);
  }
  .preview-pane :global(h1) { font-size: 24px; margin: 16px 0 8px; }
  .preview-pane :global(h2) { font-size: 20px; margin: 14px 0 6px; }
  .preview-pane :global(h3) { font-size: 16px; margin: 12px 0 4px; }
  .preview-pane :global(code) {
    background: var(--surface);
    padding: 1px 4px;
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .preview-pane :global(strong) { color: var(--text); }
  .preview-pane :global(li) {
    margin-left: 20px;
    list-style: disc;
  }
</style>
