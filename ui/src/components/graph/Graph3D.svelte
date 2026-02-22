<script lang="ts">
  import { onMount } from "svelte";
  import * as THREE from "three";
  import type { GraphData as AppGraphData } from "../../lib/types";

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
    "#7a2838", "#4a3860", "#5C8E63", "#9A7B4F", "#a06e3a",
    "#b07a8a", "#6b8fa3", "#7a9a8a", "#c49a6a", "#8b6a4a",
    "#6b7b6b", "#8aad6e", "#a07a5a", "#7a6b8a", "#9a8a6a",
    "#6a8a7a", "#a08060", "#8a7060", "#7a8a9a", "#6a7a5a",
  ];

  function communityColor(community: number): string {
    if (community < 0) return "#302c28";
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

    if (social.includes(relType)) return "rgba(63, 185, 80, 0.35)";
    if (structural.includes(relType)) return "rgba(154, 123, 79, 0.35)";
    if (temporal.includes(relType)) return "rgba(210, 153, 34, 0.35)";
    return "rgba(139, 148, 158, 0.25)";
  }

  let container: HTMLDivElement;
  let graph = $state<any>(null);
  let resizeObserver: ResizeObserver | null = null;

  // --- Community Cloud System ---
  const CLOUD_GEOMETRY = new THREE.SphereGeometry(1, 24, 16);
  const cloudMeshes = new Map<number, THREE.Mesh>();
  const cloudLabels = new Map<number, THREE.Sprite>();
  let cloudScene: THREE.Scene | null = null;

  function createCloudMaterial(color: string, opacity: number): THREE.MeshBasicMaterial {
    return new THREE.MeshBasicMaterial({
      color: new THREE.Color(color),
      transparent: true,
      opacity,
      depthWrite: false,
      side: THREE.BackSide,
    });
  }

  function createLabelSprite(text: string, color: string): THREE.Sprite {
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d")!;
    canvas.width = 512;
    canvas.height = 64;
    ctx.font = "bold 28px system-ui, -apple-system, sans-serif";
    ctx.fillStyle = color;
    ctx.globalAlpha = 0.8;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const label = text.length > 30 ? text.slice(0, 27) + "..." : text;
    ctx.fillText(label, 256, 32);

    const texture = new THREE.CanvasTexture(canvas);
    const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthWrite: false });
    const sprite = new THREE.Sprite(material);
    sprite.scale.set(60, 8, 1);
    sprite.renderOrder = 10;
    return sprite;
  }

  function updateClouds(runtimeNodes: any[]) {
    if (!cloudScene) return;

    const byCommunity = new Map<number, any[]>();
    for (const node of runtimeNodes) {
      if (node.community == null || node.community < 0) continue;
      if (node.x == null) continue;
      const list = byCommunity.get(node.community) || [];
      list.push(node);
      byCommunity.set(node.community, list);
    }

    for (const [cid, mesh] of cloudMeshes) {
      if (!byCommunity.has(cid)) {
        cloudScene.remove(mesh);
        (mesh.material as THREE.Material).dispose();
        cloudMeshes.delete(cid);
      }
    }
    for (const [cid, sprite] of cloudLabels) {
      if (!byCommunity.has(cid)) {
        cloudScene.remove(sprite);
        (sprite.material as THREE.SpriteMaterial).map?.dispose();
        sprite.material.dispose();
        cloudLabels.delete(cid);
      }
    }

    const hl = highlightedCommunity;

    for (const [cid, members] of byCommunity) {
      if (members.length < 3) continue;

      let cx = 0, cy = 0, cz = 0;
      for (const m of members) { cx += m.x; cy += m.y; cz += m.z; }
      cx /= members.length; cy /= members.length; cz /= members.length;

      let sumSqDist = 0;
      for (const m of members) {
        const dx = m.x - cx, dy = m.y - cy, dz = m.z - cz;
        sumSqDist += dx * dx + dy * dy + dz * dz;
      }
      const stddev = Math.sqrt(sumSqDist / members.length);
      const radius = Math.max(stddev * 1.5, 25);

      let mesh = cloudMeshes.get(cid);
      if (!mesh) {
        const color = communityColor(cid);
        mesh = new THREE.Mesh(CLOUD_GEOMETRY, createCloudMaterial(color, 0.06));
        mesh.renderOrder = -1;
        cloudScene.add(mesh);
        cloudMeshes.set(cid, mesh);
      }
      mesh.position.set(cx, cy, cz);
      mesh.scale.setScalar(radius);

      const mat = mesh.material as THREE.MeshBasicMaterial;
      if (hl === null) {
        mat.opacity = 0.06;
      } else if (cid === hl) {
        mat.opacity = 0.12;
      } else {
        mat.opacity = 0.02;
      }

      let sprite = cloudLabels.get(cid);
      if (!sprite) {
        const topNode = members.reduce((a: any, b: any) =>
          (a.pagerank || 0) > (b.pagerank || 0) ? a : b
        );
        sprite = createLabelSprite(topNode.id, communityColor(cid));
        cloudScene.add(sprite);
        cloudLabels.set(cid, sprite);
      }
      sprite.position.set(cx, cy + radius * 0.9, cz);
      (sprite.material as THREE.SpriteMaterial).opacity = hl === null ? 0.6 : (cid === hl ? 0.9 : 0.15);
    }
  }

  function disposeClouds() {
    if (!cloudScene) return;
    for (const [, mesh] of cloudMeshes) {
      cloudScene.remove(mesh);
      (mesh.material as THREE.Material).dispose();
    }
    for (const [, sprite] of cloudLabels) {
      cloudScene.remove(sprite);
      (sprite.material as THREE.SpriteMaterial).map?.dispose();
      sprite.material.dispose();
    }
    cloudMeshes.clear();
    cloudLabels.clear();
    cloudScene = null;
  }

  function buildGraphInput(data: AppGraphData) {
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.source, target: e.target, rel_type: e.rel_type })),
    };
  }

  export function focusOnNode(nodeId: string) {
    if (!graph) return;
    const gd = graph.graphData();
    const runtimeNode = gd.nodes.find((n: any) => n.id === nodeId);
    if (!runtimeNode) return;

    const distance = 120;
    const distRatio = 1 + distance / Math.hypot(runtimeNode.x || 0, runtimeNode.y || 0, runtimeNode.z || 0);
    graph.cameraPosition(
      { x: (runtimeNode.x || 0) * distRatio, y: (runtimeNode.y || 0) * distRatio, z: (runtimeNode.z || 0) * distRatio },
      runtimeNode,
      1000,
    );
  }

  export function zoomToFit() {
    if (graph) graph.zoomToFit(500, 50);
  }

  async function initGraph() {
    if (!container) return;

    const ForceGraph3D = (await import("3d-force-graph")).default;

    graph = new ForceGraph3D(container)
      .backgroundColor("#0a0a0f")
      .showNavInfo(false)
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
        return `<span style="color:#e8e6e3;font-family:system-ui;font-size:12px">
          <b>${node.id}</b><br/>
          Community ${comm} \u00b7 PR ${pr}
        </span>`;
      })
      .nodeOpacity(0.9)
      .linkSource("source")
      .linkTarget("target")
      .linkColor((link: any) => edgeColor(link.rel_type))
      .linkWidth(0.5)
      .linkOpacity(0.6)
      .linkDirectionalArrowLength(3)
      .linkDirectionalArrowRelPos(1)
      .linkLabel((link: any) => `<span style="color:#6b6560;font-size:11px">${link.rel_type}</span>`)
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
      .warmupTicks(100)
      .cooldownTime(3000)
      .onEngineTick(() => {
        const gd = graph.graphData();
        updateClouds(gd.nodes);
      });

    cloudScene = graph.scene();

    resizeObserver = new ResizeObserver(() => {
      if (graph && container) {
        graph.width(container.clientWidth).height(container.clientHeight);
      }
    });
    resizeObserver.observe(container);

    setTimeout(() => {
      if (graph) graph.zoomToFit(500, 50);
    }, 3500);
  }

  let prevNodeCount = 0;
  $effect(() => {
    const nodeCount = graphData.nodes.length;
    if (graph && nodeCount > 0 && nodeCount !== prevNodeCount) {
      prevNodeCount = nodeCount;
      disposeClouds();
      graph.graphData(buildGraphInput(graphData));
      cloudScene = graph.scene();
      setTimeout(() => {
        if (graph) graph.zoomToFit(500, 50);
      }, 2000);
    }
  });

  onMount(() => {
    initGraph();
    return () => {
      resizeObserver?.disconnect();
      disposeClouds();
      CLOUD_GEOMETRY.dispose();
      if (graph) graph._destructor();
    };
  });
</script>

<div class="graph3d-container" bind:this={container}></div>

<style>
  .graph3d-container {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  .graph3d-container :global(canvas) {
    display: block;
  }
</style>
