import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { fetchWorkspaceTree, fetchWorkspaceFile, fetchGitStatus } from "../lib/api";
import type { FileTreeEntry } from "../lib/types";

// --- Tree state ---
let treeEntries = $state<FileTreeEntry[]>([]);
let loading = $state(false);
let gitStatuses = $state(new SvelteMap<string, string>());
let expandedDirs = $state(new SvelteSet<string>());

// --- Tab state ---
export interface EditorTab {
  path: string;
  name: string;
  dirty: boolean;
  stale: boolean; // true when agent modified the file externally
}

let openTabs = $state<EditorTab[]>([]);
let activeTabPath = $state<string | null>(null);
let tabContentCache = new Map<string, string>(); // not reactive — raw cache for editor state
let fileLoading = $state(false);

// Legacy aliases — selectedPath/selectedContent now delegate to tab state
export function getSelectedPath(): string | null {
  return activeTabPath;
}

export function getSelectedContent(): string {
  if (!activeTabPath) return "";
  return tabContentCache.get(activeTabPath) ?? "";
}

// --- Tree accessors ---
export function getTreeEntries(): FileTreeEntry[] {
  return treeEntries;
}

export function isLoading(): boolean {
  return loading;
}

export function isFileLoading(): boolean {
  return fileLoading;
}

export function getGitStatus(path: string): string | undefined {
  return gitStatuses.get(path);
}

export function getGitStatuses(): SvelteMap<string, string> {
  return gitStatuses;
}

export function isExpanded(path: string): boolean {
  return expandedDirs.has(path);
}

export function toggleDir(path: string): void {
  const next = new SvelteSet(expandedDirs);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expandedDirs = next;
}

// --- Tab accessors ---
export function getOpenTabs(): EditorTab[] {
  return openTabs;
}

export function getActiveTabPath(): string | null {
  return activeTabPath;
}

export function openTab(path: string): void {
  if (!openTabs.find((t) => t.path === path)) {
    const name = path.split("/").pop() ?? path;
    openTabs = [...openTabs, { path, name, dirty: false, stale: false }];
  }
  activeTabPath = path;
}

export function closeTab(path: string): void {
  openTabs = openTabs.filter((t) => t.path !== path);
  tabContentCache.delete(path);
  if (activeTabPath === path) {
    activeTabPath = openTabs.at(-1)?.path ?? null;
  }
}

export function switchTab(path: string): void {
  if (openTabs.find((t) => t.path === path)) {
    activeTabPath = path;
  }
}

export function markTabDirty(path: string, dirty: boolean): void {
  openTabs = openTabs.map((t) => (t.path === path ? { ...t, dirty } : t));
}

export function markTabStale(path: string): void {
  openTabs = openTabs.map((t) => (t.path === path ? { ...t, stale: true } : t));
}

export function clearTabStale(path: string): void {
  openTabs = openTabs.map((t) => (t.path === path ? { ...t, stale: false } : t));
}

export function getTabContent(path: string): string | undefined {
  return tabContentCache.get(path);
}

export function setTabContent(path: string, content: string): void {
  tabContentCache.set(path, content);
}

export function hasAnyDirtyTab(): boolean {
  return openTabs.some((t) => t.dirty);
}

// --- File operations ---
export async function loadTree(agentId?: string): Promise<void> {
  loading = true;
  try {
    const data = await fetchWorkspaceTree(agentId, undefined, 3);
    treeEntries = data.entries;
    expandedDirs = new SvelteSet(data.entries.filter((e) => e.type === "directory").map((e) => e.name));
  } catch {
    treeEntries = [];
  } finally {
    loading = false;
  }
}

/** Open a file in a tab and load its content from the API */
export async function openFile(path: string, agentId?: string): Promise<string> {
  openTab(path);
  // If we already have cached content and tab isn't stale, return it
  if (tabContentCache.has(path) && !openTabs.find((t) => t.path === path)?.stale) {
    return tabContentCache.get(path)!;
  }
  fileLoading = true;
  try {
    const data = await fetchWorkspaceFile(path, agentId);
    tabContentCache.set(path, data.content);
    clearTabStale(path);
    return data.content;
  } catch (err) {
    const errMsg = `Error: ${err instanceof Error ? err.message : String(err)}`;
    tabContentCache.set(path, errMsg);
    return errMsg;
  } finally {
    fileLoading = false;
  }
}

/** Reload a file from the server (for stale tab refresh) */
export async function reloadFile(path: string, agentId?: string): Promise<string> {
  fileLoading = true;
  try {
    const data = await fetchWorkspaceFile(path, agentId);
    tabContentCache.set(path, data.content);
    clearTabStale(path);
    markTabDirty(path, false);
    return data.content;
  } catch (err) {
    const errMsg = `Error: ${err instanceof Error ? err.message : String(err)}`;
    return errMsg;
  } finally {
    fileLoading = false;
  }
}

// Legacy API — selectFile delegates to openFile
export async function selectFile(path: string, agentId?: string): Promise<void> {
  await openFile(path, agentId);
}

export async function loadGitStatus(agentId?: string): Promise<void> {
  try {
    const data = await fetchGitStatus(agentId);
    const map = new SvelteMap<string, string>();
    for (const f of data.files) {
      map.set(f.path, f.status);
    }
    gitStatuses = map;
  } catch {
    gitStatuses = new SvelteMap();
  }
}

export function clearFileSelection(): void {
  activeTabPath = null;
}

// --- Agent file edit notification ---
export function notifyFileEdit(agentName: string, filePath: string): void {
  // Mark tab stale if open
  const tab = openTabs.find((t) => t.path === filePath);
  if (tab) {
    markTabStale(filePath);
  }
  // Dispatch custom event for toast handling (consumed by Layout)
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent("aletheia:file-edit", {
        detail: { agentName, filePath },
      }),
    );
  }
}
