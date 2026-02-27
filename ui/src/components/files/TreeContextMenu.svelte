<script lang="ts">
  import { onMount } from "svelte";
  import { saveWorkspaceFile, deleteWorkspaceFile, moveWorkspaceFile } from "../../lib/api";
  import { getActiveAgentId } from "../../stores/agents.svelte";
  import { loadTree } from "../../stores/files.svelte";

  interface ContextMenuProps {
    x: number;
    y: number;
    path: string;
    isDirectory: boolean;
    onClose: () => void;
  }

  let { x, y, path, isDirectory, onClose }: ContextMenuProps = $props();
  
  let menuElement: HTMLDivElement | undefined;
  let showRenameDialog = $state(false);
  let renameValue = $state("");
  let operating = $state(false);
  let error = $state<string | null>(null);
  
  onMount(() => {
    function handleClickOutside(event: MouseEvent) {
      if (menuElement && !menuElement.contains(event.target as Node)) {
        onClose();
      }
    }
    
    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        if (showRenameDialog) {
          showRenameDialog = false;
        } else {
          onClose();
        }
      }
    }
    
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleEscape);
    
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  });

  async function refreshTree() {
    const agentId = getActiveAgentId();
    await loadTree(agentId ?? undefined);
  }

  async function createNewFile() {
    const fileName = prompt("Enter file name:");
    if (!fileName) return;
    
    operating = true;
    error = null;
    
    try {
      const agentId = getActiveAgentId();
      const newPath = isDirectory ? `${path}/${fileName}` : `${path}/../${fileName}`;
      const normalizedPath = newPath.replace(/\/+/g, "/").replace(/\/\.\//g, "/");
      
      await saveWorkspaceFile(normalizedPath, "", agentId ?? undefined);
      await refreshTree();
      onClose();
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to create file";
    } finally {
      operating = false;
    }
  }

  async function createNewFolder() {
    const folderName = prompt("Enter folder name:");
    if (!folderName) return;
    
    operating = true;
    error = null;
    
    try {
      const agentId = getActiveAgentId();
      const folderPath = isDirectory ? `${path}/${folderName}` : `${path}/../${folderName}`;
      const normalizedPath = folderPath.replace(/\/+/g, "/").replace(/\/\.\//g, "/");
      
      // Create a temporary file in the folder, then delete it
      // This forces the directory creation through the existing API
      const tempFilePath = `${normalizedPath}/.gitkeep`;
      await saveWorkspaceFile(tempFilePath, "", agentId ?? undefined);
      
      await refreshTree();
      onClose();
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to create folder";
    } finally {
      operating = false;
    }
  }

  function startRename() {
    const fileName = path.split("/").pop() || "";
    renameValue = fileName;
    showRenameDialog = true;
  }

  async function confirmRename() {
    if (!renameValue.trim() || renameValue === (path.split("/").pop() || "")) {
      showRenameDialog = false;
      return;
    }
    
    operating = true;
    error = null;
    
    try {
      const agentId = getActiveAgentId();
      const parentPath = path.split("/").slice(0, -1).join("/");
      const newPath = parentPath ? `${parentPath}/${renameValue}` : renameValue;
      
      await moveWorkspaceFile(path, newPath, agentId ?? undefined);
      await refreshTree();
      showRenameDialog = false;
      onClose();
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to rename";
    } finally {
      operating = false;
    }
  }

  async function deleteItem() {
    const itemType = isDirectory ? "folder" : "file";
    const confirmed = confirm(`Are you sure you want to delete this ${itemType}?`);
    if (!confirmed) return;
    
    operating = true;
    error = null;
    
    try {
      const agentId = getActiveAgentId();
      await deleteWorkspaceFile(path, agentId ?? undefined);
      await refreshTree();
      onClose();
    } catch (err) {
      error = err instanceof Error ? err.message : `Failed to delete ${itemType}`;
    } finally {
      operating = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      confirmRename();
    }
  }
</script>

<div 
  class="context-menu" 
  bind:this={menuElement}
  style="left: {x}px; top: {y}px;"
  role="menu"
>
  {#if showRenameDialog}
    <div class="rename-dialog">
      <label for="rename-input">Rename:</label>
      <input
        id="rename-input"
        type="text"
        bind:value={renameValue}
        onkeydown={handleKeydown}
        placeholder="Enter new name"
        disabled={operating}
      />
      <div class="rename-actions">
        <button onclick={confirmRename} disabled={operating || !renameValue.trim()}>
          {operating ? "..." : "Rename"}
        </button>
        <button onclick={() => showRenameDialog = false} disabled={operating}>
          Cancel
        </button>
      </div>
    </div>
  {:else}
    <button class="menu-item" onclick={createNewFile} disabled={operating}>
      <span class="menu-icon">📄</span>
      New File
    </button>
    <button class="menu-item" onclick={createNewFolder} disabled={operating}>
      <span class="menu-icon">📁</span>
      New Folder
    </button>
    <div class="menu-separator"></div>
    <button class="menu-item" onclick={startRename} disabled={operating}>
      <span class="menu-icon">✏️</span>
      Rename
    </button>
    <button class="menu-item danger" onclick={deleteItem} disabled={operating}>
      <span class="menu-icon">🗑</span>
      Delete
    </button>
  {/if}
  
  {#if error}
    <div class="menu-error">{error}</div>
  {/if}
</div>

<style>
  .context-menu {
    position: fixed;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    padding: 4px 0;
    min-width: 160px;
    z-index: 1000;
    font-size: var(--text-sm);
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 12px;
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    text-align: left;
    transition: background-color 0.15s ease;
  }

  .menu-item:hover:not(:disabled) {
    background: var(--surface-hover);
  }

  .menu-item:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .menu-item.danger {
    color: var(--status-error);
  }

  .menu-item.danger:hover:not(:disabled) {
    background: rgba(var(--status-error-rgb), 0.1);
  }

  .menu-icon {
    font-size: var(--text-xs);
    width: 16px;
    text-align: center;
  }

  .menu-separator {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }

  .rename-dialog {
    padding: 12px;
  }

  .rename-dialog label {
    display: block;
    margin-bottom: 8px;
    font-weight: 600;
    color: var(--text);
  }

  .rename-dialog input {
    width: 100%;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 6px 8px;
    font-size: var(--text-sm);
    margin-bottom: 8px;
  }

  .rename-dialog input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .rename-actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  .rename-actions button {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 4px 12px;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .rename-actions button:hover:not(:disabled) {
    background: var(--surface-hover);
  }

  .rename-actions button:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .menu-error {
    padding: 6px 12px;
    color: var(--status-error);
    font-size: var(--text-xs);
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
</style>