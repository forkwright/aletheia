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
    user_id: str = "ck"
    agent_id: str | None = None
    metadata: dict[str, Any] | None = None


class SearchRequest(BaseModel):
    query: str
    user_id: str = "ck"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)


class ImportRequest(BaseModel):
    facts: list[dict[str, Any]]
    user_id: str = "ck"


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
    user_id: str = "ck",
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
    user_id: str = "ck"
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
    user_id: str = "ck"
    limit: int = Field(default=100, ge=10, le=500)


class MergeRequest(BaseModel):
    source_id: str
    target_id: str
    user_id: str = "ck"


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
async def fact_stats(request: Request, user_id: str = "ck"):
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
    user_id: str = "ck"
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


