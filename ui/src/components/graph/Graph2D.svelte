<script lang="ts">
  import { onMount } from "svelte";
  import type { GraphData as AppGraphData } from "../../lib/types";
  import { getFilteredEdges } from "../../stores/graph.svelte";
  import { communityColor } from "../../lib/graph-colors";

  import type { AgentOverlayData, DriftData } from "../../lib/api";

  let {
    graphData,
    selectedNodeId = null,
    highlightedCommunity = null,
    hoverNodeId = $bindable(null),
    onNodeClick,
    onBackgroundClick,
    agentOverlay = null,
    agentFilter = null,
    agentColors = {},
    driftData = null,
  }: {
    graphData: AppGraphData;
    selectedNodeId?: string | null;
    highlightedCommunity?: number | null;
    hoverNodeId?: string | null;
    onNodeClick?: (nodeId: string) => void;
    onBackgroundClick?: () => void;
    agentOverlay?: AgentOverlayData | null;
    agentFilter?: string | null;
    agentColors?: Record<string, string>;
    driftData?: DriftData | null;
  } = $props();

  // Runtime node/link shapes from force-graph (untyped 3rd-party library)
  interface FGNode {
    id: string;
    pagerank?: number;
    community?: number;
    x: number;
    y: number;
  }

  interface FGLink {
    rel_type: string;
  }

  interface ForceGraph2DInstance {
    graphData(): { nodes: FGNode[]; links: FGLink[] };
    graphData(data: { nodes: object[]; links: object[] }): this;
    backgroundColor(color: string): this;
    width(w: number): this;
    height(h: number): this;
    nodeId(id: string): this;
    nodeVal(fn: (node: FGNode) => number): this;
    nodeColor(fn: (node: FGNode) => string): this;
    nodeLabel(fn: (node: FGNode) => string): this;
    nodeCanvasObjectMode(fn: () => string): this;
    nodeCanvasObject(fn: (node: FGNode, ctx: CanvasRenderingContext2D, globalScale: number) => void): this;
    linkSource(id: string): this;
    linkTarget(id: string): this;
    linkColor(fn: (link: FGLink) => string): this;
    linkWidth(w: number): this;
    linkDirectionalArrowLength(len: number): this;
    linkDirectionalArrowRelPos(pos: number): this;
    linkLabel(fn: (link: FGLink) => string): this;
    onNodeClick(fn: (node: FGNode) => void): this;
    onNodeHover(fn: (node: FGNode | null) => void): this;
    onBackgroundClick(fn: () => void): this;
    warmupTicks(n: number): this;
    cooldownTime(ms: number): this;
    centerAt(x: number, y: number, ms: number): void;
    zoom(factor: number, ms: number): void;
    zoomToFit(ms: number, padding: number): void;
    _destructor(): void;
  }

  // Build lookup sets for drift visualization
  let orphanedSet = $derived(new Set(driftData?.orphaned_nodes?.map(n => n.name) ?? []));
  let staleSet = $derived(new Set(driftData?.stale_entities?.map(e => e.name) ?? []));

  function pagerankSize(pr: number): number {
    const clamped = Math.max(pr, 0.0001);
    const scaled = (Math.log(clamped) + 10) / 10;
    return Math.max(2, Math.min(14, scaled * 12));
  }

  function edgeColor(relType: string): string {
    const social = ["KNOWS", "LIVES_WITH", "FAMILY_OF", "WORKS_WITH", "COMMUNICATES_WITH"];
    const structural = ["PART_OF", "CONTAINS", "DEPENDS_ON", "INSTANCE_OF", "LOCATED_AT"];
    const temporal = ["PRECEDES", "FOLLOWS", "OCCURRED_AT", "MENTIONS"];

    if (social.includes(relType)) return "rgba(63, 185, 80, 0.5)";
    if (structural.includes(relType)) return "rgba(154, 123, 79, 0.5)";
    if (temporal.includes(relType)) return "rgba(210, 153, 34, 0.5)";
    return "rgba(139, 148, 158, 0.35)";
  }

  // oxlint-disable-next-line no-unassigned-vars -- Svelte bind:this; assigned by Svelte runtime via template
  let container: HTMLDivElement;
  let graph = $state<ForceGraph2DInstance | null>(null);
  let resizeObserver: ResizeObserver | null = null;

  function buildGraphInput(data: AppGraphData) {
    const edges = getFilteredEdges();
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: edges.map((e) => ({ source: e.source, target: e.target, rel_type: e.rel_type })),
    };
  }

  export function focusOnNode(nodeId: string) {
    if (!graph) return;
    const gd = graph.graphData();
    const node = gd.nodes.find((n) => n.id === nodeId);
    if (node && node.x !== null && node.y !== null) {
      graph.centerAt(node.x, node.y, 500);
      graph.zoom(4, 500);
    }
  }

  export function zoomToFit() {
    if (graph) graph.zoomToFit(500, 50);
  }

  async function initGraph() {
    if (!container) return;

    const ForceGraph2D = (await import("force-graph")).default;

    graph = (new ForceGraph2D(container) as unknown as ForceGraph2DInstance)
      .backgroundColor("#0a0a0f")
      .width(container.clientWidth)
      .height(container.clientHeight)
      .graphData(buildGraphInput(graphData))
      .nodeId("id")
      .nodeVal((node) => pagerankSize(node.pagerank || 0.001))
      .nodeColor((node) => {
        if (hoverNodeId && node.id === hoverNodeId) return "#ffffff";
        if (selectedNodeId === node.id) return "#ffffff";

        // Agent overlay mode
        if (agentOverlay) {
          const nodeInfo = agentOverlay.node_agents[node.id];
          if (!nodeInfo) return "rgba(48, 54, 61, 0.3)"; // Unowned node — dim
          if (agentFilter && nodeInfo.primary !== agentFilter) return "rgba(48, 54, 61, 0.25)";
          return agentColors[nodeInfo.primary] ?? "#6b6560";
        }

        // Drift overlay mode
        if (driftData) {
          if (orphanedSet.has(node.id)) return "#c75450"; // Red for orphaned
          if (staleSet.has(node.id)) return "#b8922f"; // Yellow for stale
          return communityColor(node.community ?? -1);
        }

        const hl = highlightedCommunity;
        if (hl !== null && node.community !== hl) return "rgba(48, 54, 61, 0.4)";
        return communityColor(node.community ?? -1);
      })
      .nodeLabel((node) => {
        const pr = node.pagerank ? node.pagerank.toFixed(4) : "\u2014";
        const comm = node.community !== undefined && node.community >= 0 ? node.community : "\u2014";
        let label = `${node.id}\nCommunity ${comm} \u00b7 PR ${pr}`;
        if (agentOverlay) {
          const info = agentOverlay.node_agents[node.id];
          if (info) label += `\nAgent: ${info.primary} (${info.total_mentions} mentions)`;
          else label += "\nNo agent ownership";
        }
        if (driftData) {
          if (orphanedSet.has(node.id)) label += "\n⚠ Orphaned (no relationships)";
          if (staleSet.has(node.id)) label += "\n⚠ Stale (>30d)";
        }
        return label;
      })
      .nodeCanvasObjectMode(() => "after")
      .nodeCanvasObject((node, ctx, globalScale) => {
        const fontSize = Math.max(10 / globalScale, 1.5);
        if (fontSize < 1.5) return;
        ctx.font = `${fontSize}px system-ui, -apple-system, sans-serif`;
        ctx.fillStyle = "rgba(230, 237, 243, 0.85)";
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const size = pagerankSize(node.pagerank || 0.001);
        ctx.fillText(node.id, node.x, node.y + size / 2 + fontSize * 0.3);

        // Drift indicators: ring around orphaned/stale nodes
        if (driftData) {
          if (orphanedSet.has(node.id)) {
            ctx.beginPath();
            ctx.arc(node.x, node.y, size / 2 + 2, 0, 2 * Math.PI);
            ctx.strokeStyle = "#c75450";
            ctx.lineWidth = 1.5 / globalScale;
            ctx.setLineDash([3 / globalScale, 2 / globalScale]);
            ctx.stroke();
            ctx.setLineDash([]);
          } else if (staleSet.has(node.id)) {
            ctx.beginPath();
            ctx.arc(node.x, node.y, size / 2 + 2, 0, 2 * Math.PI);
            ctx.strokeStyle = "#b8922f";
            ctx.lineWidth = 1 / globalScale;
            ctx.setLineDash([2 / globalScale, 2 / globalScale]);
            ctx.stroke();
            ctx.setLineDash([]);
          }
        }

        // Agent overlay: small dot indicator for primary agent
        if (agentOverlay) {
          const nodeInfo = agentOverlay.node_agents[node.id];
          if (nodeInfo) {
            const dotSize = Math.max(2, size / 4);
            ctx.beginPath();
            ctx.arc(node.x + size / 2 + 1, node.y - size / 2 - 1, dotSize, 0, 2 * Math.PI);
            ctx.fillStyle = agentColors[nodeInfo.primary] ?? "#6b6560";
            ctx.fill();
          }
        }
      })
      .linkSource("source")
      .linkTarget("target")
      .linkColor((link) => edgeColor(link.rel_type))
      .linkWidth(0.8)
      .linkDirectionalArrowLength(4)
      .linkDirectionalArrowRelPos(1)
      .linkLabel((link) => link.rel_type)
      .onNodeClick((node) => {
        onNodeClick?.(node.id);
      })
      .onNodeHover((node) => {
        hoverNodeId = node?.id ?? null;
        container.style.cursor = node ? "pointer" : "default";
      })
      .onBackgroundClick(() => {
        onBackgroundClick?.();
      })
      .warmupTicks(50)
      .cooldownTime(2000);

    resizeObserver = new ResizeObserver(() => {
      if (graph && container) {
        graph.width(container.clientWidth).height(container.clientHeight);
      }
    });
    resizeObserver.observe(container);

    setTimeout(() => {
      if (graph) graph.zoomToFit(500, 50);
    }, 2500);
  }

  // Update graph data when props change (progressive loading, reload)
  let prevNodeCount = 0;
  $effect(() => {
    const nodeCount = graphData.nodes.length;
    if (graph && nodeCount > 0 && nodeCount !== prevNodeCount) {
      prevNodeCount = nodeCount;
      graph.graphData(buildGraphInput(graphData));
      setTimeout(() => {
        if (graph) graph.zoomToFit(500, 50);
      }, 1500);
    }
  });

  // Re-render when edge filters change
  let prevFilteredCount = -1;
  $effect(() => {
    const filtered = getFilteredEdges();
    if (graph && filtered.length !== prevFilteredCount && prevFilteredCount !== -1) {
      graph.graphData(buildGraphInput(graphData));
    }
    prevFilteredCount = filtered.length;
  });

  onMount(() => {
    initGraph();
    return () => {
      resizeObserver?.disconnect();
      if (graph) graph._destructor();
    };
  });
</script>

<div class="graph2d-container" bind:this={container}></div>

<style>
  .graph2d-container {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  .graph2d-container :global(canvas) {
    display: block;
  }
</style>
