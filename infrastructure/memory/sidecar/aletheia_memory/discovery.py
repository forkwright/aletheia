# Serendipity engine — cross-domain discovery and unexpected connection finding

# NOTE: Do NOT add future annotations — see routes.py comment.

import asyncio
import logging
import os
import random
from datetime import datetime, timezone
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from .graph import neo4j_driver, neo4j_available, mark_neo4j_ok, mark_neo4j_down

logger = logging.getLogger("aletheia_memory.discovery")
discovery_router = APIRouter(prefix="/discovery")

ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")


class DiscoverRequest(BaseModel):
    topic: str
    user_id: str = "default"
    agent_id: str | None = None
    max_results: int = Field(default=10, ge=1, le=30)
    novelty_weight: float = Field(default=0.5, ge=0.0, le=1.0)


class ExplorePathsRequest(BaseModel):
    source: str
    target: str | None = None
    max_depth: int = Field(default=4, ge=2, le=6)
    max_paths: int = Field(default=5, ge=1, le=20)


@discovery_router.post("/discover")
async def discover(req: DiscoverRequest):
    """Given a topic, return both relevant AND surprising related knowledge.

    Serendipity = relevance × novelty (SerenQA-inspired dual scoring).
    Relevance: vector distance from topic to entity's associated memories.
    Novelty: cross-community score (entities in different communities from the query's home).

    Pipeline:
    1. Find the query's "home community" via entity extraction + graph lookup
    2. Get all entities with PageRank and community assignments
    3. For each entity, compute serendipity = relevance × novelty
    4. Relevance: inverse graph distance (shared neighborhood overlap)
    5. Novelty: 1.0 if different community, boosted by low PageRank (obscure = more novel)
    6. Return ranked results with explanations
    """
    if not neo4j_available():
        return {"ok": False, "available": False, "discoveries": []}

    try:
        import networkx as nx
    except ImportError:
        raise HTTPException(status_code=500, detail="Missing dependency")

    driver = neo4j_driver()

    try:
        G = nx.Graph()
        with driver.session() as session:
            nodes = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "RETURN n.name AS name, labels(n) AS labels, "
                "n.pagerank AS pagerank, n.community AS community"
            )
            for record in nodes:
                G.add_node(
                    record["name"],
                    labels=record["labels"],
                    pagerank=record["pagerank"] or 0.0,
                    community=record["community"] if record["community"] is not None else -1,
                )

            rels = session.run(
                "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                "RETURN a.name AS src, b.name AS dst, type(r) AS rel_type"
            )
            for record in rels:
                G.add_edge(record["src"], record["dst"], rel_type=record["rel_type"])

        if G.number_of_nodes() < 5:
            return {"ok": True, "discoveries": [], "reason": "graph too small"}

        # Find the topic's home entities — match by substring in node names
        topic_lower = req.topic.lower()
        home_nodes = [
            n for n in G.nodes()
            if topic_lower in n.lower() or n.lower() in topic_lower
        ]

        # If no direct match, find closest via shared terms
        if not home_nodes:
            topic_terms = set(topic_lower.split())
            scored = []
            for n in G.nodes():
                overlap = len(topic_terms & set(n.lower().split()))
                if overlap > 0:
                    scored.append((n, overlap))
            scored.sort(key=lambda x: -x[1])
            home_nodes = [s[0] for s in scored[:3]]

        # Determine home community
        home_communities: set[int] = set()
        for n in home_nodes:
            c = G.nodes[n].get("community", -1)
            if c >= 0:
                home_communities.add(c)

        if not home_communities:
            home_communities = {-1}

        # Score all entities for serendipity
        max_pagerank = max((G.nodes[n].get("pagerank", 0) for n in G.nodes()), default=1.0)
        if max_pagerank == 0:
            max_pagerank = 1.0

        scored_entities: list[dict[str, Any]] = []

        for node in G.nodes():
            if node in home_nodes:
                continue

            data = G.nodes[node]
            community = data.get("community", -1)
            pagerank = data.get("pagerank", 0.0)

            # Relevance: graph proximity to home nodes
            min_distance = float("inf")
            for home in home_nodes:
                try:
                    d = nx.shortest_path_length(G, home, node)
                    min_distance = min(min_distance, d)
                except nx.NetworkXNoPath:
                    pass

            if min_distance == float("inf"):
                relevance = 0.0
            else:
                relevance = 1.0 / (1.0 + min_distance)

            # Novelty: cross-community + obscurity bonus
            cross_community = 1.0 if community not in home_communities and community >= 0 else 0.3
            obscurity = 1.0 - (pagerank / max_pagerank)
            novelty = 0.6 * cross_community + 0.4 * obscurity

            # Serendipity = weighted combination
            relevance_weight = 1.0 - req.novelty_weight
            serendipity = relevance_weight * relevance + req.novelty_weight * novelty

            if serendipity > 0.1 and relevance > 0:
                # Get connection context
                neighbors = list(G.neighbors(node))
                neighbor_labels = [
                    G.nodes[nb].get("labels", ["Entity"])[0] if G.nodes[nb].get("labels") else "Entity"
                    for nb in neighbors[:5]
                ]

                scored_entities.append({
                    "entity": node,
                    "serendipity": round(serendipity, 4),
                    "relevance": round(relevance, 4),
                    "novelty": round(novelty, 4),
                    "community": community,
                    "pagerank": round(pagerank, 6),
                    "graph_distance": min_distance if min_distance != float("inf") else None,
                    "neighbors": neighbors[:5],
                    "neighbor_types": neighbor_labels,
                    "degree": G.degree(node),
                })

        scored_entities.sort(key=lambda x: -x["serendipity"])
        top = scored_entities[:req.max_results]

        # Generate natural-language discovery descriptions via LLM
        if top and ANTHROPIC_API_KEY:
            top = await _annotate_discoveries(top, req.topic, home_nodes)

        return {
            "ok": True,
            "topic": req.topic,
            "home_entities": home_nodes[:5],
            "home_communities": list(home_communities),
            "discoveries": top,
            "graph_size": {"nodes": G.number_of_nodes(), "edges": G.number_of_edges()},
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("discover failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "discoveries": []}
    finally:
        driver.close()


@discovery_router.post("/explore_paths")
async def explore_paths(req: ExplorePathsRequest):
    """Find interesting paths between entities — or from one entity into the unknown.

    If target is provided: find all shortest paths and one random longer path.
    If target is None: find high-novelty entities reachable within max_depth and return paths.
    """
    if not neo4j_available():
        return {"ok": False, "available": False, "paths": []}

    try:
        import networkx as nx
    except ImportError:
        raise HTTPException(status_code=500, detail="Missing dependency")

    driver = neo4j_driver()

    try:
        G = nx.Graph()
        edge_labels: dict[tuple[str, str], str] = {}

        with driver.session() as session:
            nodes = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "RETURN n.name AS name, n.community AS community, n.pagerank AS pagerank"
            )
            for record in nodes:
                G.add_node(
                    record["name"],
                    community=record["community"] if record["community"] is not None else -1,
                    pagerank=record["pagerank"] or 0.0,
                )

            rels = session.run(
                "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                "RETURN a.name AS src, b.name AS dst, type(r) AS rel_type"
            )
            for record in rels:
                G.add_edge(record["src"], record["dst"])
                edge_labels[(record["src"], record["dst"])] = record["rel_type"]
                edge_labels[(record["dst"], record["src"])] = record["rel_type"]

        # Find source node (fuzzy match)
        source_lower = req.source.lower()
        source_node = None
        for n in G.nodes():
            if n.lower() == source_lower or source_lower in n.lower():
                source_node = n
                break

        if not source_node:
            return {"ok": True, "paths": [], "reason": f"entity '{req.source}' not found in graph"}

        paths: list[dict[str, Any]] = []

        if req.target:
            # Find paths to specific target
            target_lower = req.target.lower()
            target_node = None
            for n in G.nodes():
                if n.lower() == target_lower or target_lower in n.lower():
                    target_node = n
                    break

            if not target_node:
                return {"ok": True, "paths": [], "reason": f"target '{req.target}' not found in graph"}

            try:
                for path in nx.all_shortest_paths(G, source_node, target_node):
                    if len(paths) >= req.max_paths:
                        break
                    paths.append(_format_path(path, edge_labels, G))

                # Try one longer path for novelty
                if len(paths) < req.max_paths:
                    try:
                        all_simple = list(nx.all_simple_paths(G, source_node, target_node, cutoff=req.max_depth))
                        longer = [p for p in all_simple if len(p) > (len(paths[0]["nodes"]) if paths else 0)]
                        if longer:
                            chosen = random.choice(longer[:10])
                            formatted = _format_path(chosen, edge_labels, G)
                            formatted["path_type"] = "detour"
                            paths.append(formatted)
                    except Exception:
                        pass

            except nx.NetworkXNoPath:
                return {"ok": True, "paths": [], "reason": "no path exists between entities"}
        else:
            # Explore outward — find the most interesting reachable nodes
            source_community = G.nodes[source_node].get("community", -1)
            reachable: dict[str, int] = {}
            for node in nx.single_source_shortest_path_length(G, source_node, cutoff=req.max_depth):
                if node != source_node:
                    reachable[node] = nx.shortest_path_length(G, source_node, node)

            # Score by cross-community + distance
            scored = []
            for node, dist in reachable.items():
                c = G.nodes[node].get("community", -1)
                cross = 1.0 if c != source_community and c >= 0 else 0.3
                interest = cross * dist
                scored.append((node, interest, dist))

            scored.sort(key=lambda x: -x[1])

            for node, interest, dist in scored[:req.max_paths]:
                try:
                    path = nx.shortest_path(G, source_node, node)
                    formatted = _format_path(path, edge_labels, G)
                    formatted["interest_score"] = round(interest, 3)
                    paths.append(formatted)
                except nx.NetworkXNoPath:
                    pass

        return {
            "ok": True,
            "source": source_node,
            "target": req.target,
            "paths": paths,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("explore_paths failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "paths": []}
    finally:
        driver.close()


@discovery_router.post("/generate_candidates")
async def generate_discovery_candidates():
    """Nightly batch job: find cross-community bridges and novel connections.

    Writes top discovery candidates as Neo4j DiscoveryCandidate nodes
    for later retrieval by prosoche or agents.
    """
    if not neo4j_available():
        return {"ok": False, "available": False, "candidates": 0}

    try:
        import networkx as nx
    except ImportError:
        raise HTTPException(status_code=500, detail="Missing dependency")

    driver = neo4j_driver()

    try:
        G = nx.Graph()
        with driver.session() as session:
            for record in session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "RETURN n.name AS name, n.community AS community, n.pagerank AS pagerank"
            ):
                G.add_node(
                    record["name"],
                    community=record["community"] if record["community"] is not None else -1,
                    pagerank=record["pagerank"] or 0.0,
                )

            for record in session.run(
                "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                "RETURN a.name AS src, b.name AS dst, type(r) AS rel_type"
            ):
                G.add_edge(record["src"], record["dst"], rel_type=record["rel_type"])

        if G.number_of_nodes() < 10:
            return {"ok": True, "candidates": 0, "reason": "graph too small"}

        # Find cross-community bridges — edges that connect different communities
        bridges: list[dict[str, Any]] = []
        for u, v in G.edges():
            cu = G.nodes[u].get("community", -1)
            cv = G.nodes[v].get("community", -1)
            if cu != cv and cu >= 0 and cv >= 0:
                # Bridge score: lower degree = more surprising bridge
                bridge_score = 1.0 / (1.0 + min(G.degree(u), G.degree(v)))
                bridges.append({
                    "entity_a": u,
                    "entity_b": v,
                    "community_a": cu,
                    "community_b": cv,
                    "bridge_score": bridge_score,
                    "rel_type": G.edges[u, v].get("rel_type", "CONNECTED"),
                })

        bridges.sort(key=lambda x: -x["bridge_score"])

        # Find weak ties (betweenness centrality identifies bridge nodes)
        betweenness = nx.betweenness_centrality(G)
        high_betweenness = sorted(betweenness.items(), key=lambda x: -x[1])[:20]

        # Store top candidates in Neo4j
        now = datetime.now(timezone.utc).isoformat()
        stored = 0

        with driver.session() as session:
            # Clear old candidates
            session.run("MATCH (d:DiscoveryCandidate) WHERE d.generated_at < $cutoff DETACH DELETE d",
                        cutoff=now)

            for bridge in bridges[:20]:
                session.run(
                    """
                    MERGE (d:DiscoveryCandidate {
                        entity_a: $a,
                        entity_b: $b
                    })
                    SET d.bridge_score = $score,
                        d.community_a = $ca,
                        d.community_b = $cb,
                        d.rel_type = $rel,
                        d.generated_at = $now,
                        d.type = 'cross_community_bridge'
                    """,
                    a=bridge["entity_a"],
                    b=bridge["entity_b"],
                    score=round(bridge["bridge_score"], 4),
                    ca=bridge["community_a"],
                    cb=bridge["community_b"],
                    rel=bridge["rel_type"],
                    now=now,
                )
                stored += 1

            for node, centrality in high_betweenness[:10]:
                session.run(
                    """
                    MERGE (d:DiscoveryCandidate {
                        entity_a: $node,
                        entity_b: 'hub'
                    })
                    SET d.bridge_score = $centrality,
                        d.generated_at = $now,
                        d.type = 'high_betweenness_hub'
                    """,
                    node=node,
                    centrality=round(centrality, 6),
                    now=now,
                )
                stored += 1

        driver.close()

        return {
            "ok": True,
            "candidates": stored,
            "cross_community_bridges": len(bridges),
            "top_bridges": bridges[:10],
            "high_betweenness_hubs": [{"name": n, "centrality": round(c, 6)} for n, c in high_betweenness[:10]],
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("generate_discovery_candidates failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "candidates": 0}
    finally:
        driver.close()


@discovery_router.get("/candidates")
async def get_discovery_candidates(limit: int = 20):
    """Retrieve stored discovery candidates for agents to act on."""
    if not neo4j_available():
        return {"ok": True, "candidates": []}

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            result = session.run(
                """
                MATCH (d:DiscoveryCandidate)
                RETURN d.entity_a AS entity_a, d.entity_b AS entity_b,
                       d.bridge_score AS score, d.type AS type,
                       d.community_a AS community_a, d.community_b AS community_b,
                       d.rel_type AS rel_type, d.generated_at AS generated_at
                ORDER BY d.bridge_score DESC
                LIMIT $limit
                """,
                limit=limit,
            )
            candidates = [
                {
                    "entity_a": r["entity_a"],
                    "entity_b": r["entity_b"],
                    "score": r["score"],
                    "type": r["type"],
                    "community_a": r["community_a"],
                    "community_b": r["community_b"],
                    "rel_type": r["rel_type"],
                    "generated_at": str(r["generated_at"]) if r["generated_at"] else None,
                }
                for r in result
            ]
        driver.close()

        return {"ok": True, "candidates": candidates}
    except Exception as e:
        logger.warning(f"get_discovery_candidates failed: {e}")
        return {"ok": True, "candidates": [], "error": "Internal error"}


@discovery_router.get("/stats")
async def discovery_stats():
    """Discovery engine statistics."""
    if not neo4j_available():
        return {"ok": True, "available": False}

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            candidate_count = session.run(
                "MATCH (d:DiscoveryCandidate) RETURN count(d) AS c"
            ).single()["c"]
            bridge_count = session.run(
                "MATCH (d:DiscoveryCandidate) WHERE d.type = 'cross_community_bridge' RETURN count(d) AS c"
            ).single()["c"]
            hub_count = session.run(
                "MATCH (d:DiscoveryCandidate) WHERE d.type = 'high_betweenness_hub' RETURN count(d) AS c"
            ).single()["c"]
            community_count = session.run(
                "MATCH (n) WHERE n.community IS NOT NULL "
                "RETURN count(DISTINCT n.community) AS c"
            ).single()["c"]
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "total_candidates": candidate_count,
            "cross_community_bridges": bridge_count,
            "high_betweenness_hubs": hub_count,
            "communities_in_graph": community_count,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("discovery_stats failed (Neo4j may be down): %s", e)
        return {"ok": True, "available": False}


# --- Internal helpers ---


def _format_path(
    path: list[str],
    edge_labels: dict[tuple[str, str], str],
    G: Any,
) -> dict[str, Any]:
    """Format a graph path into a readable structure."""
    edges = []
    for i in range(len(path) - 1):
        rel = edge_labels.get((path[i], path[i + 1]), "CONNECTED")
        edges.append({"from": path[i], "to": path[i + 1], "relationship": rel})

    communities_traversed = set()
    for node in path:
        c = G.nodes[node].get("community", -1)
        if c >= 0:
            communities_traversed.add(c)

    return {
        "nodes": path,
        "edges": edges,
        "length": len(path) - 1,
        "communities_traversed": len(communities_traversed),
        "path_type": "shortest",
    }


async def _annotate_discoveries(
    discoveries: list[dict[str, Any]],
    topic: str,
    home_entities: list[str],
) -> list[dict[str, Any]]:
    """Use Haiku to generate natural-language descriptions of why discoveries are interesting."""
    if not ANTHROPIC_API_KEY:
        return discoveries

    # Batch — describe top 5 in one call
    top5 = discoveries[:5]
    entity_list = "\n".join(
        f"- {d['entity']} (community {d['community']}, distance {d.get('graph_distance', '?')}, "
        f"neighbors: {', '.join(d['neighbors'][:3])})"
        for d in top5
    )

    try:
        async with httpx.AsyncClient(timeout=12.0) as client:
            resp = await client.post(
                "https://api.anthropic.com/v1/messages",
                headers={
                    "x-api-key": ANTHROPIC_API_KEY,
                    "anthropic-version": "2023-06-01",
                    "content-type": "application/json",
                },
                json={
                    "model": "claude-haiku-4-5-20251001",
                    "max_tokens": 512,
                    "messages": [{
                        "role": "user",
                        "content": (
                            f'Topic: "{topic}" (related to: {", ".join(home_entities[:3])})\n\n'
                            f"These entities were found as potentially surprising connections:\n{entity_list}\n\n"
                            "For each entity, write ONE sentence explaining why the connection to the topic "
                            "might be interesting or surprising. Format: entity_name: explanation\n"
                            "Be specific about what the connection might reveal."
                        ),
                    }],
                },
            )
            if resp.status_code != 200:
                return discoveries

            data = resp.json()
            text = data.get("content", [{}])[0].get("text", "")

            # Parse annotations
            for line in text.strip().split("\n"):
                line = line.strip()
                if ":" not in line:
                    continue
                entity_name, _, explanation = line.partition(":")
                entity_name = entity_name.strip().strip("- ")
                explanation = explanation.strip()
                for d in discoveries:
                    if d["entity"].lower() in entity_name.lower() or entity_name.lower() in d["entity"].lower():
                        d["insight"] = explanation
                        break

    except Exception:
        logger.warning("Discovery annotation failed", exc_info=True)

    return discoveries
