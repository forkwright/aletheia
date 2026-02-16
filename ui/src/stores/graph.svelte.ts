// Graph visualization store
import { fetchGraphExport } from "../lib/api";
import type { GraphData, GraphNode, GraphEdge } from "../lib/types";

const EMPTY: GraphData = { nodes: [], edges: [], communities: 0 };

let graphData = $state<GraphData>(EMPTY);
let loading = $state(false);
let error = $state<string | null>(null);
let selectedNodeId = $state<string | null>(null);
let highlightedCommunity = $state<number | null>(null);
let searchQuery = $state("");

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

export async function loadGraph(community?: number): Promise<void> {
  loading = true;
  error = null;
  try {
    graphData = await fetchGraphExport(community);
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
    graphData = EMPTY;
  } finally {
    loading = false;
  }
}
