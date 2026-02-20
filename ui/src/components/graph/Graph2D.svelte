<script lang="ts">
  import { onMount } from "svelte";
  import type { GraphData as AppGraphData, GraphNode } from "../../lib/types";

  let {
    graphData,
    selectedNodeId = null,
    highlightedCommunity = null,
    hoverNodeId = $bindable(null),
    onNodeClick,
    onBackgroundClick,
  }: {
    graphData: AppGraphData;
    selectedNodeId?: string | null;
    highlightedCommunity?: number | null;
    hoverNodeId?: string | null;
    onNodeClick?: (nodeId: string) => void;
    onBackgroundClick?: () => void;
  } = $props();

  const PALETTE = [
    "#58a6ff", "#3fb950", "#d29922", "#f85149", "#bc8cff",
    "#f778ba", "#79c0ff", "#56d4dd", "#e3b341", "#db6d28",
    "#8b949e", "#7ee787", "#a5d6ff", "#ffa657", "#ff7b72",
    "#d2a8ff", "#ffd8b5", "#89dceb", "#f9e2af", "#a6e3a1",
  ];

  function communityColor(community: number): string {
    if (community < 0) return "#30363d";
    return PALETTE[community % PALETTE.length]!;
  }

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
    if (structural.includes(relType)) return "rgba(88, 166, 255, 0.5)";
    if (temporal.includes(relType)) return "rgba(210, 153, 34, 0.5)";
    return "rgba(139, 148, 158, 0.35)";
  }

  let container: HTMLDivElement;
  let graph = $state<any>(null);
  let resizeObserver: ResizeObserver | null = null;

  function buildGraphInput(data: AppGraphData) {
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.source, target: e.target, rel_type: e.rel_type })),
    };
  }

  export function focusOnNode(nodeId: string) {
    if (!graph) return;
    const gd = graph.graphData();
    const node = gd.nodes.find((n: any) => n.id === nodeId);
    if (node && node.x != null && node.y != null) {
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

    graph = new ForceGraph2D(container)
      .backgroundColor("#0a0a0f")
      .width(container.clientWidth)
      .height(container.clientHeight)
      .graphData(buildGraphInput(graphData))
      .nodeId("id")
      .nodeVal((node: any) => pagerankSize(node.pagerank || 0.001))
      .nodeColor((node: any) => {
        if (hoverNodeId && node.id === hoverNodeId) return "#ffffff";
        if (selectedNodeId === node.id) return "#ffffff";
        const hl = highlightedCommunity;
        if (hl !== null && node.community !== hl) return "rgba(48, 54, 61, 0.4)";
        return communityColor(node.community ?? -1);
      })
      .nodeLabel((node: any) => {
        const pr = node.pagerank ? node.pagerank.toFixed(4) : "\u2014";
        const comm = node.community >= 0 ? node.community : "\u2014";
        return `${node.id}\nCommunity ${comm} \u00b7 PR ${pr}`;
      })
      .nodeCanvasObjectMode(() => "after")
      .nodeCanvasObject((node: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
        const fontSize = Math.max(10 / globalScale, 1.5);
        if (fontSize < 1.5) return;
        ctx.font = `${fontSize}px system-ui, -apple-system, sans-serif`;
        ctx.fillStyle = "rgba(230, 237, 243, 0.85)";
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const size = pagerankSize(node.pagerank || 0.001);
        ctx.fillText(node.id, node.x, node.y + size / 2 + fontSize * 0.3);
      })
      .linkSource("source")
      .linkTarget("target")
      .linkColor((link: any) => edgeColor(link.rel_type))
      .linkWidth(0.8)
      .linkDirectionalArrowLength(4)
      .linkDirectionalArrowRelPos(1)
      .linkLabel((link: any) => link.rel_type)
      .onNodeClick((node: any) => {
        onNodeClick?.(node.id);
      })
      .onNodeHover((node: any) => {
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
