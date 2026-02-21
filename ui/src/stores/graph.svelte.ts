// Graph visualization store
import {
  fetchGraphExport, fetchEntityDetail, deleteEntity, mergeEntities,
  fetchMemoryHealth, fetchAgentOverlay, fetchDriftData, fetchGraphTimeline,
  type GraphExportParams, type MemoryHealth, type AgentOverlayData, type DriftData,
} from "../lib/api";
import type { GraphData, GraphNode, GraphEdge, CommunityMeta, EntityDetail } from "../lib/types";

const EMPTY: GraphData = { nodes: [], edges: [], communities: 0, community_meta: [], total_nodes: 0 };

let graphData = $state<GraphData>(EMPTY);
let loading = $state(false);
let error = $state<string | null>(null);
let selectedNodeId = $state<string | null>(null);
let highlightedCommunity = $state<number | null>(null);
let searchQuery = $state("");
let loadedMode = $state<"top" | "community" | "all">("top");
let loadedLimit = $state(200);
let entityDetail = $state<EntityDetail | null>(null);
let entityLoading = $state(false);
let hiddenEdgeTypes = $state<Set<string>>(new Set());
let searchResults = $state<Array<{ id: string; labels: string[]; pagerank: number; community: number }>>([]);
let searchLoading = $state(false);

// --- Graph Intelligence State ---
let memoryHealth = $state<MemoryHealth | null>(null);
let healthLoading = $state(false);
let agentOverlay = $state<AgentOverlayData | null>(null);
let agentOverlayLoading = $state(false);
let driftData = $state<DriftData | null>(null);
let driftLoading = $state(false);
let timelineRange = $state<{ since?: string; until?: string }>({});
let activeOverlay = $state<"none" | "agents" | "drift" | "timeline">("none");
let selectedAgentFilter = $state<string | null>(null);

export function getGraphData(): GraphData {
  return graphData;
}

export function getLoading(): boolean {
  return loading;
}

export function getError(): string | null {
  return error;
}

export function getSelectedNodeId(): string | null {
  return selectedNodeId;
}

export function setSelectedNodeId(id: string | null): void {
  selectedNodeId = id;
}

export function getHighlightedCommunity(): number | null {
  return highlightedCommunity;
}

export function setHighlightedCommunity(id: number | null): void {
  highlightedCommunity = id;
}

export function getSearchQuery(): string {
  return searchQuery;
}

export function setSearchQuery(q: string): void {
  searchQuery = q;
}

export function getSelectedNode(): GraphNode | null {
  if (!selectedNodeId) return null;
  return graphData.nodes.find((n) => n.id === selectedNodeId) ?? null;
}

export function getNodeEdges(nodeId: string): GraphEdge[] {
  return graphData.edges.filter((e) => e.source === nodeId || e.target === nodeId);
}

export function getConnectedNodes(nodeId: string): GraphNode[] {
  const edgeNodeIds = new Set<string>();
  for (const e of graphData.edges) {
    if (e.source === nodeId) edgeNodeIds.add(e.target);
    if (e.target === nodeId) edgeNodeIds.add(e.source);
  }
  return graphData.nodes.filter((n) => edgeNodeIds.has(n.id));
}

export function getCommunityIds(): number[] {
  const ids = new Set<number>();
  for (const n of graphData.nodes) {
    if (n.community >= 0) ids.add(n.community);
  }
  return [...ids].sort((a, b) => a - b);
}

export function getLoadedMode(): string {
  return loadedMode;
}

export function getLoadedLimit(): number {
  return loadedLimit;
}

export function getTotalNodes(): number {
  return graphData.total_nodes;
}

export function getCommunityMeta(): CommunityMeta[] {
  return graphData.community_meta;
}

export function getEntityDetail(): EntityDetail | null {
  return entityDetail;
}

export function getEntityLoading(): boolean {
  return entityLoading;
}

export async function loadEntityDetail(name: string): Promise<void> {
  entityLoading = true;
  try {
    entityDetail = await fetchEntityDetail(name);
  } catch {
    entityDetail = null;
  } finally {
    entityLoading = false;
  }
}

export async function removeEntity(name: string): Promise<boolean> {
  try {
    await deleteEntity(name);
    graphData = {
      ...graphData,
      nodes: graphData.nodes.filter((n) => n.id !== name),
      edges: graphData.edges.filter((e) => e.source !== name && e.target !== name),
    };
    if (selectedNodeId === name) {
      selectedNodeId = null;
      entityDetail = null;
    }
    return true;
  } catch {
    return false;
  }
}

export async function mergeEntityNodes(source: string, target: string): Promise<boolean> {
  try {
    await mergeEntities(source, target);
    graphData = {
      ...graphData,
      nodes: graphData.nodes.filter((n) => n.id !== source),
      edges: graphData.edges.filter((e) => e.source !== source && e.target !== source),
    };
    if (selectedNodeId === source) {
      selectedNodeId = target;
      await loadEntityDetail(target);
    }
    return true;
  } catch {
    return false;
  }
}

export async function loadGraph(params?: GraphExportParams): Promise<void> {
  loading = true;
  error = null;
  try {
    graphData = await fetchGraphExport(params);
    loadedMode = params?.mode ?? "top";
    loadedLimit = params?.limit ?? 200;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
    graphData = EMPTY;
  } finally {
    loading = false;
  }
}

export function getHiddenEdgeTypes(): Set<string> {
  return hiddenEdgeTypes;
}

export function getEdgeTypes(): string[] {
  const data = graphData;
  if (!data) return [];
  const types = new Set<string>();
  for (const e of data.edges) {
    if (e.rel_type) types.add(e.rel_type);
  }
  return [...types].sort();
}

export function toggleEdgeType(type: string): void {
  const next = new Set(hiddenEdgeTypes);
  if (next.has(type)) {
    next.delete(type);
  } else {
    next.add(type);
  }
  hiddenEdgeTypes = next;
}

export function getFilteredEdges(): Array<{ source: string; target: string; rel_type: string }> {
  const data = graphData;
  if (!data) return [];
  if (hiddenEdgeTypes.size === 0) return data.edges;
  return data.edges.filter((e) => !hiddenEdgeTypes.has(e.rel_type));
}

export function getSearchResults() {
  return searchResults;
}

export function getSearchLoading() {
  return searchLoading;
}

export async function searchGraph(query: string, filters?: { community?: number; relationship?: string }): Promise<void> {
  searchLoading = true;
  try {
    const sp = new URLSearchParams();
    if (query) sp.set("q", query);
    if (filters?.community !== undefined) sp.set("community", String(filters.community));
    if (filters?.relationship) sp.set("relationship", filters.relationship);
    sp.set("limit", "50");

    const base = import.meta.env.DEV ? "" : window.location.origin;
    const token = localStorage.getItem("aletheia_token");
    const headers: Record<string, string> = {};
    if (token) headers["Authorization"] = `Bearer ${token}`;

    const res = await fetch(`${base}/api/memory/graph/search?${sp.toString()}`, { headers });
    const data = await res.json();
    searchResults = data.results ?? [];
  } catch (err) {
    console.error("Graph search failed:", err);
    searchResults = [];
  } finally {
    searchLoading = false;
  }
}

export function clearSearchResults(): void {
  searchResults = [];
}

// --- Graph Intelligence ---

export function getMemoryHealth(): MemoryHealth | null { return memoryHealth; }
export function getHealthLoading(): boolean { return healthLoading; }
export function getAgentOverlay(): AgentOverlayData | null { return agentOverlay; }
export function getAgentOverlayLoading(): boolean { return agentOverlayLoading; }
export function getDriftData(): DriftData | null { return driftData; }
export function getDriftLoading(): boolean { return driftLoading; }
export function getActiveOverlay(): "none" | "agents" | "drift" | "timeline" { return activeOverlay; }
export function setActiveOverlay(v: "none" | "agents" | "drift" | "timeline"): void { activeOverlay = v; }
export function getSelectedAgentFilter(): string | null { return selectedAgentFilter; }
export function setSelectedAgentFilter(v: string | null): void { selectedAgentFilter = v; }
export function getTimelineRange(): { since?: string; until?: string } { return timelineRange; }
export function setTimelineRange(r: { since?: string; until?: string }): void { timelineRange = r; }

export async function loadMemoryHealth(): Promise<void> {
  healthLoading = true;
  try {
    memoryHealth = await fetchMemoryHealth();
  } catch (e) {
    console.error("Failed to load memory health:", e);
    memoryHealth = null;
  } finally {
    healthLoading = false;
  }
}

export async function loadAgentOverlay(): Promise<void> {
  agentOverlayLoading = true;
  try {
    agentOverlay = await fetchAgentOverlay();
  } catch (e) {
    console.error("Failed to load agent overlay:", e);
    agentOverlay = null;
  } finally {
    agentOverlayLoading = false;
  }
}

export async function loadDriftData(): Promise<void> {
  driftLoading = true;
  try {
    driftData = await fetchDriftData();
  } catch (e) {
    console.error("Failed to load drift data:", e);
    driftData = null;
  } finally {
    driftLoading = false;
  }
}

export async function loadTimeline(since?: string, until?: string): Promise<void> {
  loading = true;
  try {
    const data = await fetchGraphTimeline(since, until);
    if (data.ok) {
      graphData = {
        nodes: data.nodes,
        edges: data.edges,
        communities: 0,
        community_meta: [],
        total_nodes: data.total_nodes,
      };
      timelineRange = { since, until };
    }
  } catch (e) {
    console.error("Failed to load timeline:", e);
  } finally {
    loading = false;
  }
}
