import type { Agent, Session, HistoryMessage, MetricsData, CostSummary, GraphData, FileTreeEntry, GitFileStatus, CommandInfo, Thread, ThreadMessage } from "./types";

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

function headers(): Record<string, string> {
  const token = getToken();
  return {
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    "Content-Type": "application/json",
  };
}

async function fetchJson<T>(path: string, opts?: RequestInit): Promise<T> {
  const base = import.meta.env.DEV ? "" : window.location.origin;
  const res = await fetch(`${base}${path}`, {
    ...opts,
    headers: { ...headers(), ...opts?.headers },
  });
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

export async function fetchGitStatus(
  agentId?: string,
): Promise<{ files: GitFileStatus[] }> {
  const sp = new URLSearchParams();
  if (agentId) sp.set("agentId", agentId);
  return fetchJson(`/api/workspace/git-status?${sp.toString()}`);
}
