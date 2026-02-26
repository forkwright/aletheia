// Workspace file index — manifest builder, keyword scorer, agent-managed config
import { closeSync, type Dirent, existsSync, mkdirSync, openSync, readdirSync, readFileSync, readSync, statSync, writeFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { Buffer } from "node:buffer";
import { createLogger } from "../koina/logger.js";

const log = createLogger("workspace-indexer");

const INDEX_DIR = ".aletheia-index";
const MAX_DEPTH = 8;
const MAX_FILES = 2000;
const FIRST_LINE_BYTES = 256;
const FIRST_LINE_MAX_LEN = 120;
const STALE_REBUILD_RATIO = 0.10;

const SKIP_DIRS = new Set([".git", "node_modules", "dist", "__pycache__", ".aletheia-index"]);
const BINARY_EXTENSIONS = new Set([
  ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".pdf",
  ".zip", ".tar", ".gz", ".bz2", ".xz", ".7z", ".rar",
  ".wasm", ".so", ".dylib", ".dll", ".node",
  ".mp3", ".mp4", ".wav", ".ogg", ".mov", ".avi",
  ".ttf", ".otf", ".woff", ".woff2",
  ".sqlite", ".db",
]);

export interface FileEntry {
  path: string;    // relative to root
  mtime: number;
  firstLine: string;
}

export interface WorkspaceIndex {
  root: string;
  builtAt: number;
  files: FileEntry[];
}

export interface WorkspaceIndexConfig {
  extraPaths: string[];
}

function manifestKey(root: string): string {
  return Buffer.from(root).toString("base64url").replace(/[^a-zA-Z0-9_-]/g, "_");
}

function manifestPath(workspace: string, root: string): string {
  return join(workspace, INDEX_DIR, `manifest_${manifestKey(root)}.json`);
}

function configPath(workspace: string): string {
  return join(workspace, INDEX_DIR, "config.json");
}

function readFirstLine(filePath: string): string {
  let fd: number | undefined;
  try {
    fd = openSync(filePath, "r");
    const buf = Buffer.alloc(FIRST_LINE_BYTES);
    const bytesRead = readSync(fd, buf, 0, FIRST_LINE_BYTES, 0);
    const raw = buf.slice(0, bytesRead).toString("utf-8", 0, bytesRead);
    const nlIdx = raw.indexOf("\n");
    const line = (nlIdx === -1 ? raw : raw.slice(0, nlIdx)).trim();
    return line.slice(0, FIRST_LINE_MAX_LEN);
  } catch {
    return "";
  } finally {
    if (fd !== undefined) {
      try { closeSync(fd); } catch { /* ignore */ }
    }
  }
}

function scanDirectory(root: string, relBase: string, depth: number, entries: FileEntry[]): void {
  if (depth > MAX_DEPTH || entries.length >= MAX_FILES) return;

  let dirents: Dirent<string>[];
  try {
    dirents = readdirSync(join(root, relBase), { withFileTypes: true, encoding: "utf-8" });
  } catch {
    return;
  }

  for (const dirent of dirents) {
    if (entries.length >= MAX_FILES) break;
    const name = dirent.name;

    if (dirent.isDirectory()) {
      if (SKIP_DIRS.has(name)) continue;
      scanDirectory(root, relBase ? join(relBase, name) : name, depth + 1, entries);
    } else if (dirent.isFile()) {
      if (BINARY_EXTENSIONS.has(extname(name).toLowerCase())) continue;
      const relPath = relBase ? join(relBase, name) : name;
      const absPath = join(root, relPath);
      let mtime = 0;
      try {
        mtime = statSync(absPath).mtimeMs;
      } catch {
        continue;
      }
      const firstLine = readFirstLine(absPath);
      entries.push({ path: relPath, mtime, firstLine });
    }
  }
}

function buildIndex(root: string): WorkspaceIndex {
  const entries: FileEntry[] = [];
  scanDirectory(root, "", 0, entries);
  return { root, builtAt: Date.now(), files: entries };
}

function loadManifest(workspace: string, root: string): WorkspaceIndex | null {
  const path = manifestPath(workspace, root);
  if (!existsSync(path)) return null;
  try {
    const raw = readFileSync(path, "utf-8");
    return JSON.parse(raw) as WorkspaceIndex;
  } catch {
    return null;
  }
}

function saveManifest(workspace: string, index: WorkspaceIndex): void {
  const dir = join(workspace, INDEX_DIR);
  mkdirSync(dir, { recursive: true });
  writeFileSync(manifestPath(workspace, index.root), JSON.stringify(index), "utf-8");
}

function isStale(index: WorkspaceIndex): boolean {
  if (index.files.length === 0) return false;
  let staleCount = 0;
  for (const entry of index.files) {
    try {
      const absPath = join(index.root, entry.path);
      const mtime = statSync(absPath).mtimeMs;
      if (mtime !== entry.mtime) staleCount++;
    } catch {
      staleCount++;
    }
    if (staleCount / index.files.length > STALE_REBUILD_RATIO) return true;
  }
  return false;
}

export async function indexWorkspace(workspace: string, extraPaths?: string[]): Promise<WorkspaceIndex> {
  const roots = [workspace, ...(extraPaths ?? [])];
  const allFiles: FileEntry[] = [];

  for (const root of roots) {
    if (!existsSync(root)) continue;
    const existing = loadManifest(workspace, root);
    let index: WorkspaceIndex;

    if (existing && !isStale(existing)) {
      index = existing;
    } else {
      log.debug(`Building workspace index for ${root}`);
      index = buildIndex(root);
      saveManifest(workspace, index);
      log.debug(`Indexed ${index.files.length} files in ${root}`);
    }

    // Prefix extra-path entries with a relative base so paths are identifiable
    if (root !== workspace) {
      const prefix = relative(workspace, root);
      for (const f of index.files) {
        allFiles.push({ ...f, path: join(prefix, f.path) });
      }
    } else {
      allFiles.push(...index.files);
    }
  }

  return { root: workspace, builtAt: Date.now(), files: allFiles };
}

export async function rebuildWorkspaceIndex(workspace: string, extraPaths?: string[]): Promise<WorkspaceIndex> {
  const roots = [workspace, ...(extraPaths ?? [])];
  const allFiles: FileEntry[] = [];

  for (const root of roots) {
    if (!existsSync(root)) continue;
    const index = buildIndex(root);
    saveManifest(workspace, index);

    if (root !== workspace) {
      const prefix = relative(workspace, root);
      for (const f of index.files) {
        allFiles.push({ ...f, path: join(prefix, f.path) });
      }
    } else {
      allFiles.push(...index.files);
    }
  }

  log.info(`Rebuilt workspace index for ${workspace}: ${allFiles.length} files`);
  return { root: workspace, builtAt: Date.now(), files: allFiles };
}

export async function loadIndexConfig(workspace: string): Promise<WorkspaceIndexConfig> {
  const path = configPath(workspace);
  if (!existsSync(path)) return { extraPaths: [] };
  try {
    const raw = readFileSync(path, "utf-8");
    const parsed = JSON.parse(raw) as Partial<WorkspaceIndexConfig>;
    return { extraPaths: Array.isArray(parsed.extraPaths) ? parsed.extraPaths : [] };
  } catch {
    return { extraPaths: [] };
  }
}

export async function saveIndexConfig(workspace: string, config: WorkspaceIndexConfig): Promise<void> {
  const dir = join(workspace, INDEX_DIR);
  mkdirSync(dir, { recursive: true });
  writeFileSync(configPath(workspace), JSON.stringify(config, null, 2) + "\n", "utf-8");
}

export function queryIndex(index: WorkspaceIndex, query: string, limit: number): FileEntry[] {
  if (!query.trim()) return index.files.slice(0, limit);

  const tokens = query
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, " ")
    .split(/\s+/)
    .filter((t) => t.length >= 2);

  if (tokens.length === 0) return index.files.slice(0, limit);

  const scored = index.files.map((entry) => {
    const haystack = `${entry.path.toLowerCase()} ${entry.firstLine.toLowerCase()}`;
    let score = 0;
    for (const token of tokens) {
      if (haystack.includes(token)) score++;
    }
    return { entry, score };
  });

  return scored
    .filter((s) => s.score > 0)
    .toSorted((a, b) => b.score - a.score)
    .slice(0, limit)
    .map((s) => s.entry);
}
