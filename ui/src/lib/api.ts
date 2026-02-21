import type { Agent, Session, HistoryMessage, MetricsData, CostSummary, GraphData, FileTreeEntry, GitFileStatus, CommandInfo, Thread, ThreadMessage, EntityDetail } from "./types";
import { getAccessToken, refresh } from "./auth";

const TOKEN_KEY = "aletheia_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

export function getEffectiveToken(): string | null {
  return getAccessToken() || getToken();
}

function headers(): Record<string, string> {
  const token = getEffectiveToken();
  return {
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    "Content-Type": "application/json",
  };
}

async function fetchJson<T>(path: string, opts?: RequestInit): Promise<T> {
  const base = import.meta.env.DEV ? "" : window.location.origin;
  let res = await fetch(`${base}${path}`, {
    ...opts,
    headers: { ...headers(), ...opts?.headers },
  });

  // Auto-refresh on 401 when using session auth
  if (res.status === 401 && getAccessToken()) {
    const refreshed = await refresh();
    if (refreshed) {
      res = await fetch(`${base}${path}`, {
        ...opts,
        headers: { ...headers(), ...opts?.headers },
      });
    }
  }

  if (res.status === 401) {
    throw new Error("Unauthorized — check your token");
  }
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`API error ${res.status}: ${body}`);
  }
  return res.json();
}

export interface Branding {
  name: string;
  tagline?: string;
  favicon?: string;
}

export async function fetchBranding(): Promise<Branding> {
  return fetchJson("/api/branding");
}

export async function fetchAgents(): Promise<Agent[]> {
  const data = await fetchJson<{ agents: Agent[] }>("/api/agents");
  return data.agents;
}

export async function fetchAgentIdentity(id: string): Promise<{ name: string; emoji: string | null }> {
  return fetchJson(`/api/agents/${id}/identity`);
}

export async function fetchSessions(nousId?: string): Promise<Session[]> {
  const query = nousId ? `?nousId=${nousId}` : "";
  const data = await fetchJson<{ sessions: Session[] }>(`/api/sessions${query}`);
  return data.sessions;
}

export async function fetchHistory(sessionId: string, limit = 100): Promise<HistoryMessage[]> {
  const data = await fetchJson<{ messages: HistoryMessage[] }>(
    `/api/sessions/${sessionId}/history?limit=${limit}`,
  );
  return data.messages;
}

export async function archiveSession(sessionId: string): Promise<void> {
  await fetchJson(`/api/sessions/${sessionId}/archive`, { method: "POST" });
}

export async function distillSession(sessionId: string): Promise<void> {
  await fetchJson(`/api/sessions/${sessionId}/distill`, { method: "POST" });
}

export async function fetchThreads(nousId?: string): Promise<Thread[]> {
  const qs = nousId ? `?nousId=${nousId}` : "";
  const data = await fetchJson<{ threads: Thread[] }>(`/api/threads${qs}`);
  return data.threads;
}

export async function fetchThreadHistory(
  threadId: string,
  opts?: { before?: string; limit?: number },
): Promise<ThreadMessage[]> {
  const sp = new URLSearchParams();
  if (opts?.before) sp.set("before", opts.before);
  if (opts?.limit) sp.set("limit", String(opts.limit));
  const qs = sp.toString();
  const data = await fetchJson<{ messages: ThreadMessage[] }>(
    `/api/threads/${threadId}/history${qs ? `?${qs}` : ""}`,
  );
  return data.messages;
}

export async function fetchMetrics(): Promise<MetricsData> {
  return fetchJson("/api/metrics");
}

export async function fetchCostSummary(): Promise<CostSummary> {
  return fetchJson("/api/costs/summary");
}

export async function fetchSessionCosts(sessionId: string): Promise<unknown> {
  return fetchJson(`/api/costs/session/${sessionId}`);
}

export interface GraphExportParams {
  mode?: "top" | "community" | "all";
  limit?: number;
  community?: number;
}

export async function fetchGraphExport(params?: GraphExportParams): Promise<GraphData> {
  const sp = new URLSearchParams();
  if (params?.mode) sp.set("mode", params.mode);
  if (params?.limit) sp.set("limit", String(params.limit));
  if (params?.community !== undefined) sp.set("community", String(params.community));
  const qs = sp.toString();
  const data = await fetchJson<{ ok: boolean } & GraphData>(
    `/api/memory/graph/export${qs ? `?${qs}` : ""}`,
  );
  return {
    nodes: data.nodes,
    edges: data.edges,
    communities: data.communities,
    community_meta: data.community_meta ?? [],
    total_nodes: data.total_nodes ?? data.nodes.length,
  };
}

export async function fetchCommands(): Promise<CommandInfo[]> {
  const data = await fetchJson<{ commands: CommandInfo[] }>("/api/commands");
  return data.commands;
}

export async function executeCommand(command: string, sessionId?: string): Promise<string> {
  const data = await fetchJson<{ ok: boolean; result: string }>("/api/command", {
    method: "POST",
    body: JSON.stringify({ command, sessionId }),
  });
  return data.result;
}

export async function approveToolCall(turnId: string, toolId: string, alwaysAllow = false): Promise<void> {
  await fetchJson(`/api/turns/${encodeURIComponent(turnId)}/tools/${encodeURIComponent(toolId)}/approve`, {
    method: "POST",
    body: JSON.stringify({ alwaysAllow }),
  });
}

export async function denyToolCall(turnId: string, toolId: string): Promise<void> {
  await fetchJson(`/api/turns/${encodeURIComponent(turnId)}/tools/${encodeURIComponent(toolId)}/deny`, {
    method: "POST",
  });
}

export async function fetchApprovalMode(): Promise<string> {
  const data = await fetchJson<{ mode: string }>("/api/approval/mode");
  return data.mode;
}

// --- Plans ---

export async function approvePlan(planId: string, skip?: number[]): Promise<void> {
  await fetchJson(`/api/plans/${encodeURIComponent(planId)}/approve`, {
    method: "POST",
    body: JSON.stringify(skip?.length ? { skip } : {}),
  });
}

export async function cancelPlan(planId: string): Promise<void> {
  await fetchJson(`/api/plans/${encodeURIComponent(planId)}/cancel`, {
    method: "POST",
  });
}

export async function fetchPlan(planId: string): Promise<import("./types").PlanProposal> {
  return fetchJson<import("./types").PlanProposal>(`/api/plans/${encodeURIComponent(planId)}`);
}

// --- Workspace File Explorer ---

export async function fetchWorkspaceTree(
  agentId?: string,
  path?: string,
  depth = 2,
): Promise<{ root: string; entries: FileTreeEntry[] }> {
  const sp = new URLSearchParams();
  if (agentId) sp.set("agentId", agentId);
  if (path) sp.set("path", path);
  sp.set("depth", String(depth));
  return fetchJson(`/api/workspace/tree?${sp.toString()}`);
}

export async function fetchWorkspaceFile(
  path: string,
  agentId?: string,
): Promise<{ path: string; size: number; content: string }> {
  const sp = new URLSearchParams({ path });
  if (agentId) sp.set("agentId", agentId);
  return fetchJson(`/api/workspace/file?${sp.toString()}`);
}

export async function saveWorkspaceFile(
  path: string,
  content: string,
  agentId?: string,
): Promise<void> {
  await fetchJson("/api/workspace/file", {
    method: "PUT",
    body: JSON.stringify({ path, content, ...(agentId ? { agentId } : {}) }),
  });
}

export async function fetchGitStatus(
  agentId?: string,
): Promise<{ files: GitFileStatus[] }> {
  const sp = new URLSearchParams();
  if (agentId) sp.set("agentId", agentId);
  return fetchJson(`/api/workspace/git-status?${sp.toString()}`);
}

// --- Graph Entity Management ---

export async function fetchEntityDetail(name: string): Promise<EntityDetail> {
  return fetchJson(`/api/memory/entity/${encodeURIComponent(name)}`);
}

export async function deleteEntity(name: string): Promise<{ deleted: boolean; relationships_removed: number }> {
  return fetchJson(`/api/memory/entity/${encodeURIComponent(name)}`, { method: "DELETE" });
}

export async function mergeEntities(source: string, target: string): Promise<{ merged: boolean; message: string }> {
  return fetchJson("/api/memory/entity/merge", {
    method: "POST",
    body: JSON.stringify({ source, target }),
  });
}

// --- Graph Intelligence (Spec 09 Phases 8-13) ---

export interface MemoryHealth {
  ok: boolean;
  total: number;
  sampled: number;
  stale: number;
  conflicts: number;
  flagged: number;
  forgotten: number;
  avg_confidence: number;
  by_agent: Record<string, number>;
  date_range: { oldest: string | null; newest: string | null };
}

export async function fetchMemoryHealth(): Promise<MemoryHealth> {
  return fetchJson("/api/memory/health");
}

export interface TimelineData {
  ok: boolean;
  nodes: Array<import("./types").GraphNode & { created_at?: string | null }>;
  edges: import("./types").GraphEdge[];
  total_nodes: number;
  date_range: { since: string | null; until: string | null };
}

export async function fetchGraphTimeline(since?: string, until?: string): Promise<TimelineData> {
  const sp = new URLSearchParams();
  if (since) sp.set("since", since);
  if (until) sp.set("until", until);
  const qs = sp.toString();
  return fetchJson(`/api/memory/graph/timeline${qs ? `?${qs}` : ""}`);
}

export interface AgentOverlayData {
  ok: boolean;
  node_agents: Record<string, { primary: string; agents: Record<string, number>; total_mentions: number }>;
  all_agents: string[];
  total_entities: number;
}

export async function fetchAgentOverlay(): Promise<AgentOverlayData> {
  return fetchJson("/api/memory/graph/agent-overlay");
}

export interface DriftData {
  ok: boolean;
  total_nodes: number;
  orphaned_nodes: Array<{ name: string; pagerank: number; community: number }>;
  low_connectivity: Array<{ name: string; pagerank: number; degree: number }>;
  small_clusters: Array<{ comm: number; members: string[]; size: number }>;
  stale_entities: Array<{ name: string; last_seen: string; age_days: number }>;
  suggestions: Array<{ type: string; entity: string; reason: string }>;
  suggestion_count: number;
}

export async function fetchDriftData(): Promise<DriftData> {
  return fetchJson("/api/memory/graph/drift");
}

// --- Tool Stats ---

export async function fetchToolStats(agentId?: string, window = "7d"): Promise<unknown> {
  const sp = new URLSearchParams();
  if (agentId) sp.set("agentId", agentId);
  sp.set("window", window);
  return fetchJson(`/api/tool-stats?${sp.toString()}`);
}
