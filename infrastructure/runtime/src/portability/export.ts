// Agent export — produce a portable AgentFile from a running nous
//
// Captures: config, workspace files, session history, working state,
// agent notes, distillation priming, and optionally memory vectors + graph.
//
// Design: Spec 21 Phase 1 (Agent Portability)

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { createLogger } from "../koina/logger.js";
import type { DistillationPriming, Session, SessionStore, WorkingState } from "../mneme/store.js";
import { paths } from "../taxis/paths.js";

const log = createLogger("portability:export");

// --- AgentFile Schema ---

export interface AgentFile {
  version: 1;
  exportedAt: string;
  generator: string;
  nous: {
    id: string;
    name: string | null;
    model: string | null;
    config: Record<string, unknown>;
  };
  workspace: {
    files: Record<string, string>;    // path → content (text files only)
    binaryFiles: string[];            // paths of binary files (not included)
  };
  sessions: ExportedSession[];
  memory?: {
    vectors?: ExportedVector[];
    graph?: {
      nodes: ExportedGraphNode[];
      edges: ExportedGraphEdge[];
    };
  };
}

export interface ExportedSession {
  id: string;
  sessionKey: string;
  status: Session["status"];
  sessionType: Session["sessionType"];
  messageCount: number;
  tokenCountEstimate: number;
  distillationCount: number;
  createdAt: string;
  updatedAt: string;
  workingState: WorkingState | null;
  distillationPriming: DistillationPriming | null;
  notes: ExportedNote[];
  messages: ExportedMessage[];
}

export interface ExportedMessage {
  role: string;
  content: string;
  seq: number;
  tokenEstimate: number;
  isDistilled: boolean;
  createdAt: string;
}

export interface ExportedNote {
  category: string;
  content: string;
  createdAt: string;
}

export interface ExportedVector {
  id: string;
  text: string;
  metadata: Record<string, unknown>;
  // Embeddings omitted — can be regenerated on import
}

export interface ExportedGraphNode {
  name: string;
  labels: string[];
  properties: Record<string, unknown>;
}

export interface ExportedGraphEdge {
  source: string;
  target: string;
  relType: string;
}

// --- Export Options ---

export interface ExportOptions {
  /** Include memory vectors from Qdrant */
  includeMemory?: boolean;
  /** Include knowledge graph from Neo4j */
  includeGraph?: boolean;
  /** Max messages per session (0 = all). Default: 500 */
  maxMessagesPerSession?: number;
  /** Include archived sessions. Default: false */
  includeArchived?: boolean;
  /** Memory sidecar URL for vector/graph export */
  sidecarUrl?: string;
}

const DEFAULT_OPTIONS: Required<ExportOptions> = {
  includeMemory: false,
  includeGraph: false,
  maxMessagesPerSession: 500,
  includeArchived: false,
  sidecarUrl: "http://localhost:8230",
};

// --- Text file detection ---

const TEXT_EXTENSIONS = new Set([
  ".md", ".txt", ".yaml", ".yml", ".json", ".ts", ".js", ".py",
  ".sh", ".bash", ".zsh", ".css", ".html", ".svelte", ".toml",
  ".ini", ".cfg", ".conf", ".env", ".log", ".csv", ".xml",
  ".gitignore", ".editorconfig", ".prettierrc",
]);

const BINARY_EXTENSIONS = new Set([
  ".png", ".jpg", ".jpeg", ".gif", ".ico", ".svg", ".webp",
  ".woff", ".woff2", ".ttf", ".eot",
  ".zip", ".tar", ".gz", ".bz2", ".xz",
  ".pdf", ".doc", ".docx", ".xlsx",
  ".db", ".sqlite", ".sqlite3",
  ".wasm", ".so", ".dylib",
]);

function isTextFile(filename: string): boolean {
  const ext = extname(filename).toLowerCase();
  if (TEXT_EXTENSIONS.has(ext)) return true;
  if (BINARY_EXTENSIONS.has(ext)) return false;
  // No extension or unknown — check if no extension (likely text: Makefile, Dockerfile, etc.)
  if (!ext) return true;
  return false; // Default to binary for unknown extensions
}

// --- Workspace scanning ---

const IGNORE_DIRS = new Set(["node_modules", ".git", "__pycache__", ".cache", "dist"]);
const MAX_FILE_SIZE = 1024 * 1024; // 1MB — skip files larger than this

function scanWorkspace(
  workspacePath: string,
): { files: Record<string, string>; binaryFiles: string[] } {
  const files: Record<string, string> = {};
  const binaryFiles: string[] = [];

  function walk(dir: string): void {
    let entries: string[];
    try {
      entries = readdirSync(dir);
    } catch {
      return;
    }

    for (const entry of entries) {
      if (entry.startsWith(".") && entry !== ".env") continue;
      if (IGNORE_DIRS.has(entry)) continue;

      const fullPath = join(dir, entry);
      let stat;
      try {
        stat = statSync(fullPath);
      } catch {
        continue;
      }

      if (stat.isDirectory()) {
        walk(fullPath);
        continue;
      }

      if (!stat.isFile()) continue;

      const relPath = relative(workspacePath, fullPath);

      if (stat.size > MAX_FILE_SIZE) {
        binaryFiles.push(relPath);
        continue;
      }

      if (isTextFile(entry)) {
        try {
          files[relPath] = readFileSync(fullPath, "utf-8");
        } catch {
          binaryFiles.push(relPath);
        }
      } else {
        binaryFiles.push(relPath);
      }
    }
  }

  walk(workspacePath);
  return { files, binaryFiles };
}

// --- Session export ---

function exportSession(
  store: SessionStore,
  session: Session,
  _nousId: string,
  maxMessages: number,
): ExportedSession {
  // Get messages — most recent N if limited
  const limit = maxMessages > 0 ? maxMessages : 0;
  const messages = store.getHistory(session.id, limit > 0 ? { limit } : {});

  // Get notes for this session
  const notes = store.getNotes(session.id, { limit: 100 });

  return {
    id: session.id,
    sessionKey: session.sessionKey,
    status: session.status,
    sessionType: session.sessionType,
    messageCount: session.messageCount,
    tokenCountEstimate: session.tokenCountEstimate,
    distillationCount: session.distillationCount,
    createdAt: session.createdAt,
    updatedAt: session.updatedAt,
    workingState: session.workingState,
    distillationPriming: session.distillationPriming,
    notes: notes.map((n) => ({
      category: n.category,
      content: n.content,
      createdAt: n.createdAt,
    })),
    messages: messages.map((m) => ({
      role: m.role,
      content: m.content,
      seq: m.seq,
      tokenEstimate: m.tokenEstimate,
      isDistilled: m.isDistilled,
      createdAt: m.createdAt,
    })),
  };
}

// --- Memory export (via sidecar HTTP) ---

async function exportMemoryVectors(
  sidecarUrl: string,
  nousId: string,
): Promise<ExportedVector[]> {
  try {
    const response = await fetch(
      `${sidecarUrl}/memories?agent_id=${encodeURIComponent(nousId)}&limit=10000`,
    );
    if (!response.ok) {
      log.warn(`Memory export failed: ${response.status} ${response.statusText}`);
      return [];
    }
    const data = (await response.json()) as { ok: boolean; memories?: Array<Record<string, unknown>> };
    if (!data.ok || !data.memories) return [];

    return data.memories.map((m) => ({
      id: String(m["id"] ?? ""),
      text: String(m["memory"] ?? m["text"] ?? ""),
      metadata: (m["metadata"] as Record<string, unknown>) ?? {},
    }));
  } catch (err) {
    log.warn(`Memory sidecar unreachable: ${err instanceof Error ? err.message : err}`);
    return [];
  }
}

async function exportGraph(
  sidecarUrl: string,
): Promise<{ nodes: ExportedGraphNode[]; edges: ExportedGraphEdge[] } | null> {
  try {
    const response = await fetch(`${sidecarUrl}/graph/export?mode=all`);
    if (!response.ok) {
      log.warn(`Graph export failed: ${response.status} ${response.statusText}`);
      return null;
    }
    const data = (await response.json()) as {
      ok: boolean;
      nodes?: Array<Record<string, unknown>>;
      edges?: Array<Record<string, unknown>>;
    };
    if (!data.ok) return null;

    const nodes: ExportedGraphNode[] = (data.nodes ?? []).map((n) => ({
      name: String(n["id"] ?? n["name"] ?? ""),
      labels: (n["labels"] as string[]) ?? [],
      properties: {
        pagerank: n["pagerank"],
        community: n["community"],
      },
    }));

    const edges: ExportedGraphEdge[] = (data.edges ?? []).map((e) => ({
      source: String(e["source"] ?? ""),
      target: String(e["target"] ?? ""),
      relType: String(e["rel_type"] ?? e["relType"] ?? "RELATED_TO"),
    }));

    return { nodes, edges };
  } catch (err) {
    log.warn(`Graph export failed: ${err instanceof Error ? err.message : err}`);
    return null;
  }
}

// --- Main export function ---

export async function exportAgent(
  nousId: string,
  nousConfig: Record<string, unknown>,
  store: SessionStore,
  opts?: ExportOptions,
): Promise<AgentFile> {
  const options = { ...DEFAULT_OPTIONS, ...opts };

  log.info(`Exporting agent ${nousId}...`);

  // 1. Workspace files
  const workspacePath = paths.nousDir(nousId);
  let workspace = { files: {} as Record<string, string>, binaryFiles: [] as string[] };
  if (existsSync(workspacePath)) {
    workspace = scanWorkspace(workspacePath);
    log.info(`Workspace: ${Object.keys(workspace.files).length} text files, ${workspace.binaryFiles.length} binary files`);
  } else {
    log.warn(`Workspace not found: ${workspacePath}`);
  }

  // 2. Sessions
  const allSessions = store.listSessions(nousId);
  const sessions = options.includeArchived
    ? allSessions
    : allSessions.filter((s) => s.status !== "archived");

  const exportedSessions = sessions.map((s) =>
    exportSession(store, s, nousId, options.maxMessagesPerSession),
  );
  log.info(`Sessions: ${exportedSessions.length} (${sessions.length} total, ${allSessions.length - sessions.length} archived skipped)`);

  // 3. Memory (optional)
  let memory: AgentFile["memory"];
  if (options.includeMemory || options.includeGraph) {
    memory = {};

    if (options.includeMemory) {
      memory.vectors = await exportMemoryVectors(options.sidecarUrl, nousId);
      log.info(`Memory vectors: ${memory.vectors.length}`);
    }

    if (options.includeGraph) {
      const graph = await exportGraph(options.sidecarUrl);
      if (graph) {
        memory.graph = graph;
        log.info(`Graph: ${graph.nodes.length} nodes, ${graph.edges.length} edges`);
      }
    }
  }

  // 4. Build AgentFile
  const agentFile: AgentFile = {
    version: 1,
    exportedAt: new Date().toISOString(),
    generator: `aletheia-export/1.0`,
    nous: {
      id: nousId,
      name: (nousConfig["name"] as string) ?? null,
      model: (nousConfig["model"] as string) ?? null,
      config: nousConfig,
    },
    workspace,
    sessions: exportedSessions,
  };

  if (memory) {
    agentFile.memory = memory;
  }

  const jsonSize = JSON.stringify(agentFile).length;
  log.info(`Export complete: ${(jsonSize / 1024 / 1024).toFixed(1)}MB`);

  return agentFile;
}

// --- CLI entry point helper ---

export function agentFileToJson(agentFile: AgentFile, pretty = true): string {
  return JSON.stringify(agentFile, null, pretty ? 2 : undefined);
}
