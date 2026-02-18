import { fetchWorkspaceTree, fetchWorkspaceFile, fetchGitStatus } from "../lib/api";
import type { FileTreeEntry, GitFileStatus } from "../lib/types";

let treeEntries = $state<FileTreeEntry[]>([]);
let selectedPath = $state<string | null>(null);
let selectedContent = $state<string>("");
let loading = $state(false);
let fileLoading = $state(false);
let gitStatuses = $state<Map<string, string>>(new Map());
let expandedDirs = $state<Set<string>>(new Set());

export function getTreeEntries(): FileTreeEntry[] {
  return treeEntries;
}

export function getSelectedPath(): string | null {
  return selectedPath;
}

export function getSelectedContent(): string {
  return selectedContent;
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

export function getGitStatuses(): Map<string, string> {
  return gitStatuses;
}

export function isExpanded(path: string): boolean {
  return expandedDirs.has(path);
}

export function toggleDir(path: string): void {
  const next = new Set(expandedDirs);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expandedDirs = next;
}

export async function loadTree(agentId?: string): Promise<void> {
  loading = true;
  try {
    const data = await fetchWorkspaceTree(agentId, undefined, 3);
    treeEntries = data.entries;
    // Auto-expand root dirs
    expandedDirs = new Set(data.entries.filter(e => e.type === "directory").map(e => e.name));
  } catch {
    treeEntries = [];
  } finally {
    loading = false;
  }
}

export async function selectFile(path: string, agentId?: string): Promise<void> {
  selectedPath = path;
  selectedContent = "";
  fileLoading = true;
  try {
    const data = await fetchWorkspaceFile(path, agentId);
    selectedContent = data.content;
  } catch (err) {
    selectedContent = `Error: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    fileLoading = false;
  }
}

export async function loadGitStatus(agentId?: string): Promise<void> {
  try {
    const data = await fetchGitStatus(agentId);
    const map = new Map<string, string>();
    for (const f of data.files) {
      map.set(f.path, f.status);
    }
    gitStatuses = map;
  } catch {
    gitStatuses = new Map();
  }
}

export function clearFileSelection(): void {
  selectedPath = null;
  selectedContent = "";
}
