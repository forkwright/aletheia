# API routes for Aletheia memory sidecar

from __future__ import annotations

import asyncio
import json
import logging
import os
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from .config import NEO4J_PASSWORD, NEO4J_URL, NEO4J_USER, QDRANT_HOST, QDRANT_PORT

logger = logging.getLogger("aletheia_memory")
router = APIRouter()


class AddRequest(BaseModel):
    text: str
    user_id: str = "default"
    agent_id: str | None = None
    metadata: dict[str, Any] | None = None


class SearchRequest(BaseModel):
    query: str
    user_id: str = "default"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)


class ImportRequest(BaseModel):
    facts: list[dict[str, Any]]
    user_id: str = "default"


DEDUP_THRESHOLD = 0.85


@router.post("/add")
async def add_memory(req: AddRequest, request: Request):
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": req.user_id}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id
    if req.metadata:
        kwargs["metadata"] = req.metadata

    try:
        # Cross-agent dedup: search globally (no agent_id) before adding
        existing = await asyncio.to_thread(mem.search, req.text, user_id=req.user_id, limit=3)
        results = existing.get("results", []) if isinstance(existing, dict) else existing
        for candidate in (results if isinstance(results, list) else []):
            top = candidate
            score = top.get("score", 0)
            if score > DEDUP_THRESHOLD:
                logger.info(
                    f"Dedup: skipped (score={score:.3f}, existing={top.get('id', '?')}, "
                    f"agent={req.agent_id or 'global'})"
                )
                return {"ok": True, "result": {"deduplicated": True, "existing_id": top.get("id"), "score": score}}

        result = await asyncio.to_thread(mem.add, req.text, **kwargs)

        # Autonomous link generation (A-Mem pattern) — fire and forget
        if LINK_GENERATION_ENABLED:
            asyncio.create_task(_generate_links(mem, req.text, req.user_id))

        return {"ok": True, "result": result}
    except Exception as e:
        logger.exception("add_memory failed")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/search")
async def search_memory(req: SearchRequest, request: Request):
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        raw = await asyncio.to_thread(mem.search, req.query, **kwargs)
        results = raw.get("results", raw) if isinstance(raw, dict) else raw
        return {"ok": True, "results": results}
    except Exception as e:
        logger.exception("search_memory failed")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/graph_search")
async def graph_search(req: SearchRequest, request: Request):
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        results = await asyncio.to_thread(mem.search, req.query, **kwargs)
        graph_results = [r for r in results.get("results", []) if r.get("source") == "graph"]
        return {"ok": True, "results": graph_results}
    except Exception as e:
        logger.exception("graph_search failed")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/import")
async def import_facts(req: ImportRequest, request: Request):
    mem = request.app.state.memory
    imported = 0
    errors = []

    for i, fact in enumerate(req.facts):
        subject = fact.get("subject", "")
        predicate = fact.get("predicate", "")
        obj = fact.get("object", "")
        text = f"{subject} {predicate} {obj}".strip()
        if not text:
            continue

        metadata = {
            "source": "facts.jsonl",
            "confidence": fact.get("confidence", 1.0),
        }
        if fact.get("domain"):
            metadata["domain"] = fact["domain"]
        if fact.get("agent"):
            metadata["original_agent"] = fact["agent"]

        try:
            await asyncio.to_thread(mem.add, text, user_id=req.user_id, metadata=metadata)
            imported += 1
        except Exception as e:
            errors.append({"index": i, "error": str(e)})
            if len(errors) > 10:
                break

    return {"ok": True, "imported": imported, "errors": errors}


@router.get("/memories")
async def list_memories(
    request: Request,
    user_id: str = "default",
    agent_id: str | None = None,
    limit: int = 50,
):
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": user_id}
    if agent_id:
        kwargs["agent_id"] = agent_id

    try:
        kwargs["limit"] = limit
        results = await asyncio.to_thread(mem.get_all, **kwargs)
        entries = results.get("results", results) if isinstance(results, dict) else results
        return {"ok": True, "memories": entries}
    except Exception as e:
        logger.exception("list_memories failed")
        raise HTTPException(status_code=500, detail=str(e))


@router.delete("/memories/{memory_id}")
async def delete_memory(memory_id: str, request: Request):
    mem = request.app.state.memory
    try:
        await asyncio.to_thread(mem.delete, memory_id)
        return {"ok": True}
    except Exception as e:
        logger.exception("delete_memory failed")
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/health")
async def health_check():
    checks: dict[str, Any] = {}

    async with httpx.AsyncClient(timeout=5.0) as client:
        try:
            r = await client.get(f"http://{QDRANT_HOST}:{QDRANT_PORT}/healthz")
            checks["qdrant"] = "ok" if r.status_code == 200 else f"status {r.status_code}"
        except Exception as e:
            checks["qdrant"] = f"error: {e}"

        try:
            r = await client.post(
                "https://api.voyageai.com/v1/embeddings",
                headers={
                    "Authorization": f"Bearer {os.environ.get('VOYAGE_API_KEY', '')}",
                    "Content-Type": "application/json",
                },
                json={"model": "voyage-3-large", "input": ["health check"]},
            )
            checks["voyage"] = "ok" if r.status_code == 200 else f"status {r.status_code}"
        except Exception as e:
            checks["voyage"] = f"error: {e}"

    try:
        from neo4j import GraphDatabase
        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
        driver.verify_connectivity()
        driver.close()
        checks["neo4j"] = "ok"
    except Exception as e:
        checks["neo4j"] = f"error: {e}"

    all_ok = all(v == "ok" for v in checks.values())
    return {"ok": all_ok, "checks": checks}


@router.get("/graph_stats")
async def graph_stats():
    from neo4j import GraphDatabase

    try:
        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
        with driver.session() as session:
            node_count = session.run("MATCH (n) RETURN count(n) AS c").single()["c"]
            rel_count = session.run("MATCH ()-[r]->() RETURN count(r) AS c").single()["c"]
            rel_types = session.run(
                "MATCH ()-[r]->() RETURN type(r) AS t, count(*) AS c ORDER BY c DESC LIMIT 30"
            ).data()
            top_nodes = session.run(
                "MATCH (n)-[r]-() RETURN n.name AS name, labels(n) AS labels, count(r) AS rels "
                "ORDER BY rels DESC LIMIT 10"
            ).data()
            singleton_types = session.run(
                "MATCH ()-[r]->() WITH type(r) AS t, count(*) AS c WHERE c = 1 RETURN count(t) AS c"
            ).single()["c"]
        driver.close()

        return {
            "ok": True,
            "nodes": node_count,
            "relationships": rel_count,
            "singleton_rel_types": singleton_types,
            "top_relationship_types": rel_types,
            "top_connected_nodes": top_nodes,
        }
    except Exception as e:
        logger.exception("graph_stats failed")
        raise HTTPException(status_code=500, detail=str(e))


# --- Phase 2.1: Graph-Enhanced Retrieval ---


class GraphEnhancedSearchRequest(BaseModel):
    query: str
    user_id: str = "default"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)
    graph_weight: float = Field(default=0.3, ge=0.0, le=1.0)
    graph_depth: int = Field(default=1, ge=1, le=3)


def _extract_entities(text: str) -> list[str]:
    """Heuristic entity extraction — capitalize words, proper nouns, known patterns."""
    entities: list[str] = []
    # Capitalized words (likely proper nouns)
    for match in re.finditer(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b", text):
        entities.append(match.group())
    # Technical terms (lowercase with hyphens/underscores)
    for match in re.finditer(r"\b[a-z]+[-_][a-z]+(?:[-_][a-z]+)*\b", text):
        entities.append(match.group())
    # Quoted strings
    for match in re.finditer(r'"([^"]+)"', text):
        entities.append(match.group(1))
    return list(set(entities))[:10]


@router.post("/graph_enhanced_search")
async def graph_enhanced_search(req: GraphEnhancedSearchRequest, request: Request):
    """Vector search enhanced with graph neighborhood expansion."""
    mem = request.app.state.memory

    # Step 1: Standard vector search
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit * 2}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        raw = await asyncio.to_thread(mem.search, req.query, **kwargs)
        vector_results = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception:
        logger.exception("graph_enhanced_search: vector search failed")
        vector_results = []

    # Step 2: Extract entities and expand via graph
    entities = _extract_entities(req.query)
    graph_neighbors: list[str] = []

    if entities and NEO4J_PASSWORD:
        try:
            from neo4j import GraphDatabase
            driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
            with driver.session() as session:
                for entity in entities[:5]:
                    result = session.run(
                        "MATCH (n)-[r*1.." + str(req.graph_depth) + "]-(neighbor) "
                        "WHERE toLower(n.name) CONTAINS toLower($name) "
                        "RETURN DISTINCT neighbor.name AS name, labels(neighbor) AS labels "
                        "LIMIT 10",
                        name=entity,
                    )
                    for record in result:
                        name = record["name"]
                        if name:
                            graph_neighbors.append(name)
            driver.close()
        except Exception:
            logger.warning("graph_enhanced_search: Neo4j unavailable, falling back to vector-only")

    # Step 3: If graph found neighbors, do a supplementary vector search with expanded terms
    graph_results: list[dict[str, Any]] = []
    if graph_neighbors:
        expanded_query = req.query + " " + " ".join(set(graph_neighbors)[:5])
        try:
            raw2 = await asyncio.to_thread(mem.search, expanded_query, **kwargs)
            graph_results = raw2.get("results", raw2) if isinstance(raw2, dict) else raw2
        except Exception:
            logger.warning("graph_enhanced_search: expanded search failed")

    # Step 4: Merge and deduplicate results
    seen_ids: set[str] = set()
    merged: list[dict[str, Any]] = []

    def add_result(r: dict[str, Any], source: str, weight: float) -> None:
        rid = r.get("id", r.get("hash", str(r.get("memory", ""))))
        if rid in seen_ids:
            return
        seen_ids.add(rid)
        score = r.get("score", 0.5)
        r["combined_score"] = score * weight
        r["retrieval_source"] = source
        merged.append(r)

    vector_weight = 1.0 - req.graph_weight
    for r in (vector_results if isinstance(vector_results, list) else []):
        add_result(r, "vector", vector_weight)
    for r in (graph_results if isinstance(graph_results, list) else []):
        add_result(r, "graph_expanded", req.graph_weight)

    merged.sort(key=lambda r: r.get("combined_score", 0), reverse=True)

    return {
        "ok": True,
        "results": merged[:req.limit],
        "entities_extracted": entities,
        "graph_neighbors": list(set(graph_neighbors))[:20],
        "sources": {"vector": len(vector_results) if isinstance(vector_results, list) else 0,
                     "graph_expanded": len(graph_results) if isinstance(graph_results, list) else 0},
    }


# --- Phase 2.2: Memory Consolidation ---


MERGE_THRESHOLD = 0.90


class ConsolidateRequest(BaseModel):
    dry_run: bool = False
    threshold: float = Field(default=MERGE_THRESHOLD, ge=0.5, le=1.0)
    user_id: str = "default"
    limit: int = Field(default=100, ge=10, le=500)


class MergeRequest(BaseModel):
    source_id: str
    target_id: str
    user_id: str = "default"


@router.post("/consolidate")
async def consolidate_memories(req: ConsolidateRequest, request: Request):
    """Find and optionally merge near-duplicate memories."""
    mem = request.app.state.memory

    try:
        raw = await asyncio.to_thread(mem.get_all, user_id=req.user_id, limit=req.limit)
        entries = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to fetch memories: {e}")

    if not isinstance(entries, list):
        return {"ok": True, "candidates": [], "message": "No memories found"}

    # Find near-duplicate pairs by searching each memory against the corpus
    candidates: list[dict[str, Any]] = []
    checked: set[str] = set()

    for entry in entries[:50]:  # Cap to prevent excessive API calls
        memory_text = entry.get("memory", "")
        memory_id = entry.get("id", "")
        if not memory_text or memory_id in checked:
            continue
        checked.add(memory_id)

        try:
            search_results = await asyncio.to_thread(
                mem.search, memory_text, user_id=req.user_id, limit=5
            )
            results = search_results.get("results", search_results) if isinstance(search_results, dict) else search_results
        except Exception:
            continue

        for r in (results if isinstance(results, list) else []):
            other_id = r.get("id", "")
            if other_id == memory_id or other_id in checked:
                continue
            score = r.get("score", 0)
            if score >= req.threshold:
                candidates.append({
                    "source": {"id": memory_id, "text": memory_text[:200]},
                    "duplicate": {"id": other_id, "text": r.get("memory", "")[:200]},
                    "score": round(score, 4),
                })

    merged_count = 0
    if not req.dry_run:
        for pair in candidates:
            try:
                # Keep the source, delete the duplicate
                await asyncio.to_thread(mem.delete, pair["duplicate"]["id"])
                merged_count += 1
                logger.info(f"Consolidated: deleted {pair['duplicate']['id']} (score={pair['score']})")
            except Exception as e:
                logger.warning(f"Failed to delete duplicate {pair['duplicate']['id']}: {e}")

    return {
        "ok": True,
        "candidates": len(candidates),
        "merged": merged_count,
        "dry_run": req.dry_run,
        "pairs": candidates[:20],
    }


@router.post("/merge")
async def merge_memories(req: MergeRequest, request: Request):
    """Merge two memories — keeps target, deletes source."""
    mem = request.app.state.memory
    try:
        await asyncio.to_thread(mem.delete, req.source_id)
        return {"ok": True, "deleted": req.source_id, "kept": req.target_id}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/fact_stats")
async def fact_stats(request: Request, user_id: str = "default"):
    """Memory corpus statistics."""
    mem = request.app.state.memory

    try:
        raw = await asyncio.to_thread(mem.get_all, user_id=user_id, limit=500)
        entries = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

    if not isinstance(entries, list):
        return {"ok": True, "total": 0}

    total = len(entries)
    avg_length = sum(len(e.get("memory", "")) for e in entries) / max(total, 1)

    # Categorize by metadata
    by_agent: dict[str, int] = {}
    by_domain: dict[str, int] = {}
    for entry in entries:
        meta = entry.get("metadata", {}) or {}
        agent = meta.get("agent_id") or meta.get("original_agent") or "unknown"
        domain = meta.get("domain", "general")
        by_agent[agent] = by_agent.get(agent, 0) + 1
        by_domain[domain] = by_domain.get(domain, 0) + 1

    return {
        "ok": True,
        "total": total,
        "avg_length": round(avg_length, 1),
        "by_agent": dict(sorted(by_agent.items(), key=lambda x: -x[1])),
        "by_domain": dict(sorted(by_domain.items(), key=lambda x: -x[1])),
    }


# --- Phase 2.6: Forgetting Protocol ---


RETRACTION_LOG = Path(os.environ.get("ALETHEIA_HOME", "/mnt/ssd/aletheia")) / "shared" / "memory" / "retractions.jsonl"


class RetractRequest(BaseModel):
    query: str
    user_id: str = "default"
    cascade: bool = False
    dry_run: bool = False
    reason: str = ""


@router.post("/retract")
async def retract_memory(req: RetractRequest, request: Request):
    """Atomic retraction across Mem0 vector store + Neo4j graph.

    Finds matching memories, removes them from both stores,
    and logs the retraction for audit trail.
    """
    mem = request.app.state.memory

    # Find memories matching the retraction query
    try:
        raw = await asyncio.to_thread(mem.search, req.query, user_id=req.user_id, limit=20)
        results = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Search failed: {e}")

    if not isinstance(results, list) or not results:
        return {"ok": True, "retracted": 0, "message": "No matching memories found"}

    # Filter to high-confidence matches (score > 0.75)
    to_retract = [r for r in results if r.get("score", 0) > 0.75]

    if not to_retract:
        return {"ok": True, "retracted": 0, "message": "No high-confidence matches (>0.75)"}

    # Neo4j cascade — find and remove connected entities
    neo4j_removed: list[str] = []
    if req.cascade and NEO4J_PASSWORD:
        try:
            from neo4j import GraphDatabase
            driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
            with driver.session() as session:
                for item in to_retract:
                    text = item.get("memory", "")
                    entities = _extract_entities(text)
                    for entity in entities[:5]:
                        # Find and delete connected relationships
                        result = session.run(
                            "MATCH (n)-[r]-(m) "
                            "WHERE toLower(n.name) CONTAINS toLower($name) "
                            "DELETE r "
                            "RETURN count(r) AS deleted, collect(DISTINCT m.name) AS affected",
                            name=entity,
                        )
                        record = result.single()
                        if record and record["deleted"] > 0:
                            neo4j_removed.extend(record["affected"])
                            logger.info(f"Retract cascade: removed {record['deleted']} rels for entity '{entity}'")
            driver.close()
        except Exception:
            logger.warning("retract: Neo4j cascade failed, continuing with vector retraction")

    retracted: list[dict[str, Any]] = []
    if not req.dry_run:
        for item in to_retract:
            memory_id = item.get("id", "")
            if not memory_id:
                continue
            try:
                await asyncio.to_thread(mem.delete, memory_id)
                retracted.append({
                    "id": memory_id,
                    "text": item.get("memory", "")[:200],
                    "score": item.get("score", 0),
                })
            except Exception as e:
                logger.warning(f"Failed to retract {memory_id}: {e}")

        # Audit log
        _log_retraction(req, retracted, neo4j_removed)

    return {
        "ok": True,
        "retracted": len(retracted),
        "dry_run": req.dry_run,
        "items": retracted if not req.dry_run else [
            {"id": r.get("id"), "text": r.get("memory", "")[:200], "score": r.get("score", 0)}
            for r in to_retract
        ],
        "neo4j_cascade": neo4j_removed[:20] if req.cascade else [],
    }


def _log_retraction(req: RetractRequest, retracted: list[dict[str, Any]], neo4j_removed: list[str]) -> None:
    """Append retraction to audit log."""
    RETRACTION_LOG.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "query": req.query,
        "reason": req.reason,
        "user_id": req.user_id,
        "cascade": req.cascade,
        "retracted_ids": [r["id"] for r in retracted],
        "retracted_texts": [r["text"] for r in retracted],
        "neo4j_removed": neo4j_removed[:20],
    }
    with open(RETRACTION_LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")
    logger.info(f"Retraction logged: {len(retracted)} memories, reason={req.reason or 'none'}")


# --- Phase C1: Foresight Signals ---


class ForesightAddRequest(BaseModel):
    entity: str
    signal: str
    activation: str  # ISO datetime
    expiry: str | None = None
    weight: float = Field(default=1.0, ge=0.0, le=10.0)


foresight_router = APIRouter(prefix="/foresight")


@foresight_router.post("/add")
async def add_foresight(req: ForesightAddRequest):
    if not NEO4J_PASSWORD:
        raise HTTPException(status_code=503, detail="Neo4j not configured")

    try:
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
        with driver.session() as session:
            session.run(
                """
                MERGE (e {name: $entity})
                ON CREATE SET e:Entity
                CREATE (f:ForesightSignal {
                    signal: $signal,
                    activation: $activation,
                    expiry: $expiry,
                    weight: $weight,
                    created_at: datetime()
                })
                CREATE (e)-[:HAS_FORESIGHT]->(f)
                """,
                entity=req.entity,
                signal=req.signal,
                activation=req.activation,
                expiry=req.expiry,
                weight=req.weight,
            )
        driver.close()
        return {"ok": True, "entity": req.entity, "signal": req.signal}
    except Exception as e:
        logger.exception("add_foresight failed")
        raise HTTPException(status_code=500, detail=str(e))


@foresight_router.get("/active")
async def active_foresight():
    if not NEO4J_PASSWORD:
        return {"ok": True, "signals": []}

    try:
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
        now = datetime.now(timezone.utc).isoformat()
        with driver.session() as session:
            result = session.run(
                """
                MATCH (e)-[:HAS_FORESIGHT]->(f:ForesightSignal)
                WHERE f.activation <= $now AND (f.expiry IS NULL OR f.expiry >= $now)
                RETURN e.name AS entity, f.signal AS signal, f.activation AS activation,
                       f.expiry AS expiry, f.weight AS weight
                ORDER BY f.weight DESC
                LIMIT 50
                """,
                now=now,
            )
            signals = [
                {
                    "entity": r["entity"],
                    "signal": r["signal"],
                    "activation": str(r["activation"]),
                    "expiry": str(r["expiry"]) if r["expiry"] else None,
                    "weight": r["weight"],
                }
                for r in result
            ]
        driver.close()
        return {"ok": True, "signals": signals}
    except Exception as e:
        logger.exception("active_foresight failed")
        return {"ok": True, "signals": [], "error": str(e)}


@foresight_router.post("/decay")
async def decay_foresight():
    if not NEO4J_PASSWORD:
        return {"ok": True, "decayed": 0, "deleted": 0}

    try:
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
        now = datetime.now(timezone.utc).isoformat()
        with driver.session() as session:
            # Decay expired signals
            decay_result = session.run(
                """
                MATCH (f:ForesightSignal)
                WHERE f.expiry IS NOT NULL AND f.expiry < $now AND f.weight > 0
                SET f.weight = f.weight - 0.1
                RETURN count(f) AS decayed
                """,
                now=now,
            )
            decayed = decay_result.single()["decayed"]

            # Delete fully decayed signals
            delete_result = session.run(
                """
                MATCH (e)-[r:HAS_FORESIGHT]->(f:ForesightSignal)
                WHERE f.weight <= 0
                DELETE r, f
                RETURN count(f) AS deleted
                """
            )
            deleted = delete_result.single()["deleted"]
        driver.close()
        return {"ok": True, "decayed": decayed, "deleted": deleted}
    except Exception as e:
        logger.exception("decay_foresight failed")
        raise HTTPException(status_code=500, detail=str(e))


# --- Phase C2: Autonomous Link Generation (A-Mem Pattern) ---


LINK_GENERATION_ENABLED = os.environ.get("LINK_GENERATION_ENABLED", "false").lower() == "true"
ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
LINK_SCORE_THRESHOLD = 0.6
LINK_MAX_NEIGHBORS = 3


async def _generate_links(mem: Any, new_text: str, user_id: str) -> list[dict[str, Any]]:
    """Generate LLM-described links between a new memory and its nearest neighbors."""
    if not LINK_GENERATION_ENABLED or not ANTHROPIC_API_KEY or not NEO4J_PASSWORD:
        return []

    # Find nearest neighbors
    try:
        raw = await asyncio.to_thread(mem.search, new_text, user_id=user_id, limit=LINK_MAX_NEIGHBORS + 1)
        results = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception:
        return []

    if not isinstance(results, list):
        return []

    # Filter to high-similarity neighbors (skip self)
    neighbors = [
        r for r in results
        if r.get("score", 0) > LINK_SCORE_THRESHOLD
        and r.get("memory", "") != new_text
    ][:LINK_MAX_NEIGHBORS]

    if not neighbors:
        return []

    links: list[dict[str, Any]] = []

    async with httpx.AsyncClient(timeout=10.0) as client:
        for neighbor in neighbors:
            neighbor_text = neighbor.get("memory", "")
            if not neighbor_text:
                continue

            try:
                resp = await client.post(
                    "https://api.anthropic.com/v1/messages",
                    headers={
                        "x-api-key": ANTHROPIC_API_KEY,
                        "anthropic-version": "2023-06-01",
                        "content-type": "application/json",
                    },
                    json={
                        "model": "claude-haiku-4-5-20251001",
                        "max_tokens": 64,
                        "messages": [
                            {
                                "role": "user",
                                "content": (
                                    f'Memory A: "{new_text[:200]}"\n'
                                    f'Memory B: "{neighbor_text[:200]}"\n'
                                    "Describe the relationship between A and B in 10 words or less."
                                ),
                            }
                        ],
                    },
                )
                if resp.status_code != 200:
                    continue

                data = resp.json()
                description = data.get("content", [{}])[0].get("text", "").strip()
                if not description or len(description) > 100:
                    continue

                links.append({
                    "neighbor_id": neighbor.get("id", ""),
                    "neighbor_text": neighbor_text[:200],
                    "description": description,
                    "score": neighbor.get("score", 0),
                })
            except Exception:
                continue

    # Store links in Neo4j
    if links:
        try:
            from neo4j import GraphDatabase

            driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
            with driver.session() as session:
                for link in links:
                    session.run(
                        """
                        MERGE (a:Memory {text_preview: $new_text})
                        MERGE (b:Memory {text_preview: $neighbor_text})
                        CREATE (a)-[:LINKED {
                            description: $description,
                            score: $score,
                            generated_at: datetime()
                        }]->(b)
                        """,
                        new_text=new_text[:200],
                        neighbor_text=link["neighbor_text"],
                        description=link["description"],
                        score=link["score"],
                    )
            driver.close()
            logger.info(f"Generated {len(links)} memory links for new memory")
        except Exception:
            logger.warning("Failed to store memory links in Neo4j")

    return links


# --- Graph Analytics (networkx, since Community Neo4j lacks GDS) ---


class GraphAnalyzeRequest(BaseModel):
    top_k: int = Field(default=20, ge=5, le=100)
    store_scores: bool = True


@router.post("/graph/analyze")
async def analyze_graph(req: GraphAnalyzeRequest):
    """Run PageRank + community detection on the Neo4j graph via networkx.

    Scores are optionally written back as node properties for retrieval weighting.
    Intended to be called from the nightly consolidation cron.
    """
    if not NEO4J_PASSWORD:
        raise HTTPException(status_code=503, detail="Neo4j not configured")

    try:
        import networkx as nx
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))

        # Export graph to networkx
        G = nx.DiGraph()
        with driver.session() as session:
            nodes = session.run("MATCH (n) WHERE n.name IS NOT NULL RETURN n.name AS name, labels(n) AS labels")
            for record in nodes:
                G.add_node(record["name"], labels=record["labels"])

            rels = session.run(
                "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                "RETURN a.name AS src, b.name AS dst, type(r) AS rel_type"
            )
            for record in rels:
                G.add_edge(record["src"], record["dst"], rel_type=record["rel_type"])

        if G.number_of_nodes() == 0:
            driver.close()
            return {"ok": True, "nodes": 0, "message": "Empty graph"}

        # PageRank
        pagerank = nx.pagerank(G, alpha=0.85, max_iter=100)
        top_pagerank = sorted(pagerank.items(), key=lambda x: -x[1])[:req.top_k]

        # Community detection (Louvain on undirected projection)
        G_undirected = G.to_undirected()
        try:
            communities = nx.community.louvain_communities(G_undirected, seed=42)
            community_map: dict[str, int] = {}
            for idx, community in enumerate(communities):
                for node in community:
                    community_map[node] = idx
            num_communities = len(communities)
            largest_communities = sorted(communities, key=len, reverse=True)[:5]
            community_summaries = [
                {"id": idx, "size": len(c), "sample": sorted(c)[:5]}
                for idx, c in enumerate(largest_communities)
            ]
        except Exception:
            community_map = {}
            num_communities = 0
            community_summaries = []

        # Node similarity — find dedup candidates (nodes with >0.8 Jaccard on neighbors)
        dedup_candidates: list[dict[str, Any]] = []
        nodes_list = list(G_undirected.nodes())
        for i in range(min(len(nodes_list), 200)):
            n1 = nodes_list[i]
            neighbors1 = set(G_undirected.neighbors(n1))
            if not neighbors1:
                continue
            for j in range(i + 1, min(len(nodes_list), 200)):
                n2 = nodes_list[j]
                neighbors2 = set(G_undirected.neighbors(n2))
                if not neighbors2:
                    continue
                jaccard = len(neighbors1 & neighbors2) / len(neighbors1 | neighbors2)
                if jaccard > 0.8:
                    dedup_candidates.append({
                        "node_a": n1,
                        "node_b": n2,
                        "jaccard": round(jaccard, 3),
                        "shared_neighbors": len(neighbors1 & neighbors2),
                    })
        dedup_candidates.sort(key=lambda x: -x["jaccard"])

        # Store scores back to Neo4j
        scores_stored = 0
        if req.store_scores:
            with driver.session() as session:
                for name, score in top_pagerank:
                    community_id = community_map.get(name, -1)
                    session.run(
                        "MATCH (n {name: $name}) "
                        "SET n.pagerank = $score, n.community = $community",
                        name=name, score=round(score, 6), community=community_id,
                    )
                    scores_stored += 1

        driver.close()

        return {
            "ok": True,
            "nodes": G.number_of_nodes(),
            "edges": G.number_of_edges(),
            "pagerank_top": [{"name": n, "score": round(s, 6)} for n, s in top_pagerank],
            "communities": num_communities,
            "community_summaries": community_summaries,
            "dedup_candidates": dedup_candidates[:10],
            "scores_stored": scores_stored,
        }
    except ImportError:
        raise HTTPException(status_code=500, detail="networkx not installed")
    except Exception as e:
        logger.exception("graph/analyze failed")
        raise HTTPException(status_code=500, detail=str(e))


# --- Enhanced Search with Query Rewriting ---


class EnhancedSearchRequest(BaseModel):
    query: str
    user_id: str = "default"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)
    rewrite: bool = True


@router.post("/search_enhanced")
async def search_enhanced(req: EnhancedSearchRequest, request: Request):
    """Search with entity alias resolution and LLM-generated query variants.

    Pipeline:
    1. Extract entities from query
    2. Resolve aliases via Neo4j (find canonical names)
    3. Generate 2-3 alternate phrasings via Haiku
    4. Run parallel vector searches on all variants
    5. Merge and deduplicate results
    """
    mem = request.app.state.memory

    # Skip rewriting for very short or very long queries
    if not req.rewrite or len(req.query) < 10 or len(req.query) > 500:
        return await _simple_search(mem, req)

    # Step 1: Extract entities
    entities = _extract_entities(req.query)

    # Step 2: Resolve aliases via Neo4j
    canonical_names: dict[str, str] = {}
    if entities and NEO4J_PASSWORD:
        try:
            from neo4j import GraphDatabase
            driver = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))
            with driver.session() as session:
                for entity in entities[:5]:
                    result = session.run(
                        "MATCH (n) WHERE toLower(n.name) CONTAINS toLower($name) "
                        "RETURN n.name AS canonical ORDER BY size(n.name) LIMIT 1",
                        name=entity,
                    )
                    record = result.single()
                    if record and record["canonical"] != entity:
                        canonical_names[entity] = record["canonical"]
            driver.close()
        except Exception:
            logger.warning("search_enhanced: Neo4j alias resolution failed")

    # Build alias-resolved query
    resolved_query = req.query
    for original, canonical in canonical_names.items():
        resolved_query = resolved_query.replace(original, canonical)

    # Step 3: Generate alternate phrasings via Haiku
    query_variants = [req.query]
    if resolved_query != req.query:
        query_variants.append(resolved_query)

    if ANTHROPIC_API_KEY:
        try:
            async with httpx.AsyncClient(timeout=8.0) as client:
                resp = await client.post(
                    "https://api.anthropic.com/v1/messages",
                    headers={
                        "x-api-key": ANTHROPIC_API_KEY,
                        "anthropic-version": "2023-06-01",
                        "content-type": "application/json",
                    },
                    json={
                        "model": "claude-haiku-4-5-20251001",
                        "max_tokens": 128,
                        "messages": [{
                            "role": "user",
                            "content": (
                                f'Rewrite this search query 2 different ways to find the same information. '
                                f'Return ONLY the 2 variants, one per line, no numbering.\n\n'
                                f'Query: "{req.query}"'
                            ),
                        }],
                    },
                )
                if resp.status_code == 200:
                    data = resp.json()
                    text = data.get("content", [{}])[0].get("text", "")
                    for line in text.strip().split("\n"):
                        line = line.strip().strip('"').strip("- ")
                        if line and len(line) > 5 and line != req.query:
                            query_variants.append(line)
        except Exception:
            logger.warning("search_enhanced: query rewriting failed")

    # Step 4: Parallel vector searches
    search_kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        search_kwargs["agent_id"] = req.agent_id

    async def do_search(query: str) -> list[dict[str, Any]]:
        try:
            raw = await asyncio.to_thread(mem.search, query, **search_kwargs)
            results = raw.get("results", raw) if isinstance(raw, dict) else raw
            return results if isinstance(results, list) else []
        except Exception:
            return []

    all_results = await asyncio.gather(*[do_search(q) for q in query_variants[:4]])

    # Step 5: Merge and deduplicate
    seen_ids: set[str] = set()
    merged: list[dict[str, Any]] = []
    for variant_results in all_results:
        for r in variant_results:
            rid = r.get("id", r.get("hash", str(r.get("memory", ""))))
            if rid not in seen_ids:
                seen_ids.add(rid)
                merged.append(r)

    merged.sort(key=lambda r: r.get("score", 0), reverse=True)

    return {
        "ok": True,
        "results": merged[:req.limit],
        "query_variants": query_variants[:4],
        "aliases_resolved": canonical_names,
        "total_candidates": len(merged),
    }


async def _simple_search(mem: Any, req: EnhancedSearchRequest) -> dict[str, Any]:
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id
    try:
        raw = await asyncio.to_thread(mem.search, req.query, **kwargs)
        results = raw.get("results", raw) if isinstance(raw, dict) else raw
        return {
            "ok": True,
            "results": results if isinstance(results, list) else [],
            "query_variants": [req.query],
            "aliases_resolved": {},
            "total_candidates": len(results) if isinstance(results, list) else 0,
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


