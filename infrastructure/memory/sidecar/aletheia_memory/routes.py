# API routes for Aletheia memory sidecar
# NOTE: Do NOT add 'from __future__ import annotations' here.
# It causes intermittent TypeError in FastAPI's dependency injection
# when routes accept both a Pydantic model and Request parameter.
# Python 3.12+ supports all modern type syntax natively.

import asyncio
import hashlib
import json
import logging
import math
import os
import re
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, LiteralString, cast

import httpx
import neo4j as _neo4j
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field
from qdrant_client import QdrantClient
from qdrant_client.models import FieldCondition, Filter, MatchValue, PointStruct

from .config import LLM_BACKEND, QDRANT_HOST, QDRANT_PORT
from .entity_resolver import (
    cleanup_orphan_entities,
    get_canonical_entities,
    merge_duplicate_entities,
    resolve_entity,
)
from .evolution import exponential_decay_penalty
from .graph import mark_neo4j_down, mark_neo4j_ok, neo4j_available, neo4j_driver
from .graph_extraction import extract_graph, extract_graph_batch


def _extract_entities_for_episode(text: str) -> list[str]:
    """Extract capitalized multi-word entity names from text for episode linking."""
    return re.findall(r'\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b', text)

logger = logging.getLogger("aletheia_memory")
router = APIRouter()


def _extract_results(raw: Any, key: str = "results") -> Any:
    """Unwrap Mem0 response: if dict with key, return that value; else return raw."""
    if isinstance(raw, dict):
        d: dict[str, Any] = cast('dict[str, Any]', raw)
        return d.get(key, raw)
    return raw


def _as_result_list(raw: Any) -> list[dict[str, Any]]:
    """Normalize a Mem0 response to a typed list of result dicts."""
    unwrapped: Any = _extract_results(raw)
    return cast('list[dict[str, Any]]', unwrapped) if isinstance(unwrapped, list) else []


def _neo4j_session(driver: _neo4j.Driver) -> _neo4j.Session:
    """Create a typed neo4j session (works around neo4j stub's **config: Unknown)."""
    factory: Any = getattr(driver, "session")  # noqa: B009 — bypasses neo4j stub's partially unknown type
    return cast('_neo4j.Session', factory())


def _cypher(query: str) -> LiteralString:
    """Cast a dynamically built Cypher string to LiteralString for neo4j Query()."""
    return cast('LiteralString', query)


_background_tasks: set[asyncio.Task[Any]] = set()  # prevent GC of fire-and-forget tasks


def _get_memory(request: Request) -> Any:
    """Safely retrieve Memory instance from app state."""
    mem: Any = getattr(request.app.state, "memory", None)
    if mem is None:
        raise HTTPException(status_code=503, detail="Memory not initialized")
    return mem


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
    domains: list[str] | None = None


class ImportRequest(BaseModel):
    facts: list[dict[str, Any]]
    user_id: str = "default"


class AddDirectRequest(BaseModel):
    """Store a pre-extracted fact directly — bypass Mem0 LLM extraction."""
    text: str
    user_id: str = "default"
    agent_id: str | None = None
    source: str = "direct"
    session_id: str | None = None
    confidence: float = Field(default=0.8, ge=0.0, le=1.0)


class AddBatchRequest(BaseModel):
    """Store multiple pre-extracted facts directly — bypass Mem0 LLM extraction."""
    texts: list[str]
    user_id: str = "default"
    agent_id: str | None = None
    source: str = "distillation"
    session_id: str | None = None
    confidence: float = Field(default=0.8, ge=0.0, le=1.0)


class DeduplicateRequest(BaseModel):
    """Deduplicate a batch of texts using in-memory pairwise cosine similarity."""
    texts: list[str]
    user_id: str = "default"
    threshold: float = Field(default=0.90, ge=0.5, le=1.0)


DEDUP_THRESHOLD = 0.85
DIRECT_DEDUP_THRESHOLD = 0.90  # Higher threshold for pre-extracted facts (more specific)
COLLECTION_NAME = "aletheia_memories"

# Recall-time noise patterns — compiled once at module level for performance.
# Mirrors the TypeScript NOISE_PATTERNS in melete/extract.ts.
# Applied as a soft downrank (0.3x score penalty) — noisy results remain in output.
_RECALL_NOISE_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"(?i)(session|conversation|chat)\s+(id|started|ended|created)"),
    re.compile(r"(?i)(the user|the agent|the assistant)\s+(asked|told|said|mentioned)"),
    re.compile(r"(?i)(called|invoked|ran|executed)\s+(tool|function|command|script)\b"),
    re.compile(r"(?i)^(sure|ok|okay|got it|understood|will do|no problem|sounds good)\b"),
]
_RECALL_NOISE_MIN_LENGTH = 15  # Memories shorter than this are considered noise fragments

# Stop words excluded from domain relevance token matching — common words add noise.
_DOMAIN_STOP_WORDS: frozenset[str] = frozenset({
    "the", "a", "an", "is", "in", "of", "to", "and", "or", "for",
    "it", "was", "be", "has", "had", "not", "are", "but", "at", "on",
    "with", "this", "that", "from", "by",
})


def _domain_relevance_score(memory_text: str, query_context: str, min_factor: float = 0.6) -> float:
    """Compute token-level Jaccard overlap between memory text and query context.

    Returns a score multiplier in [min_factor, 1.0]. Cross-domain results are
    penalized by up to (1 - min_factor) but never excluded (soft boundaries).

    Args:
        memory_text: The memory content to score.
        query_context: The full query string, typically including "Thread context: ..."
        min_factor: Minimum multiplier — default 0.6 means worst penalty is 40%.
    """
    def tokenize(text: str) -> frozenset[str]:
        tokens = set(text.lower().split())
        return frozenset(tokens - _DOMAIN_STOP_WORDS)

    query_tokens = tokenize(query_context)
    if not query_tokens:
        return 1.0

    memory_tokens = tokenize(memory_text)
    overlap = len(query_tokens & memory_tokens) / len(query_tokens)
    return max(min_factor, min(1.0, 0.6 + overlap * 0.4))


def _apply_domain_reranking(results: list[dict[str, Any]], query: str) -> list[dict[str, Any]]:
    """Re-rank results by domain relevance using token Jaccard overlap against query context.

    Only applies when the query contains context (Thread context: marker present).
    Skipped for bare queries with no domain signal — avoids spurious penalization.
    Modifies scores in-place and re-sorts descending.
    """
    if "Thread context:" not in query and "thread context:" not in query.lower():
        return results

    for r in results:
        memory_text: str = str(r.get("memory") or r.get("data") or "").strip()
        factor = _domain_relevance_score(memory_text, query)
        r["score"] = (r.get("score") or 0.0) * factor

    results.sort(key=lambda r: r.get("score", 0), reverse=True)
    return results


@router.post("/add")
async def add_memory(req: AddRequest, request: Request) -> dict[str, Any]:
    # NOTE: metadata enforcement (session_id, agent_id) is deferred for this route.
    # Reason: /add is the Mem0 path. Traffic analysis is needed to confirm whether
    # this route is still used in production before enforcing required fields.
    # If /add is still active, it may produce orphans missing required metadata.
    # See STATE.md blocker: "need traffic trace to confirm /add route usage".
    # Enforcement is tracked in Phase 2 data integrity work.
    mem = _get_memory(request)
    kwargs: dict[str, Any] = {"user_id": req.user_id}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id
    if req.metadata:
        kwargs["metadata"] = req.metadata

    try:
        # Cross-agent dedup: search globally (no agent_id) before adding
        existing: Any = await asyncio.to_thread(mem.search, req.text, user_id=req.user_id, limit=3)
        results_list: list[dict[str, Any]] = _as_result_list(existing)
        for candidate in results_list:
            top: dict[str, Any] = candidate
            score: float = top.get("score", 0)
            if score > DEDUP_THRESHOLD:
                safe_agent = (req.agent_id or "global").replace("\n", "").replace("\r", "")[:50]
                logger.info(
                    f"Dedup: skipped (score={score:.3f}, existing={top.get('id', '?')}, "
                    f"agent={safe_agent})"
                )
                return {"ok": True, "result": {"deduplicated": True, "existing_id": top.get("id"), "score": score}}

        # Tier 3 (no LLM): skip extraction, just store raw text as embedding
        backend: dict[str, Any] = getattr(request.app.state, "backend", LLM_BACKEND)
        if backend.get("tier", 1) >= 3:
            logger.info("Tier 3: storing text as embedding only (no fact extraction)")
            # Use Mem0's vector store directly for embedding-only storage
            try:
                import uuid as _uuid

                from qdrant_client import QdrantClient
                from qdrant_client.models import PointStruct
                embedder: Any = mem.embedding_model
                vector: Any = await asyncio.to_thread(embedder.embed, req.text)
                point_id = str(_uuid.uuid4())
                payload: dict[str, Any] = {
                    "memory": req.text[:500],
                    "data": req.text,
                    "user_id": req.user_id,
                    "agent_id": req.agent_id,
                    "created_at": datetime.now(UTC).isoformat(),
                    **(req.metadata or {}),
                }
                qclient = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
                qclient.upsert(
                    collection_name="aletheia_memories",
                    points=[PointStruct(id=point_id, vector=vector, payload=payload)],
                )
                return {"ok": True, "result": {"tier3_embed_only": True, "id": point_id}}
            except Exception as e:
                logger.exception("Tier 3 embedding failed")
                raise HTTPException(status_code=500, detail=str(e)) from e

        result: Any = await asyncio.to_thread(mem.add, req.text, **kwargs)
        graph_degraded = False

        # Autonomous link generation (A-Mem pattern) — fire and forget
        if LINK_GENERATION_ENABLED:
            link_task: asyncio.Task[Any] = asyncio.create_task(_generate_links(mem, req.text, req.user_id))
            _background_tasks.add(link_task)
            link_task.add_done_callback(_background_tasks.discard)

        # Episode tracking — record this interaction as a temporal episode
        if neo4j_available() and req.agent_id:
            ep_task: asyncio.Task[Any] = asyncio.create_task(_record_episode(req.text, req.agent_id, req.metadata))
            _background_tasks.add(ep_task)
            ep_task.add_done_callback(_background_tasks.discard)

        # Graph extraction via SimpleKGPipeline — fire and forget
        graph_task: asyncio.Task[Any] = asyncio.create_task(extract_graph(req.text, backend=backend))
        _background_tasks.add(graph_task)
        graph_task.add_done_callback(_background_tasks.discard)

        return {"ok": True, "result": result, **({"graph_degraded": True} if graph_degraded else {})}
    except Exception as e:
        # Neo4j failure during mem.add — vector write likely succeeded but graph write failed.
        # Mem0 processes vector first, so partial success is common when Neo4j is down.
        err_str = str(e).lower()
        if "neo4j" in err_str or "ServiceUnavailable" in str(type(e).__name__) or "connection" in err_str:
            mark_neo4j_down()
            logger.warning("Neo4j failed during add, vector portion likely saved: %s", e)
            return {"ok": True, "result": {"graph_degraded": True}, "warning": "Neo4j unavailable, stored vector only"}
        logger.exception("add_memory failed")
        raise HTTPException(status_code=500, detail="Internal server error") from None


def _apply_recency_boost(results: list[dict[str, Any]], max_boost: float = 0.15, window_hours: float = 24.0) -> list[dict[str, Any]]:
    """Boost scores for recently created memories (linear decay over window)."""
    from datetime import datetime
    now = datetime.now(UTC)
    for r in results:
        meta_dict: dict[str, Any] = r.get("metadata") or {}
        created: Any = r.get("created_at") or meta_dict.get("created_at")
        if not created:
            continue
        try:
            if isinstance(created, str):
                dt = datetime.fromisoformat(created.replace("Z", "+00:00"))
            else:
                continue
            age_hours = (now - dt).total_seconds() / 3600
            if age_hours < window_hours:
                boost = max_boost * (1.0 - age_hours / window_hours)
                r["score"] = (r.get("score") or 0.0) + boost
        except (ValueError, TypeError):
            pass
    results.sort(key=lambda r: r.get("score", 0), reverse=True)
    return results


def _apply_confidence_weight(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Apply exponential decay and access boost to search scores.

    Decay is time-based (days since last_accessed) using exponential formula with
    ~14-day half-life (lambda=0.05). Memories never accessed have full salience.
    Access boost applies a small bonus for frequently reinforced memories.
    """
    if not neo4j_available() or not results:
        return results
    memory_ids: list[Any] = [r.get("id") for r in results if r.get("id")]
    if not memory_ids:
        return results
    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            records: list[dict[str, Any]] = session.run(
                "UNWIND $ids AS mid "
                "OPTIONAL MATCH (m:MemoryAccess {memory_id: mid}) "
                "RETURN mid, m.access_count AS accesses, m.last_accessed AS last_accessed",
                ids=memory_ids,
            ).data()
        driver.close()
        mark_neo4j_ok()
        now = datetime.now(UTC)
        access_map: dict[str, tuple[int, str | None]] = {}
        for rec in records:
            access_map[rec["mid"]] = (rec.get("accesses") or 0, rec.get("last_accessed"))
        for r in results:
            mid: str | None = r.get("id")
            if mid and mid in access_map:
                accesses, last_accessed = access_map[mid]
                # Exponential decay: penalize based on days since last access
                # New memories (no last_accessed) receive no penalty — full salience
                if last_accessed is not None:
                    try:
                        last_dt = datetime.fromisoformat(last_accessed)
                        if last_dt.tzinfo is None:
                            last_dt = last_dt.replace(tzinfo=UTC)
                        days_inactive = (now - last_dt).total_seconds() / 86400.0
                        multiplier = exponential_decay_penalty(days_inactive)
                        r["score"] = (r.get("score") or 0.0) * multiplier
                    except (ValueError, TypeError):
                        pass  # Unparseable timestamp — skip decay for this entry
                # Access boost: small bonus for frequently reinforced memories
                if accesses > 2:
                    boost = min(0.05, accesses * 0.01)
                    r["score"] = (r.get("score") or 0.0) + boost
        results.sort(key=lambda r: r.get("score", 0), reverse=True)
    except Exception as e:
        mark_neo4j_down()
        logger.warning("confidence weighting failed: %s", e)
    return results


def _filter_noisy_results(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Apply soft noise penalty to low-quality recall results.

    Noisy results receive a 0.3x score multiplier (pushed down in ranking) but
    are NOT removed — soft boundaries per design. Results are re-sorted after
    penalty application.
    """
    penalized = 0
    for r in results:
        memory_text: str = str(r.get("memory") or r.get("data") or "").strip()
        is_noisy = (
            len(memory_text) < _RECALL_NOISE_MIN_LENGTH
            or any(p.search(memory_text) for p in _RECALL_NOISE_PATTERNS)
        )
        if is_noisy:
            r["score"] = (r.get("score") or 0.0) * 0.3
            penalized += 1
    if penalized:
        logger.debug("recall noise filter: penalized %d result(s)", penalized)
        results.sort(key=lambda r: r.get("score", 0), reverse=True)
    return results


# ---------------------------------------------------------------------------
# Direct storage endpoints — bypass Mem0 LLM extraction
# These accept pre-extracted facts and embed + store them directly in Qdrant.
# ---------------------------------------------------------------------------

def _content_hash(text: str) -> str:
    """Deterministic hash for content dedup."""
    return hashlib.md5(text.strip().lower().encode()).hexdigest()


async def _embed_texts(mem: Any, texts: list[str]) -> list[list[float]]:
    """Embed a list of texts using the Mem0 embedding model."""
    embedder: Any = mem.embedding_model
    vectors: list[list[float]] = []
    for text in texts:
        vec: list[float] = await asyncio.to_thread(embedder.embed, text)
        vectors.append(vec)
    return vectors


async def _semantic_dedup_check(
    client: QdrantClient,
    vector: list[float],
    user_id: str,
    threshold: float = DIRECT_DEDUP_THRESHOLD,
) -> bool:
    """Check if a semantically similar memory already exists. Returns True if duplicate."""
    try:
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=vector,
            query_filter=Filter(
                must=[FieldCondition(key="user_id", match=MatchValue(value=user_id))]
            ),
            limit=1,
            with_payload=False,
        )
        if results.points and results.points[0].score >= threshold:
            return True
    except Exception as e:
        logger.warning("Semantic dedup check failed (proceeding with add): %s", e)
    return False


def _cosine_similarity(a: list[float], b: list[float]) -> float:
    """Cosine similarity between two equal-length vectors. Returns 0.0 on zero-magnitude input."""
    dot = sum(x * y for x, y in zip(a, b, strict=False))
    mag_a = math.sqrt(sum(x * x for x in a))
    mag_b = math.sqrt(sum(x * x for x in b))
    if mag_a == 0.0 or mag_b == 0.0:
        return 0.0
    return dot / (mag_a * mag_b)


@router.post("/dedup/batch")
async def dedup_batch(req: DeduplicateRequest, request: Request) -> dict[str, Any]:
    """Deduplicate a batch of texts using in-memory pairwise cosine similarity.

    Embeds all submitted texts and removes near-duplicates using greedy clustering:
    each text is kept only if it has cosine similarity < threshold against all
    already-kept texts. Does NOT query Qdrant — purely in-memory comparison of
    the submitted batch.
    """
    mem = _get_memory(request)
    texts = [t.strip() for t in req.texts if t.strip()]
    if not texts:
        return {"deduplicated": [], "removed": 0}

    vectors = await _embed_texts(mem, texts)

    kept_texts: list[str] = []
    kept_vectors: list[list[float]] = []

    for text, vector in zip(texts, vectors, strict=False):
        is_duplicate = any(
            _cosine_similarity(vector, kv) >= req.threshold
            for kv in kept_vectors
        )
        if not is_duplicate:
            kept_texts.append(text)
            kept_vectors.append(vector)

    removed = len(texts) - len(kept_texts)
    if removed > 0:
        logger.info(f"dedup_batch: removed {removed} near-duplicate(s) from {len(texts)} texts (threshold={req.threshold})")

    return {"deduplicated": kept_texts, "removed": removed}


# NOTE: /add_direct is defined below (Phase 5) with contradiction detection + entity resolution


@router.post("/add_batch")
async def add_batch(req: AddBatchRequest, request: Request) -> dict[str, Any]:
    """Store multiple pre-extracted facts directly in Qdrant.

    Same as /add_direct but batched for efficiency. Used by distillation
    and reflection pipelines to flush extracted facts.

    Requires agent_id and session_id — requests missing either are rejected
    with 400 to prevent creation of orphaned Qdrant entries.

    NOTE: The aletheia.ts memory flush path (addMemories) does not currently
    pass session_id. That caller must be updated to include session_id before
    this endpoint is safe to call from that path.

    EXTR-06 (infer=False): This path structurally bypasses mem.add() entirely.
    Facts are written directly to Qdrant via client.upsert() — Mem0's LLM
    extraction is never invoked. The infer=False requirement is satisfied
    architecturally, not via a parameter.
    """
    missing = [f for f in ("agent_id", "session_id") if not getattr(req, f, None)]
    if missing:
        raise HTTPException(
            status_code=400,
            detail=f"Required field(s) missing: {', '.join(missing)}",
        )

    mem = _get_memory(request)
    texts = [t.strip() for t in req.texts if t.strip()]
    if not texts:
        return {"ok": True, "added": 0, "skipped": 0, "errors": 0}

    try:
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
        now = datetime.now(UTC).isoformat()

        # Content hash dedup — batch check existing hashes
        hashes = {_content_hash(t): t for t in texts}
        existing_hashes: set[str] = set()
        for h in hashes:
            results = client.scroll(
                collection_name=COLLECTION_NAME,
                scroll_filter=Filter(
                    must=[
                        FieldCondition(key="hash", match=MatchValue(value=h)),
                        FieldCondition(key="user_id", match=MatchValue(value=req.user_id)),
                    ]
                ),
                limit=1,
                with_payload=False,
                with_vectors=False,
            )
            if results[0]:
                existing_hashes.add(h)

        # Filter to new-only
        new_texts: list[str] = []
        new_hashes: list[str] = []
        for h, t in hashes.items():
            if h not in existing_hashes:
                new_texts.append(t)
                new_hashes.append(h)

        if not new_texts:
            return {"ok": True, "added": 0, "skipped": len(texts), "errors": 0}

        # Embed all new texts
        vectors = await _embed_texts(mem, new_texts)

        # Pre-fetch canonical entities for resolution
        canonical_list = await asyncio.to_thread(get_canonical_entities)

        # Semantic dedup + contradiction check + entity resolution + build points
        points: list[PointStruct] = []
        skipped_semantic = 0
        all_contradictions: list[dict[str, Any]] = []
        for text, vector, content_hash in zip(new_texts, vectors, new_hashes, strict=False):
            if await _semantic_dedup_check(client, vector, req.user_id):
                skipped_semantic += 1
                continue

            # Contradiction detection
            contradictions = await _check_contradictions(client, vector, text, req.user_id)
            if contradictions:
                all_contradictions.extend(contradictions)

            # Entity resolution
            entities = _extract_entities(text)
            resolved_entities: list[str] = []
            for entity in entities[:5]:
                resolved = resolve_entity(entity, canonical_list)
                if resolved:
                    resolved_entities.append(resolved)

            payload: dict[str, Any] = {
                "memory": text[:500],
                "data": text,
                "source": req.source,
                "hash": content_hash,
                "user_id": req.user_id,
                "created_at": now,
                "confidence": req.confidence,
            }
            if req.agent_id:
                payload["agent_id"] = req.agent_id
            if req.session_id:
                payload["session_id"] = req.session_id
            if resolved_entities:
                payload["entities"] = resolved_entities
            if contradictions:
                payload["contradicts"] = [c["id"] for c in contradictions]

            points.append(PointStruct(
                id=str(uuid.uuid4()),
                vector=vector,
                payload=payload,
            ))

        # Upsert in batches of 100
        added = 0
        errors = 0
        for i in range(0, len(points), 100):
            batch: list[PointStruct] = points[i : i + 100]
            try:
                client.upsert(collection_name=COLLECTION_NAME, points=batch)
                added += len(batch)
            except Exception as e:
                logger.error(f"add_batch upsert failed for batch {i}: {e}")
                errors += len(batch)

        total_skipped = len(existing_hashes) + skipped_semantic
        logger.info(
            f"add_batch: {added} added, {total_skipped} skipped "
            f"({len(existing_hashes)} hash, {skipped_semantic} semantic), "
            f"{errors} errors (source={req.source}, agent={req.agent_id})"
        )

        # Graph extraction via SimpleKGPipeline — fire and forget
        if added > 0 and new_texts:
            backend: dict[str, Any] = getattr(request.app.state, "backend", LLM_BACKEND)
            graph_task: asyncio.Task[Any] = asyncio.create_task(extract_graph_batch(new_texts, backend=backend))
            _background_tasks.add(graph_task)
            graph_task.add_done_callback(_background_tasks.discard)

        result: dict[str, Any] = {"ok": True, "added": added, "skipped": total_skipped, "errors": errors}
        if all_contradictions:
            result["contradictions"] = all_contradictions[:10]  # Cap at 10 for response size
        return result

    except Exception as e:
        logger.exception("add_batch failed")
        raise HTTPException(status_code=500, detail=str(e)) from e


@router.post("/search")
async def search_memory(req: SearchRequest, request: Request) -> dict[str, Any]:
    mem = _get_memory(request)
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        raw: Any = await asyncio.to_thread(mem.search, req.query, **kwargs)
        results_list: list[dict[str, Any]] = _as_result_list(raw)
        for r in results_list:
            meta: dict[str, Any] = r.get("metadata") or {}
            if "created_at" in meta:
                r["created_at"] = meta["created_at"]
        # Exclude forgotten and superseded memories
        results_list = [
            r for r in results_list
            if not cast('dict[str, Any]', r.get("metadata") or r).get("forgotten")
            and not cast('dict[str, Any]', r.get("metadata") or r).get("superseded_by")
        ]
        if req.domains:
            allowed = set(req.domains)
            results_list = [
                r for r in results_list
                if not cast('dict[str, Any]', r.get("metadata") or {}).get("domain")
                or cast('dict[str, Any]', r.get("metadata") or {}).get("domain") in allowed
            ]
        results_list = _apply_recency_boost(results_list)
        results_list = _apply_confidence_weight(results_list)
        results_list = _filter_noisy_results(results_list)
        results_list = _apply_domain_reranking(results_list, req.query)
        return {"ok": True, "results": results_list}
    except Exception:
        logger.exception("search_memory failed")
        raise HTTPException(status_code=500, detail="Internal server error") from None


@router.post("/graph_search")
async def graph_search_post(req: SearchRequest, request: Request) -> dict[str, Any]:
    mem = _get_memory(request)
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": req.limit}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        raw_results: Any = await asyncio.to_thread(mem.search, req.query, **kwargs)
        all_results: list[dict[str, Any]] = _as_result_list(raw_results)
        graph_results: list[dict[str, Any]] = [r for r in all_results if r.get("source") == "graph"]
        return {"ok": True, "results": graph_results}
    except Exception:
        logger.exception("graph_search failed")
        raise HTTPException(status_code=500, detail="Internal server error") from None


@router.post("/import")
async def import_facts(req: ImportRequest, request: Request) -> dict[str, Any]:
    mem = _get_memory(request)
    imported = 0
    errors: list[dict[str, Any]] = []

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
            logger.debug("facts import error at index %d: %s", i, e)
            errors.append({"index": i, "error": "Import failed"})
            if len(errors) > 10:
                break

    return {"ok": True, "imported": imported, "errors": errors}


@router.get("/memories")
async def list_memories(
    request: Request,
    user_id: str = "default",
    agent_id: str | None = None,
    limit: int = 50,
) -> dict[str, Any]:
    mem = _get_memory(request)
    kwargs: dict[str, Any] = {"user_id": user_id}
    if agent_id:
        kwargs["agent_id"] = agent_id

    try:
        kwargs["limit"] = limit
        raw: Any = await asyncio.to_thread(mem.get_all, **kwargs)
        entries: list[dict[str, Any]] = _as_result_list(raw)
        return {"ok": True, "memories": entries}
    except Exception:
        logger.exception("list_memories failed")
        raise HTTPException(status_code=500, detail="Internal server error") from None


@router.delete("/memories/{memory_id}")
async def delete_memory(memory_id: str, request: Request) -> dict[str, Any]:
    mem = _get_memory(request)
    try:
        await asyncio.to_thread(mem.delete, memory_id)
        return {"ok": True}
    except Exception:
        logger.exception("delete_memory failed")
        raise HTTPException(status_code=500, detail="Internal server error") from None


@router.get("/health")
async def health_check(request: Request) -> dict[str, Any]:
    checks: dict[str, Any] = {}

    async with httpx.AsyncClient(timeout=5.0) as client:
        try:
            r = await client.get(f"http://{QDRANT_HOST}:{QDRANT_PORT}/healthz")
            checks["qdrant"] = "ok" if r.status_code == 200 else f"status {r.status_code}"
        except Exception as e:
            checks["qdrant"] = f"error: {e}"

    try:
        mem = _get_memory(request)
        if mem and hasattr(mem, "embedding_model"):
            vec: Any = mem.embedding_model.embed("health check")
            checks["embedder"] = "ok" if len(vec) > 0 else "empty vector"
        else:
            checks["embedder"] = "not initialized"
    except Exception as e:
        checks["embedder"] = f"error: {e}"

    if neo4j_available():
        checks["neo4j"] = "ok"
    else:
        checks["neo4j"] = "unavailable"

    # LLM backend info
    backend: dict[str, Any] = getattr(request.app.state, "backend", LLM_BACKEND)
    llm_info: dict[str, Any] = {
        "tier": backend.get("tier", 0),
        "provider": backend.get("provider", "unknown"),
        "model": backend.get("model"),
        "extraction_enabled": backend.get("tier", 0) < 3,
    }

    all_ok = all(v == "ok" for v in checks.values())
    return {"ok": all_ok, "version": "2.0.0", "llm": llm_info, "checks": checks}


@router.get("/graph_stats")
async def graph_stats() -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            node_single = session.run("MATCH (n) RETURN count(n) AS c").single()
            node_count: int = node_single["c"] if node_single else 0
            rel_single = session.run("MATCH ()-[r]->() RETURN count(r) AS c").single()
            rel_count: int = rel_single["c"] if rel_single else 0
            rel_types: list[dict[str, Any]] = session.run(
                "MATCH ()-[r]->() RETURN type(r) AS t, count(*) AS c ORDER BY c DESC LIMIT 30"
            ).data()
            top_nodes: list[dict[str, Any]] = session.run(
                "MATCH (n)-[r]-() RETURN n.name AS name, labels(n) AS labels, count(r) AS rels "
                "ORDER BY rels DESC LIMIT 10"
            ).data()
            singleton_single = session.run(
                "MATCH ()-[r]->() WITH type(r) AS t, count(*) AS c WHERE c = 1 RETURN count(t) AS c"
            ).single()
            singleton_types: int = singleton_single["c"] if singleton_single else 0
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "nodes": node_count,
            "relationships": rel_count,
            "singleton_rel_types": singleton_types,
            "top_relationship_types": rel_types,
            "top_connected_nodes": top_nodes,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph_stats failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False}


# --- Graph Export for Visualization ---


@router.get("/graph/export")
async def graph_export(
    limit: int | None = None,
    community: int | None = None,
    mode: str = "top",
) -> dict[str, Any]:
    """Export graph as nodes + edges for 3D visualization.

    Modes:
      top       — Top N nodes by pagerank (default N=200). Smart default.
      community — All nodes in a specific community (requires community param).
      all       — Full graph export. Use with caution on large graphs.
    """
    if not neo4j_available():
        return {"ok": False, "available": False, "nodes": [], "edges": [], "total_nodes": 0}

    try:
        from collections import defaultdict

        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            # Total node count for "loaded X of Y" UI
            total_single = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL RETURN count(n) AS c"
            ).single()
            total_nodes: int = total_single["c"] if total_single else 0

            # Fetch nodes based on mode
            if mode == "community" and community is not None:
                if limit:
                    node_records: list[dict[str, Any]] = session.run(
                        "MATCH (n) WHERE n.name IS NOT NULL AND n.community = $community "
                        "RETURN n.name AS name, labels(n) AS labels, "
                        "n.pagerank AS pagerank, n.community AS community "
                        "ORDER BY n.pagerank DESC LIMIT $lim",
                        community=community, lim=int(limit),
                    ).data()
                else:
                    node_records: list[dict[str, Any]] = session.run(
                        "MATCH (n) WHERE n.name IS NOT NULL AND n.community = $community "
                        "RETURN n.name AS name, labels(n) AS labels, "
                        "n.pagerank AS pagerank, n.community AS community "
                        "ORDER BY n.pagerank DESC",
                        community=community,
                    ).data()
            elif mode == "all":
                if limit:
                    node_records = session.run(
                        "MATCH (n) WHERE n.name IS NOT NULL "
                        "RETURN n.name AS name, labels(n) AS labels, "
                        "n.pagerank AS pagerank, n.community AS community "
                        "LIMIT $lim",
                        lim=int(limit),
                    ).data()
                else:
                    node_records = session.run(
                        "MATCH (n) WHERE n.name IS NOT NULL "
                        "RETURN n.name AS name, labels(n) AS labels, "
                        "n.pagerank AS pagerank, n.community AS community",
                    ).data()
            else:
                # mode=top (default): top N by pagerank
                effective_limit = limit or 200
                node_query = (
                    "MATCH (n) WHERE n.name IS NOT NULL "
                    "RETURN n.name AS name, labels(n) AS labels, "
                    "n.pagerank AS pagerank, n.community AS community "
                    "ORDER BY n.pagerank DESC LIMIT $lim"
                )
                node_records = session.run(node_query, lim=effective_limit).data()

            node_names: set[str] = {r["name"] for r in node_records}

            nodes: list[dict[str, Any]] = [
                {
                    "id": r["name"],
                    "labels": r["labels"],
                    "pagerank": r["pagerank"] if r["pagerank"] is not None else 0.001,
                    "community": r["community"] if r["community"] is not None else -1,
                }
                for r in node_records
            ]

            # Fetch edges — optimized: push filter to Cypher when node set is small
            edges: list[dict[str, Any]]
            if node_names and len(node_names) < 5000:
                edge_records: list[dict[str, Any]] = session.run(
                    "MATCH (a)-[r]->(b) "
                    "WHERE a.name IN $names AND b.name IN $names "
                    "RETURN a.name AS source, b.name AS target, type(r) AS rel_type",
                    names=list(node_names),
                ).data()
                edges = [
                    {"source": r["source"], "target": r["target"], "rel_type": r["rel_type"]}
                    for r in edge_records
                ]
            else:
                edge_records = session.run(
                    "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                    "RETURN a.name AS source, b.name AS target, type(r) AS rel_type"
                ).data()
                edges = [
                    {"source": r["source"], "target": r["target"], "rel_type": r["rel_type"]}
                    for r in edge_records
                    if r["source"] in node_names and r["target"] in node_names
                ]

            # Community metadata for cloud visualization
            comm_nodes: dict[int, list[dict[str, Any]]] = defaultdict(list)
            for n in nodes:
                if n["community"] != -1:
                    comm_nodes[n["community"]].append(n)

            community_meta: list[dict[str, Any]] = []
            for cid, members in sorted(comm_nodes.items(), key=lambda x: -len(x[1])):
                ranked: list[dict[str, Any]] = sorted(members, key=lambda m: m["pagerank"], reverse=True)
                centroid = ranked[0]
                # Name from top 2-3 nodes
                top_names = [m["id"] for m in ranked[:3]]
                name = " & ".join(top_names[:2])
                if len(top_names) > 2:
                    name += f" +{len(members) - 2}"
                community_meta.append({
                    "id": cid,
                    "size": len(members),
                    "centroid_node": centroid["id"],
                    "name": name,
                    "top_nodes": top_names,
                })

        driver.close()

        return {
            "ok": True,
            "nodes": nodes,
            "edges": edges,
            "communities": len(comm_nodes),
            "community_meta": community_meta,
            "total_nodes": total_nodes,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph/export failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "nodes": [], "edges": [], "total_nodes": 0}


@router.get("/graph/search")
async def graph_search(
    q: str = "",
    community: int | None = None,
    node_type: str | None = None,
    relationship: str | None = None,
    limit: int = 50,
) -> dict[str, Any]:
    """Search graph nodes with filters."""
    if not neo4j_available():
        return {"ok": False, "results": []}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            conditions: list[str] = ["n.name IS NOT NULL"]
            params: dict[str, Any] = {"lim": limit}

            if q:
                conditions.append("toLower(n.name) CONTAINS toLower($query)")
                params["query"] = q

            if community is not None:
                conditions.append("n.community = $community")
                params["community"] = community

            if node_type:
                conditions.append("$node_type IN labels(n)")
                params["node_type"] = node_type

            where = " AND ".join(conditions)
            cypher = (
                f"MATCH (n) WHERE {where} "
                "RETURN n.name AS name, labels(n) AS labels, "
                "n.pagerank AS pagerank, n.community AS community "
                "ORDER BY n.pagerank DESC LIMIT $lim"
            )
            records: list[dict[str, Any]] = session.run(_neo4j.Query(_cypher(cypher)), **params).data()

            results: list[dict[str, Any]] = []
            for r in records:
                node: dict[str, Any] = {
                    "id": r["name"],
                    "labels": r["labels"],
                    "pagerank": r["pagerank"] if r["pagerank"] is not None else 0.001,
                    "community": r["community"] if r["community"] is not None else -1,
                }

                # If relationship filter, check connectivity
                if relationship:
                    rel_check = session.run(
                        "MATCH (n {name: $name})-[r]-() "
                        "WHERE type(r) = $rel "
                        "RETURN count(r) AS c",
                        name=r["name"],
                        rel=relationship,
                    ).single()
                    if rel_check and rel_check["c"] > 0:
                        results.append(node)
                else:
                    results.append(node)

        driver.close()
        return {"ok": True, "results": results, "total": len(results)}
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph/search failed: %s", e)
        return {"ok": False, "results": [], "error": str(e)}


# --- Phase 2.1: Graph-Enhanced Retrieval ---


class GraphEnhancedSearchRequest(BaseModel):
    query: str
    user_id: str = "default"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)
    graph_weight: float = Field(default=0.3, ge=0.0, le=1.0)
    graph_depth: int = Field(default=1, ge=1, le=3)


_ENTITY_STOP_WORDS = frozenset({
    "top", "plus", "maybe", "phase", "done", "let", "ready", "first", "new",
    "well", "even", "made", "also", "back", "next", "last", "here", "just",
    "good", "now", "set", "run", "got", "yes", "use", "key", "note", "need",
    "see", "add", "fix", "test", "end", "big", "old", "few", "get", "try",
    "two", "one", "all", "way", "day", "part", "full", "sure", "real", "open",
    "high", "low", "main", "stop", "take", "left", "right", "make", "like",
    "look", "check", "still", "going", "point", "thing", "plan", "work",
    "start", "about", "think", "know", "want", "move", "find", "keep", "help",
    "call", "come", "give", "tell", "turn", "pull", "push", "hold", "send",
    "drop", "update", "change", "build", "break", "clear", "reset", "clean",
    "points", "both", "second", "third", "only", "some", "much", "many",
    "most", "more", "less", "same", "other", "before", "after", "already",
    "available", "current", "actually", "completely", "quickly", "easily",
    "however", "therefore", "because", "although", "since", "while", "every",
    "each", "above", "below", "between", "through", "during", "without",
})


def _extract_entities(text: str) -> list[str]:
    """Heuristic entity extraction — capitalize words, proper nouns, known patterns."""
    entities: list[str] = []
    # Capitalized words (likely proper nouns)
    for match in re.finditer(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b", text):
        word = match.group()
        if word.lower() not in _ENTITY_STOP_WORDS:
            entities.append(word)
    # Technical terms (lowercase with hyphens/underscores)
    for match in re.finditer(r"\b[a-z]+[-_][a-z]+(?:[-_][a-z]+)*\b", text):
        entities.append(match.group())
    # Quoted strings
    for match in re.finditer(r'"([^"]+)"', text):
        entities.append(match.group(1))
    return list(set(entities))[:10]


def _neo4j_expand_sync(query: str, user_id: str, graph_depth: int = 1) -> list[str]:
    """Synchronous Neo4j neighborhood expansion — called via asyncio.to_thread."""
    if not neo4j_available():
        return []
    entities = _extract_entities(query)
    if not entities:
        return []
    neighbors: list[str] = []
    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            for entity in entities[:5]:
                cypher = (
                    "MATCH (n)-[r*1.." + str(graph_depth) + "]-(neighbor) "
                    "WHERE toLower(n.name) CONTAINS toLower($name) "
                    "RETURN DISTINCT neighbor.name AS name, labels(neighbor) AS labels "
                    "LIMIT 10"
                )
                result = session.run(
                    _neo4j.Query(_cypher(cypher)),
                    name=entity,
                )
                for record in result:
                    name = record["name"]
                    if name:
                        neighbors.append(name)
        driver.close()
        mark_neo4j_ok()
    except Exception as e:
        mark_neo4j_down()
        logger.warning("_neo4j_expand_sync failed: %s", e)
    return neighbors


async def _neo4j_expand_with_timeout(
    query: str,
    user_id: str,
    timeout_ms: int = 800,
    graph_depth: int = 1,
) -> list[str]:
    """Neo4j neighborhood expansion wrapped in asyncio.wait_for with timeout.

    Returns empty list on timeout or any error — Neo4j failure is non-fatal.
    """
    try:
        return await asyncio.wait_for(
            asyncio.to_thread(_neo4j_expand_sync, query, user_id, graph_depth),
            timeout=timeout_ms / 1000,
        )
    except TimeoutError:
        logger.warning("Neo4j expansion timed out after %dms", timeout_ms)
        return []
    except Exception as e:
        logger.warning("Neo4j expansion failed: %s", e)
        return []


def _qdrant_search_direct(
    query: str,
    user_id: str,
    limit: int,
    min_score: float,
    mem: Any,
) -> list[dict[str, Any]]:
    """Direct Qdrant vector search — bypasses Mem0's sequential Qdrant+Neo4j search.

    Embeds query using the Mem0 embedder and queries the Qdrant collection directly.
    Returns results as dicts with memory, score, and metadata fields.
    """
    try:
        embedder: Any = mem.embedding_model
        vector: list[float] = embedder.embed(query)
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=vector,
            query_filter=Filter(
                must=[FieldCondition(key="user_id", match=MatchValue(value=user_id))]
            ),
            limit=limit,
            with_payload=True,
        )
        output: list[dict[str, Any]] = []
        for point in results.points:
            if point.score < min_score:
                continue
            payload: dict[str, Any] = dict(point.payload or {})
            row: dict[str, Any] = {
                "id": str(point.id),
                "memory": payload.get("memory") or payload.get("data", ""),
                "score": point.score,
                "metadata": payload,
            }
            if "created_at" in payload:
                row["created_at"] = payload["created_at"]
            output.append(row)
        return output
    except Exception as e:
        logger.warning("_qdrant_search_direct failed: %s", e)
        return []


@router.post("/graph_enhanced_search")
async def graph_enhanced_search(req: GraphEnhancedSearchRequest, request: Request) -> dict[str, Any]:
    """Vector search enhanced with graph neighborhood expansion.

    Qdrant and Neo4j run in parallel via asyncio.gather. Neo4j is wrapped in
    an 800ms timeout — if it times out or fails, Qdrant-only results are returned.
    """
    mem = _get_memory(request)
    min_score = 0.0  # Return all scored results; post-processing filters by weight

    # Step 1: Launch Qdrant and Neo4j queries in parallel
    qdrant_task = asyncio.create_task(
        asyncio.to_thread(_qdrant_search_direct, req.query, req.user_id, req.limit * 2, min_score, mem)
    )
    neo4j_task = asyncio.create_task(
        _neo4j_expand_with_timeout(req.query, req.user_id, timeout_ms=800, graph_depth=req.graph_depth)
    )

    gather_results = await asyncio.gather(qdrant_task, neo4j_task, return_exceptions=True)

    # Handle results — both failures are non-fatal
    vector_results_raw = gather_results[0]
    graph_neighbors_raw = gather_results[1]

    if isinstance(vector_results_raw, BaseException):
        logger.error("graph_enhanced_search: Qdrant query failed: %s", vector_results_raw)
        vector_results: list[dict[str, Any]] = []
    else:
        vector_results = vector_results_raw  # type: ignore[assignment]

    if isinstance(graph_neighbors_raw, BaseException):
        logger.warning("graph_enhanced_search: Neo4j expansion failed: %s", graph_neighbors_raw)
        graph_neighbors: list[str] = []
    else:
        graph_neighbors = graph_neighbors_raw  # type: ignore[assignment]

    entities = _extract_entities(req.query)

    # Step 2: If Neo4j returned neighbors, run second Qdrant query with expanded terms
    # This depends on Neo4j results, so it cannot be parallelized with Step 1
    graph_results: list[dict[str, Any]] = []
    if graph_neighbors:
        expanded_query = req.query + " " + " ".join(list(set(graph_neighbors))[:5])
        try:
            graph_results = await asyncio.to_thread(
                _qdrant_search_direct, expanded_query, req.user_id, req.limit * 2, min_score, mem
            )
        except Exception as e:
            logger.warning("graph_enhanced_search: expanded search failed: %s", e)

    # Step 3: Merge and deduplicate by memory ID, keeping highest score
    seen_ids: set[str] = set()
    merged: list[dict[str, Any]] = []

    def add_result(r: dict[str, Any], source: str, weight: float) -> None:
        rid = r.get("id", r.get("hash", str(r.get("memory", ""))))
        if rid in seen_ids:
            # Already added from another source — keep only if score is higher
            for existing in merged:
                existing_rid = existing.get("id", existing.get("hash", str(existing.get("memory", ""))))
                if existing_rid == rid:
                    new_combined = r.get("score", 0.5) * weight
                    if new_combined > existing.get("combined_score", 0):
                        existing["combined_score"] = new_combined
                        existing["retrieval_source"] = source
                    break
            return
        seen_ids.add(rid)
        score = r.get("score", 0.5)
        r["combined_score"] = score * weight
        r["retrieval_source"] = source
        merged.append(r)

    vector_weight = 1.0 - req.graph_weight
    for r in vector_results:
        add_result(r, "vector", vector_weight)
    for r in graph_results:
        add_result(r, "graph_expanded", req.graph_weight)

    # Step 4: Apply existing post-processing hooks
    merged = _apply_confidence_weight(merged)
    merged = _apply_recency_boost(merged)

    # Sort by combined_score (post-processing adjusts the underlying score field)
    merged.sort(key=lambda r: r.get("combined_score", 0), reverse=True)

    return {
        "ok": True,
        "results": merged[:req.limit],
        "entities_extracted": entities,
        "graph_neighbors": list(set(graph_neighbors))[:20],
        "sources": {"vector": len(vector_results),
                     "graph_expanded": len(graph_results)},
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
async def consolidate_memories(req: ConsolidateRequest, request: Request) -> dict[str, Any]:
    """Find and optionally merge near-duplicate memories."""
    mem = _get_memory(request)

    try:
        raw_all: Any = await asyncio.to_thread(mem.get_all, user_id=req.user_id, limit=req.limit)
        entries: list[dict[str, Any]] = _as_result_list(raw_all)
    except Exception:
        raise HTTPException(status_code=500, detail="Failed to fetch memories") from None

    if not entries:
        return {"ok": True, "candidates": [], "message": "No memories found"}

    # Find near-duplicate pairs by searching each memory against the corpus
    candidates: list[dict[str, Any]] = []
    checked: set[str] = set()

    for entry in entries[:50]:  # Cap to prevent excessive API calls
        memory_text: str = entry.get("memory", "")
        memory_id: str = entry.get("id", "")
        if not memory_text or memory_id in checked:
            continue
        checked.add(memory_id)

        try:
            search_raw: Any = await asyncio.to_thread(
                mem.search, memory_text, user_id=req.user_id, limit=5
            )
            search_results: list[dict[str, Any]] = _as_result_list(search_raw)
        except Exception:
            continue

        for r in search_results:
            other_id: str = r.get("id", "")
            if other_id == memory_id or other_id in checked:
                continue
            score: float = r.get("score", 0)
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
async def merge_memories(req: MergeRequest, request: Request) -> dict[str, Any]:
    """Merge two memories — keeps target, deletes source."""
    mem = _get_memory(request)
    try:
        await asyncio.to_thread(mem.delete, req.source_id)
        return {"ok": True, "deleted": req.source_id, "kept": req.target_id}
    except Exception:
        raise HTTPException(status_code=500, detail="Internal server error") from None


@router.get("/fact_stats")
async def fact_stats(request: Request, user_id: str = "default") -> dict[str, Any]:
    """Memory corpus statistics."""
    mem = _get_memory(request)

    try:
        raw: Any = await asyncio.to_thread(mem.get_all, user_id=user_id, limit=500)
        entries: list[dict[str, Any]] = _as_result_list(raw)
    except Exception:
        raise HTTPException(status_code=500, detail="Internal server error") from None
    if not entries:
        return {"ok": True, "total": 0}

    total = len(entries)
    avg_length = sum(len(str(e.get("memory", ""))) for e in entries) / max(total, 1)

    # Categorize by metadata
    by_agent: dict[str, int] = {}
    by_domain: dict[str, int] = {}
    for entry in entries:
        meta: dict[str, Any] = entry.get("metadata", {}) or {}
        agent: str = meta.get("agent_id") or meta.get("original_agent") or "unknown"
        domain: str = meta.get("domain", "general")
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
async def retract_memory(req: RetractRequest, request: Request) -> dict[str, Any]:
    """Atomic retraction across Mem0 vector store + Neo4j graph.

    Finds matching memories, removes them from both stores,
    and logs the retraction for audit trail.
    """
    mem = _get_memory(request)

    # Find memories matching the retraction query
    try:
        raw: Any = await asyncio.to_thread(mem.search, req.query, user_id=req.user_id, limit=20)
        results: list[dict[str, Any]] = _as_result_list(raw)
    except Exception:
        raise HTTPException(status_code=500, detail="Search failed") from None
    if not results:
        return {"ok": True, "retracted": 0, "message": "No matching memories found"}

    # Filter to high-confidence matches (score > 0.75)
    to_retract = [r for r in results if r.get("score", 0) > 0.75]

    if not to_retract:
        return {"ok": True, "retracted": 0, "message": "No high-confidence matches (>0.75)"}

    # Neo4j cascade — find and remove connected entities
    neo4j_removed: list[str] = []
    if req.cascade and neo4j_available():
        try:
            driver = neo4j_driver()
            with _neo4j_session(driver) as session:
                for item in to_retract:
                    text = item.get("memory", "")
                    entities = _extract_entities(text)
                    for entity in entities[:5]:
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
            mark_neo4j_ok()
        except Exception:
            mark_neo4j_down()
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
        "timestamp": datetime.now(UTC).isoformat(),
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
    safe_reason = (req.reason or "none").replace("\n", " ").replace("\r", " ")[:100]
    logger.info("Retraction logged: %d memories, reason=%s", len(retracted), safe_reason)


# --- Episode recording (linked from /add) ---


async def _record_episode(text: str, agent_id: str, metadata: dict[str, Any] | None) -> None:
    """Fire-and-forget: create an Episode node for temporal tracking."""
    if not neo4j_available():
        return
    try:
        episode_id = f"ep_{uuid.uuid4().hex[:12]}"
        now = datetime.now(UTC).isoformat()
        session_id = (metadata or {}).get("sessionId", "")
        entities = _extract_entities_for_episode(text)

        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            session.run(
                """
                CREATE (e:Episode {
                    id: $id,
                    content_preview: $preview,
                    agent_id: $agent_id,
                    session_id: $session_id,
                    source: 'after_turn',
                    occurred_at: $now,
                    recorded_at: $now
                })
                """,
                id=episode_id, preview=text[:500], agent_id=agent_id,
                session_id=str(session_id), now=now,
            )
            for entity_name in entities[:15]:
                session.run(
                    """
                    MERGE (ent:Entity {name: $name})
                    WITH ent
                    MATCH (ep:Episode {id: $ep_id})
                    CREATE (ep)-[:MENTIONS {occurred_at: $now}]->(ent)
                    """,
                    name=entity_name, ep_id=episode_id, now=now,
                )
        driver.close()
        mark_neo4j_ok()
        logger.debug(f"Episode {episode_id}: {len(entities)} entities linked")
    except Exception:
        mark_neo4j_down()
        logger.warning("Episode recording failed (non-fatal)", exc_info=True)


# --- Phase C1: Foresight Signals ---


class ForesightAddRequest(BaseModel):
    entity: str
    signal: str
    activation: str  # ISO datetime
    expiry: str | None = None
    weight: float = Field(default=1.0, ge=0.0, le=10.0)


foresight_router = APIRouter(prefix="/foresight")


@foresight_router.post("/add")
async def add_foresight(req: ForesightAddRequest) -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
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
        mark_neo4j_ok()
        return {"ok": True, "entity": req.entity, "signal": req.signal}
    except Exception as e:
        mark_neo4j_down()
        logger.warning("add_foresight failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "reason": "graph_unavailable"}


@foresight_router.get("/active")
async def active_foresight() -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": True, "signals": []}

    try:
        driver = neo4j_driver()
        now = datetime.now(UTC).isoformat()
        with _neo4j_session(driver) as session:
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
            signals: list[dict[str, Any]] = [
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
        mark_neo4j_ok()
        return {"ok": True, "signals": signals}
    except Exception as e:
        mark_neo4j_down()
        logger.warning("active_foresight failed: %s", e)
        return {"ok": True, "signals": []}


@foresight_router.post("/decay")
async def decay_foresight() -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": True, "decayed": 0, "deleted": 0}

    try:
        driver = neo4j_driver()
        now = datetime.now(UTC).isoformat()
        with _neo4j_session(driver) as session:
            decay_result = session.run(
                """
                MATCH (f:ForesightSignal)
                WHERE f.expiry IS NOT NULL AND f.expiry < $now AND f.weight > 0
                SET f.weight = f.weight - 0.1
                RETURN count(f) AS decayed
                """,
                now=now,
            )
            decay_single = decay_result.single()
            decayed: int = decay_single["decayed"] if decay_single else 0

            delete_result = session.run(
                """
                MATCH (e)-[r:HAS_FORESIGHT]->(f:ForesightSignal)
                WHERE f.weight <= 0
                DELETE r, f
                RETURN count(f) AS deleted
                """
            )
            delete_single = delete_result.single()
            deleted: int = delete_single["deleted"] if delete_single else 0
        driver.close()
        mark_neo4j_ok()
        return {"ok": True, "decayed": decayed, "deleted": deleted}
    except Exception as e:
        mark_neo4j_down()
        logger.warning("decay_foresight failed (Neo4j may be down): %s", e)
        return {"ok": True, "decayed": 0, "deleted": 0}


# --- Phase C2: Autonomous Link Generation (A-Mem Pattern) ---


LINK_GENERATION_ENABLED = os.environ.get("LINK_GENERATION_ENABLED", "false").lower() == "true"
ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
LINK_SCORE_THRESHOLD = 0.6
LINK_MAX_NEIGHBORS = 3


async def _generate_links(mem: Any, new_text: str, user_id: str) -> list[dict[str, Any]]:
    """Generate LLM-described links between a new memory and its nearest neighbors."""
    if not LINK_GENERATION_ENABLED or not ANTHROPIC_API_KEY or not neo4j_available():
        return []

    # Find nearest neighbors
    try:
        raw: Any = await asyncio.to_thread(mem.search, new_text, user_id=user_id, limit=LINK_MAX_NEIGHBORS + 1)
        results: list[dict[str, Any]] = _as_result_list(raw)
    except Exception:
        return []

    if not results:
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

                data: dict[str, Any] = resp.json()
                description: str = data.get("content", [{}])[0].get("text", "").strip()
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
            driver = neo4j_driver()
            with _neo4j_session(driver) as session:
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
            mark_neo4j_ok()
            logger.info(f"Generated {len(links)} memory links for new memory")
        except Exception:
            mark_neo4j_down()
            logger.warning("Failed to store memory links in Neo4j")

    return links


# --- Graph Analytics (networkx, since Community Neo4j lacks GDS) ---


class GraphAnalyzeRequest(BaseModel):
    top_k: int = Field(default=20, ge=5, le=100)
    store_scores: bool = True


@router.post("/graph/analyze")
async def analyze_graph(req: GraphAnalyzeRequest) -> dict[str, Any]:
    """Run PageRank + community detection on the Neo4j graph via networkx.

    Scores are optionally written back as node properties for retrieval weighting.
    Intended to be called from the nightly consolidation cron.
    """
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        import networkx as nx

        driver = neo4j_driver()

        graph: Any = cast('Any', nx.DiGraph())
        with _neo4j_session(driver) as session:
            nodes = session.run("MATCH (n) WHERE n.name IS NOT NULL RETURN n.name AS name, labels(n) AS labels")
            for record in nodes:
                graph.add_node(record["name"], labels=record["labels"])

            rels = session.run(
                "MATCH (a)-[r]->(b) WHERE a.name IS NOT NULL AND b.name IS NOT NULL "
                "RETURN a.name AS src, b.name AS dst, type(r) AS rel_type"
            )
            for record in rels:
                graph.add_edge(record["src"], record["dst"], rel_type=record["rel_type"])

        node_count: int = graph.number_of_nodes()
        if node_count == 0:
            driver.close()
            return {"ok": True, "nodes": 0, "message": "Empty graph"}

        # PageRank
        pagerank: dict[str, float] = nx.pagerank(graph, alpha=0.85, max_iter=100)
        top_pagerank: list[tuple[str, float]] = sorted(
            pagerank.items(), key=lambda x: -x[1]
        )[:req.top_k]

        # Community detection (Louvain on undirected projection)
        graph_undirected: Any = graph.to_undirected()
        try:
            communities: Any = nx.community.louvain_communities(graph_undirected, seed=42)
            community_map: dict[str, int] = {}
            for idx, comm in enumerate(cast('list[Any]', communities)):
                for node in cast('set[str]', comm):
                    community_map[node] = idx
            num_communities: int = len(communities)
            largest_communities: list[Any] = sorted(communities, key=len, reverse=True)[:5]
            community_summaries: list[dict[str, Any]] = [
                {"id": idx, "size": len(c), "sample": sorted(cast('set[str]', c))[:5]}
                for idx, c in enumerate(largest_communities)
            ]
        except Exception:
            community_map = {}
            num_communities = 0
            community_summaries = []

        # Node similarity — find dedup candidates (nodes with >0.8 Jaccard on neighbors)
        dedup_candidates: list[dict[str, Any]] = []
        nodes_list: list[str] = list(graph_undirected.nodes())
        for i in range(min(len(nodes_list), 200)):
            n1: str = nodes_list[i]
            neighbors1: set[str] = set(graph_undirected.neighbors(n1))
            if not neighbors1:
                continue
            for j in range(i + 1, min(len(nodes_list), 200)):
                n2: str = nodes_list[j]
                neighbors2: set[str] = set(graph_undirected.neighbors(n2))
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

        # Store scores back to Neo4j for ALL nodes (not just top_k)
        scores_stored = 0
        if req.store_scores:
            with _neo4j_session(driver) as session:
                batch: list[dict[str, Any]] = [
                    {"name": name, "score": round(score, 6), "community": community_map.get(name, -1)}
                    for name, score in pagerank.items()
                ]
                for i in range(0, len(batch), 500):
                    chunk: list[dict[str, Any]] = batch[i : i + 500]
                    session.run(
                        "UNWIND $batch AS row "
                        "MATCH (n {name: row.name}) "
                        "SET n.pagerank = row.score, n.community = row.community",
                        batch=chunk,
                    )
                    scores_stored += len(chunk)

        edge_count: int = graph.number_of_edges()
        driver.close()

        return {
            "ok": True,
            "nodes": node_count,
            "edges": edge_count,
            "pagerank_top": [{"name": n, "score": round(s, 6)} for n, s in top_pagerank],
            "communities": num_communities,
            "community_summaries": community_summaries,
            "dedup_candidates": dedup_candidates[:10],
            "scores_stored": scores_stored,
        }
    except ImportError as err:
        raise HTTPException(status_code=500, detail="networkx not installed") from err
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph/analyze failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False}


# --- Enhanced Search with Query Rewriting ---


class EnhancedSearchRequest(BaseModel):
    query: str
    user_id: str = "default"
    agent_id: str | None = None
    limit: int = Field(default=10, ge=1, le=50)
    rewrite: bool = True


@router.post("/search_enhanced")
async def search_enhanced(req: EnhancedSearchRequest, request: Request) -> dict[str, Any]:
    """Search with entity alias resolution and LLM-generated query variants.

    Pipeline:
    1. Extract entities from query
    2. Resolve aliases via Neo4j (find canonical names)
    3. Generate 2-3 alternate phrasings via Haiku
    4. Run parallel vector searches on all variants
    5. Merge and deduplicate results
    """
    mem = _get_memory(request)

    # Skip rewriting for very short or very long queries
    if not req.rewrite or len(req.query) < 10 or len(req.query) > 500:
        return await _simple_search(mem, req)

    # Step 1: Extract entities
    entities = _extract_entities(req.query)

    # Step 2: Resolve aliases via Neo4j
    canonical_names: dict[str, str] = {}
    if entities and neo4j_available():
        try:
            driver = neo4j_driver()
            with _neo4j_session(driver) as session:
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
            mark_neo4j_ok()
        except Exception:
            mark_neo4j_down()
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
                    data: dict[str, Any] = resp.json()
                    rewrite_text: str = data.get("content", [{}])[0].get("text", "")
                    for line in rewrite_text.strip().split("\n"):
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
            search_raw: Any = await asyncio.to_thread(mem.search, query, **search_kwargs)
            return _as_result_list(search_raw)
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
        raw: Any = await asyncio.to_thread(mem.search, req.query, **kwargs)
        results_list: list[dict[str, Any]] = _as_result_list(raw)
        return {
            "ok": True,
            "results": results_list,
            "query_variants": [req.query],
            "aliases_resolved": {},
            "total_candidates": len(results_list),
        }
    except Exception:
        raise HTTPException(status_code=500, detail="Internal server error") from None





# ---------------------------------------------------------------------------
# Phase 4b: Entity Resolution Endpoints
# ---------------------------------------------------------------------------


class ResolveEntityRequest(BaseModel):
    """Resolve entity names to canonical forms."""
    names: list[str]


@router.post("/entities/resolve")
async def resolve_entities(req: ResolveEntityRequest) -> dict[str, Any]:
    """Resolve a list of entity names to their canonical forms.

    Uses the alias table, fuzzy matching, and Neo4j canonical registry
    to deduplicate entity names before creation.
    """
    existing = await asyncio.to_thread(get_canonical_entities)
    results: list[dict[str, Any]] = []

    for name in req.names[:50]:  # Cap at 50 per request
        resolved = resolve_entity(name, existing)
        if resolved is None:
            results.append({"input": name, "canonical": None, "action": "skip", "reason": "invalid"})
        elif resolved != name.strip().lower():
            results.append({"input": name, "canonical": resolved, "action": "alias"})
        else:
            results.append({"input": name, "canonical": resolved, "action": "new"})

    return {"ok": True, "results": results}


@router.post("/entities/merge_duplicates")
async def merge_duplicates() -> dict[str, Any]:
    """Find and merge duplicate entity nodes in Neo4j using alias table + fuzzy matching."""
    result: dict[str, Any] = await asyncio.to_thread(merge_duplicate_entities)
    return result


@router.post("/entities/cleanup_orphans")
async def cleanup_orphans() -> dict[str, Any]:
    """Remove entity nodes with no relationships."""
    result: dict[str, Any] = await asyncio.to_thread(cleanup_orphan_entities)
    return result


# ---------------------------------------------------------------------------
# Entity Detail / Edit / Delete — Graph UI support
# ---------------------------------------------------------------------------


class EntityMergeRequest(BaseModel):
    source: str
    target: str


@router.get("/entity/{name}")
async def entity_detail(name: str, request: Request) -> dict[str, Any]:
    """Full entity detail: relationships, memory mentions, timestamps, confidence."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    relationships: list[dict[str, Any]] = []
    properties: dict[str, Any] = {}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            # Node properties
            node = session.run(
                "MATCH (n {name: $name}) "
                "RETURN n.name AS name, labels(n) AS labels, n.pagerank AS pagerank, "
                "n.community AS community, properties(n) AS props",
                name=name,
            ).single()
            if not node:
                raise HTTPException(status_code=404, detail=f"Entity '{name}' not found")
            properties = dict(node["props"] or {})
            properties["labels"] = node["labels"]

            # Relationships (both directions)
            rels: list[dict[str, Any]] = session.run(
                "MATCH (n {name: $name})-[r]-(m) "
                "RETURN type(r) AS rel_type, m.name AS target, "
                "startNode(r) = n AS outgoing, properties(r) AS props "
                "ORDER BY type(r) LIMIT 50",
                name=name,
            ).data()
            for rel in rels:
                relationships.append({
                    "type": rel["rel_type"],
                    "target": rel["target"],
                    "direction": "outgoing" if rel["outgoing"] else "incoming",
                    **({"props": rel["props"]} if rel["props"] else {}),
                })

        driver.close()
        mark_neo4j_ok()
    except HTTPException:
        raise
    except Exception as e:
        mark_neo4j_down()
        logger.warning("entity_detail failed: %s", e)
        return {"ok": False, "available": False}

    # Memory mentions (Qdrant search by entity name)
    memories: list[dict[str, Any]] = []
    try:
        mem = _get_memory(request)
        raw: Any = await asyncio.to_thread(mem.search, name, user_id="default", limit=10)
        results: list[dict[str, Any]] = _as_result_list(raw)
        if results:
            for r in results:
                if r.get("score", 0) > 0.4:
                    meta: dict[str, Any] = cast('dict[str, Any]', r.get("metadata") or {})
                    created: Any = r.get("created_at") or meta.get("created_at")
                    agent_id: Any = meta.get("agent_id")
                    source: Any = meta.get("source", "inferred")
                    memories.append({
                        "id": r.get("id"),
                        "text": r.get("memory", "")[:300],
                        "score": round(r.get("score", 0), 3),
                        "created_at": created,
                        "agent_id": agent_id,
                        "source": source,
                    })
    except Exception:
        pass  # Non-fatal — graph data still returned

    # Confidence indicator: green (>0.01 pagerank + 5+ rels), yellow (either), red (neither)
    pagerank = properties.get("pagerank", 0) or 0
    rel_count = len(relationships)
    confidence = "high" if pagerank > 0.01 and rel_count >= 5 else "medium" if pagerank > 0.005 or rel_count >= 3 else "low"

    return {
        "ok": True,
        "name": name,
        "properties": properties,
        "relationships": relationships,
        "relationship_count": rel_count,
        "memories": memories,
        "confidence": confidence,
    }


@router.delete("/entity/{name}")
async def delete_entity(name: str) -> dict[str, Any]:
    """Delete an entity node and all its connected edges from Neo4j."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            result = session.run(
                "MATCH (n {name: $name}) "
                "OPTIONAL MATCH (n)-[r]-() "
                "WITH n, count(r) AS rels "
                "DETACH DELETE n "
                "RETURN rels",
                name=name,
            ).single()
        driver.close()
        mark_neo4j_ok()

        if result is None:
            raise HTTPException(status_code=404, detail=f"Entity '{name}' not found")

        logger.info(f"Deleted entity '{name}' ({result['rels']} relationships removed)")
        return {"ok": True, "deleted": name, "relationships_removed": result["rels"]}
    except HTTPException:
        raise
    except Exception as e:
        mark_neo4j_down()
        logger.warning("delete_entity failed: %s", e)
        return {"ok": False, "available": False}


@router.patch("/entity/{name}/flag")
async def flag_entity(name: str, request: Request) -> dict[str, Any]:
    """Toggle flagged state on an entity node."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        body = await request.json()
        flagged = bool(body.get("flagged", True))
    except Exception:
        flagged = True

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            result = session.run(
                "MATCH (n {name: $name}) "
                "SET n.flagged = $flagged "
                "RETURN n.name AS name, n.flagged AS flagged",
                name=name,
                flagged=flagged,
            ).single()
        driver.close()
        mark_neo4j_ok()

        if result is None:
            raise HTTPException(status_code=404, detail=f"Entity '{name}' not found")

        logger.info(f"{'Flagged' if flagged else 'Unflagged'} entity '{name}'")
        return {"ok": True, "entity": result["name"], "flagged": result["flagged"]}
    except HTTPException:
        raise
    except Exception as e:
        mark_neo4j_down()
        logger.warning("flag_entity failed: %s", e)
        return {"ok": False, "available": False}


@router.post("/entity/merge")
async def merge_entities(req: EntityMergeRequest) -> dict[str, Any]:
    """Merge source entity into target — redirects all relationships, deletes source."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        redirected = 0
        with _neo4j_session(driver) as session:
            # Verify both exist
            source = session.run("MATCH (n {name: $name}) RETURN n", name=req.source).single()
            target = session.run("MATCH (n {name: $name}) RETURN n", name=req.target).single()
            if not source:
                raise HTTPException(status_code=404, detail=f"Source entity '{req.source}' not found")
            if not target:
                raise HTTPException(status_code=404, detail=f"Target entity '{req.target}' not found")

            # Redirect outgoing relationships
            out_result = session.run(
                "MATCH (s {name: $source})-[r]->(m) "
                "WHERE m.name <> $target "
                "WITH s, r, m, type(r) AS rtype, properties(r) AS props "
                "MATCH (t {name: $target}) "
                "CALL apoc.create.relationship(t, rtype, props, m) YIELD rel "
                "DELETE r "
                "RETURN count(r) AS count",
                source=req.source, target=req.target,
            ).single()

            # Redirect incoming relationships
            in_result = session.run(
                "MATCH (m)-[r]->(s {name: $source}) "
                "WHERE m.name <> $target "
                "WITH s, r, m, type(r) AS rtype, properties(r) AS props "
                "MATCH (t {name: $target}) "
                "CALL apoc.create.relationship(m, rtype, props, t) YIELD rel "
                "DELETE r "
                "RETURN count(r) AS count",
                source=req.source, target=req.target,
            ).single()

            redirected = (out_result["count"] if out_result else 0) + (in_result["count"] if in_result else 0)

            # Delete source node
            session.run("MATCH (n {name: $name}) DETACH DELETE n", name=req.source)

        driver.close()
        mark_neo4j_ok()
        logger.info(f"Merged entity '{req.source}' → '{req.target}' ({redirected} relationships redirected)")
        return {"ok": True, "source": req.source, "target": req.target, "relationships_redirected": redirected}
    except HTTPException:
        raise
    except Exception as e:
        mark_neo4j_down()
        # APOC might not be available — fall back to simple merge (just delete source, lose rels)
        if "apoc" in str(e).lower() or "Unknown function" in str(e):
            try:
                driver = neo4j_driver()
                with _neo4j_session(driver) as session:
                    session.run("MATCH (n {name: $name}) DETACH DELETE n", name=req.source)
                driver.close()
                mark_neo4j_ok()
                logger.info(f"Merged (fallback, rels lost) entity '{req.source}' → '{req.target}'")
                return {"ok": True, "source": req.source, "target": req.target,
                        "relationships_redirected": 0, "warning": "APOC not available, relationships not redirected"}
            except Exception as e2:
                mark_neo4j_down()
                logger.warning("merge_entities fallback failed: %s", e2)
        else:
            logger.warning("merge_entities failed: %s", e)
        return {"ok": False, "available": False}


# ---------------------------------------------------------------------------
# Phase 5: Memory Lifecycle — Confidence, Contradiction, Correction
# ---------------------------------------------------------------------------

CONTRADICTION_THRESHOLD = 0.80  # Cosine similarity to check for contradictions


class CorrectMemoryRequest(BaseModel):
    """Correct an existing memory — supersedes old version."""
    query: str  # Semantic search to find the memory to correct
    corrected_text: str  # The corrected version
    reason: str  # Audit trail
    user_id: str = "default"
    agent_id: str | None = None


class ForgetMemoryRequest(BaseModel):
    """Soft-delete memories matching a semantic query."""
    query: str
    reason: str
    user_id: str = "default"
    max_deletions: int = Field(default=3, ge=1, le=10)
    min_score: float = Field(default=0.85, ge=0.5, le=1.0)
    dry_run: bool = False


async def _check_contradictions(
    client: QdrantClient,
    vector: list[float],
    text: str,
    user_id: str,
) -> list[dict[str, Any]]:
    """Check if a new fact contradicts existing memories.

    Returns list of potentially contradicting memories with scores.
    Contradiction detection is heuristic — high similarity + different content.
    """
    try:
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=vector,
            query_filter=Filter(
                must=[FieldCondition(key="user_id", match=MatchValue(value=user_id))]
            ),
            limit=5,
            with_payload=True,
        )

        contradictions: list[dict[str, Any]] = []
        text_lower = text.lower()
        negation_words = {"not", "no", "never", "don't", "doesn't", "isn't", "wasn't",
                          "aren't", "weren't", "won't", "can't", "shouldn't", "incorrect",
                          "wrong", "false", "correction", "actually", "instead", "rather"}

        for point in results.points:
            if point.score < CONTRADICTION_THRESHOLD:
                continue

            existing_text = (point.payload or {}).get("memory", "") or (point.payload or {}).get("data", "")
            if not existing_text:
                continue

            existing_lower = existing_text.lower()

            # Skip if texts are near-identical (reinforcement, not contradiction)
            if point.score >= DIRECT_DEDUP_THRESHOLD:
                continue

            # Heuristic: high similarity but with negation markers suggests contradiction
            new_has_negation = any(w in text_lower.split() for w in negation_words)
            old_has_negation = any(w in existing_lower.split() for w in negation_words)
            has_correction = "correction" in text_lower or text_lower.startswith("correction:")

            # Flag if one has negation and the other doesn't, or if it's an explicit correction
            if new_has_negation != old_has_negation or has_correction:
                contradictions.append({
                    "id": str(point.id),
                    "memory": existing_text[:300],
                    "score": round(point.score, 4),
                    "reason": "correction_marker" if has_correction else "negation_asymmetry",
                })

        return contradictions

    except Exception as e:
        logger.warning(f"Contradiction check failed: {e}")
        return []


@router.post("/add_direct")
async def add_direct_v2(req: AddDirectRequest, request: Request) -> dict[str, Any]:
    """Store a single pre-extracted fact directly in Qdrant.

    Bypasses Mem0's LLM extraction entirely. The caller is responsible
    for fact quality — this endpoint embeds and stores as-is.
    Includes contradiction detection (Phase 5).

    Requires agent_id and session_id — requests missing either are rejected
    with 400 to prevent creation of orphaned Qdrant entries.

    EXTR-06 (infer=False): This path structurally bypasses mem.add() entirely.
    Facts are written directly to Qdrant via client.upsert() — Mem0's LLM
    extraction is never invoked. The infer=False requirement is satisfied
    architecturally, not via a parameter.
    """
    missing = [f for f in ("agent_id", "session_id") if not getattr(req, f, None)]
    if missing:
        raise HTTPException(
            status_code=400,
            detail=f"Required field(s) missing: {', '.join(missing)}",
        )

    mem = _get_memory(request)
    text = req.text.strip()
    if not text:
        return {"ok": False, "error": "empty text"}

    try:
        content_hash = _content_hash(text)
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

        # Content hash dedup (exact match)
        existing = client.scroll(
            collection_name=COLLECTION_NAME,
            scroll_filter=Filter(
                must=[
                    FieldCondition(key="hash", match=MatchValue(value=content_hash)),
                    FieldCondition(key="user_id", match=MatchValue(value=req.user_id)),
                ]
            ),
            limit=1,
            with_payload=False,
            with_vectors=False,
        )
        if existing[0]:
            return {"ok": True, "result": "deduplicated", "reason": "content_hash"}

        # Embed
        vectors = await _embed_texts(mem, [text])
        vector = vectors[0]

        # Semantic dedup (cosine similarity)
        if await _semantic_dedup_check(client, vector, req.user_id):
            return {"ok": True, "result": "deduplicated", "reason": "semantic"}

        # Contradiction detection
        contradictions = await _check_contradictions(client, vector, text, req.user_id)

        # Entity resolution — resolve entities in the text before storage
        entities = _extract_entities(text)
        resolved_entities: list[str] = []
        if entities:
            canonical_list = await asyncio.to_thread(get_canonical_entities)
            for entity in entities[:5]:
                resolved = resolve_entity(entity, canonical_list)
                if resolved:
                    resolved_entities.append(resolved)

        # Store
        now = datetime.now(UTC).isoformat()
        point_id = str(uuid.uuid4())
        payload: dict[str, Any] = {
            "memory": text[:500],
            "data": text,
            "source": req.source,
            "hash": content_hash,
            "user_id": req.user_id,
            "created_at": now,
            "confidence": req.confidence,
        }
        if req.agent_id:
            payload["agent_id"] = req.agent_id
        if req.session_id:
            payload["session_id"] = req.session_id
        if resolved_entities:
            payload["entities"] = resolved_entities
        if contradictions:
            payload["contradicts"] = [c["id"] for c in contradictions]

        client.upsert(
            collection_name=COLLECTION_NAME,
            points=[PointStruct(id=point_id, vector=vector, payload=payload)],
        )

        # Graph extraction via SimpleKGPipeline — fire and forget
        backend: dict[str, Any] = getattr(request.app.state, "backend", LLM_BACKEND)
        graph_task: asyncio.Task[Any] = asyncio.create_task(extract_graph(text, backend=backend))
        _background_tasks.add(graph_task)
        graph_task.add_done_callback(_background_tasks.discard)

        result: dict[str, Any] = {"ok": True, "result": "added", "id": point_id}
        if contradictions:
            result["contradictions"] = contradictions
            logger.info(f"add_direct: stored '{text[:80]}' with {len(contradictions)} contradiction(s)")
        else:
            logger.info(f"add_direct: stored '{text[:80]}' (source={req.source}, agent={req.agent_id})")

        return result

    except Exception as e:
        logger.exception("add_direct failed")
        raise HTTPException(status_code=500, detail=str(e)) from e


@router.post("/memory/correct")
async def correct_memory(req: CorrectMemoryRequest, request: Request) -> dict[str, Any]:
    """Correct an existing memory — finds it by semantic search, supersedes it, stores the correction.

    The old memory gets a 'superseded_by' field and its confidence drops to 0.1.
    The new memory gets a 'corrects' field linking to the old ID.
    """
    mem = _get_memory(request)
    corrected_text = req.corrected_text.strip()
    if not corrected_text:
        return {"ok": False, "error": "empty corrected_text"}

    try:
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

        # Find the memory to correct via semantic search
        query_vector = (await _embed_texts(mem, [req.query]))[0]
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=query_vector,
            query_filter=Filter(
                must=[FieldCondition(key="user_id", match=MatchValue(value=req.user_id))]
            ),
            limit=3,
            with_payload=True,
        )

        if not results.points or results.points[0].score < 0.80:
            return {"ok": False, "error": "no matching memory found", "top_score": results.points[0].score if results.points else 0}

        old_point = results.points[0]
        old_id = str(old_point.id)
        old_text = (old_point.payload or {}).get("memory", "?")

        # Mark old memory as superseded (drop confidence, add metadata)
        old_payload = dict(old_point.payload or {})
        old_payload["confidence"] = 0.1
        old_payload["superseded_by"] = None  # Will be filled after new point is created
        old_payload["superseded_at"] = datetime.now(UTC).isoformat()
        old_payload["superseded_reason"] = f"[{req.agent_id or 'user'}] {req.reason}"

        # Store the corrected version
        new_vector = (await _embed_texts(mem, [corrected_text]))[0]
        new_id = str(uuid.uuid4())
        now = datetime.now(UTC).isoformat()
        new_payload = {
            "memory": corrected_text[:500],
            "data": corrected_text,
            "source": "correction",
            "hash": _content_hash(corrected_text),
            "user_id": req.user_id,
            "created_at": now,
            "confidence": 0.95,  # Corrections are high-confidence
            "corrects": old_id,
            "correction_reason": req.reason,
        }
        if req.agent_id:
            new_payload["agent_id"] = req.agent_id

        # Update old point to reference new one
        old_payload["superseded_by"] = new_id

        # Write both
        client.upsert(
            collection_name=COLLECTION_NAME,
            points=[
                PointStruct(id=old_id, vector=cast('Any', old_point.vector) or query_vector, payload=old_payload),
                PointStruct(id=new_id, vector=new_vector, payload=new_payload),
            ],
        )

        logger.info(f"memory/correct: '{old_text[:60]}' → '{corrected_text[:60]}' (reason: {req.reason})")

        return {
            "ok": True,
            "old_id": old_id,
            "old_text": old_text[:200],
            "new_id": new_id,
            "new_text": corrected_text[:200],
        }

    except Exception as e:
        logger.exception("memory/correct failed")
        raise HTTPException(status_code=500, detail=str(e)) from e


@router.post("/memory/forget")
async def forget_memory(req: ForgetMemoryRequest, request: Request) -> dict[str, Any]:
    """Soft-delete memories matching a semantic query.

    Doesn't physically remove — sets confidence to 0.0 and adds 'forgotten' flag.
    This way the memory is excluded from recall but recoverable if needed.
    """
    mem = _get_memory(request)

    try:
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

        # Search for matching memories
        query_vector = (await _embed_texts(mem, [req.query]))[0]
        results = client.query_points(
            collection_name=COLLECTION_NAME,
            query=query_vector,
            query_filter=Filter(
                must=[FieldCondition(key="user_id", match=MatchValue(value=req.user_id))]
            ),
            limit=req.max_deletions * 2,
            with_payload=True,
        )

        # Filter to high-confidence matches only
        matches = [p for p in results.points if p.score >= req.min_score][:req.max_deletions]

        if not matches:
            return {"ok": True, "forgotten": 0, "reason": "no matches above threshold"}

        if req.dry_run:
            previews = [
                {"id": str(p.id), "memory": ((p.payload or {}).get("memory", ""))[:200], "score": round(p.score, 4)}
                for p in matches
            ]
            return {"ok": True, "dry_run": True, "would_forget": len(matches), "matches": previews}

        # Soft-delete: set confidence=0, add forgotten flag
        now = datetime.now(UTC).isoformat()
        points_to_update: list[PointStruct] = []
        forgotten_texts: list[str] = []
        for point in matches:
            payload = dict(point.payload or {})
            payload["confidence"] = 0.0
            payload["forgotten"] = True
            payload["forgotten_at"] = now
            payload["forgotten_reason"] = req.reason
            points_to_update.append(
                PointStruct(id=point.id, vector=cast('Any', point.vector) or query_vector, payload=payload)
            )
            mem_text: str = str((point.payload or {}).get("memory", ""))
            forgotten_texts.append(mem_text[:100])

        client.upsert(collection_name=COLLECTION_NAME, points=points_to_update)

        logger.info(f"memory/forget: {len(matches)} memories forgotten (reason: {req.reason})")

        return {
            "ok": True,
            "forgotten": len(matches),
            "memories": forgotten_texts,
        }

    except Exception as e:
        logger.exception("memory/forget failed")
        raise HTTPException(status_code=500, detail=str(e)) from e



# --- Spec 09 Phases 8-13: Graph Intelligence Endpoints ---


@router.get("/memory/health")
async def memory_health(request: Request, user_id: str = "default") -> dict[str, Any]:
    """Aggregate memory health stats: total, stale, conflicting, flagged, avg confidence."""
    mem = _get_memory(request)

    try:
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
        # Get collection info for total count
        collection = client.get_collection(COLLECTION_NAME)
        total: int = collection.points_count or 0

        # Sample memories for stats (up to 500)
        raw: Any = await asyncio.to_thread(mem.get_all, user_id=user_id, limit=500)
        entries: list[dict[str, Any]] = _as_result_list(raw)

        now = datetime.now(UTC)
        stale_count = 0
        flagged_count = 0
        forgotten_count = 0
        low_confidence_count = 0
        confidence_sum = 0.0
        confidence_n = 0
        by_agent: dict[str, int] = {}
        oldest_date: str | None = None
        newest_date: str | None = None

        for entry in entries:
            meta: dict[str, Any] = cast('dict[str, Any]', entry.get("metadata", {}) or {})

            # Confidence tracking
            conf: Any = meta.get("confidence")
            if isinstance(conf, (int, float)):
                confidence_sum += conf
                confidence_n += 1
                if conf < 0.3:
                    low_confidence_count += 1

            # Staleness: >30 days since created
            created: Any = entry.get("created_at") or meta.get("created_at")
            if created:
                try:
                    created_str: str = str(created)
                    dt = datetime.fromisoformat(created_str.replace("Z", "+00:00"))
                    age_days = (now - dt).days
                    if age_days > 30:
                        stale_count += 1
                    date_str = dt.isoformat()
                    if oldest_date is None or date_str < oldest_date:
                        oldest_date = date_str
                    if newest_date is None or date_str > newest_date:
                        newest_date = date_str
                except (ValueError, TypeError):
                    pass

            # Flagged / forgotten
            if meta.get("flagged"):
                flagged_count += 1
            if meta.get("forgotten"):
                forgotten_count += 1

            # By agent
            agent: str = str(meta.get("agent_id") or meta.get("original_agent") or "unknown")
            by_agent[agent] = by_agent.get(agent, 0) + 1

        avg_confidence = round(confidence_sum / max(confidence_n, 1), 3)

        # Contradiction detection: search for pairs with opposing sentiment on same entity
        # Lightweight: count memories with confidence < 0.4 as potential conflicts
        conflict_count = low_confidence_count  # Proxy: low confidence often means contradicting info

        return {
            "ok": True,
            "total": total,
            "sampled": len(entries),
            "stale": stale_count,
            "conflicts": conflict_count,
            "flagged": flagged_count,
            "forgotten": forgotten_count,
            "avg_confidence": avg_confidence,
            "by_agent": dict(sorted(by_agent.items(), key=lambda x: -x[1])),
            "date_range": {"oldest": oldest_date, "newest": newest_date},
        }
    except Exception as e:
        logger.exception("memory/health failed")
        raise HTTPException(status_code=500, detail=str(e)) from e


@router.get("/graph/timeline")
async def graph_timeline(
    request: Request,
    since: str | None = None,
    until: str | None = None,
    limit: int = 200,
) -> dict[str, Any]:
    """Date-filtered graph export. Returns nodes that have memories within the date range."""
    if not neo4j_available():
        return {"ok": False, "available": False, "nodes": [], "edges": []}

    # Parse date params
    since_dt = None
    until_dt = None
    if since:
        try:
            since_dt = datetime.fromisoformat(since.replace("Z", "+00:00"))
        except ValueError as err:
            raise HTTPException(status_code=400, detail=f"Invalid 'since' date: {since}") from err
    if until:
        try:
            until_dt = datetime.fromisoformat(until.replace("Z", "+00:00"))
        except ValueError as err:
            raise HTTPException(status_code=400, detail=f"Invalid 'until' date: {until}") from err

    # Get all entities from Neo4j
    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            result = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "OPTIONAL MATCH (n)-[r]->(m) WHERE m.name IS NOT NULL "
                "RETURN n.name AS name, labels(n) AS labels, "
                "n.pagerank AS pagerank, n.community AS community, "
                "n.created_at AS created_at, n.updated_at AS updated_at, "
                "collect(DISTINCT {type: type(r), target: m.name}) AS rels "
                "ORDER BY COALESCE(n.pagerank, 0) DESC LIMIT $limit",
                limit=limit,
            )
            rows: list[dict[str, Any]] = result.data()
        driver.close()
        mark_neo4j_ok()
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph/timeline failed: %s", e)
        return {"ok": False, "available": False, "nodes": [], "edges": []}

    # Filter by date range if specified
    nodes: list[dict[str, Any]] = []
    edges: list[dict[str, Any]] = []
    node_ids: set[str] = set()

    for row in rows:
        include = True
        created = row.get("created_at") or row.get("updated_at")
        if created and (since_dt or until_dt):
            try:
                dt = datetime.fromisoformat(str(created).replace("Z", "+00:00"))
                if since_dt and dt < since_dt:
                    include = False
                if until_dt and dt > until_dt:
                    include = False
            except (ValueError, TypeError):
                pass  # Include if date unparseable

        if include:
            name = row["name"]
            node_ids.add(name)
            nodes.append({
                "id": name,
                "labels": row.get("labels", []),
                "pagerank": row.get("pagerank") or 0,
                "community": row.get("community", -1) if row.get("community") is not None else -1,
                "created_at": str(created) if created else None,
            })

            rels_list: list[dict[str, Any]] = cast('list[dict[str, Any]]', row.get("rels") or [])
            for rel in rels_list:
                if rel.get("target"):
                    edges.append({
                        "source": name,
                        "target": rel["target"],
                        "rel_type": rel.get("type", "RELATED_TO"),
                    })

    # Filter edges to only include nodes in our set
    edges = [e for e in edges if e["source"] in node_ids and e["target"] in node_ids]

    return {
        "ok": True,
        "nodes": nodes,
        "edges": edges,
        "total_nodes": len(nodes),
        "date_range": {"since": since, "until": until},
    }


@router.get("/graph/agent-overlay")
async def graph_agent_overlay(request: Request, user_id: str = "default") -> dict[str, Any]:
    """Returns per-node agent ownership data: which agent knows most about each entity."""
    mem = _get_memory(request)

    try:
        # Get all memories with agent metadata
        raw: Any = await asyncio.to_thread(mem.get_all, user_id=user_id, limit=500)
        entries: list[dict[str, Any]] = _as_result_list(raw)

        # Build entity → agent frequency map
        entity_agents: dict[str, dict[str, int]] = {}

        for entry in entries:
            meta: dict[str, Any] = cast('dict[str, Any]', entry.get("metadata", {}) or {})
            agent: str = str(meta.get("agent_id") or meta.get("original_agent") or "")
            if not agent:
                continue

            text: str = str(entry.get("memory", ""))
            words: list[str] = re.findall(r'\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b', text)
            for word in words:
                if word not in entity_agents:
                    entity_agents[word] = {}
                entity_agents[word][agent] = entity_agents[word].get(agent, 0) + 1

        # Convert to node → primary agent + all agents
        node_agents: dict[str, dict[str, Any]] = {}
        all_agents: set[str] = set()

        for entity, agents in entity_agents.items():
            if not agents:
                continue
            primary = max(agents, key=agents.get)  # type: ignore
            all_agents.update(agents.keys())
            node_agents[entity] = {
                "primary": primary,
                "agents": agents,
                "total_mentions": sum(agents.values()),
            }

        return {
            "ok": True,
            "node_agents": node_agents,
            "all_agents": sorted(all_agents),
            "total_entities": len(node_agents),
        }
    except Exception as e:
        logger.exception("graph/agent-overlay failed")
        raise HTTPException(status_code=500, detail=str(e)) from e


@router.get("/graph/drift")
async def graph_drift(request: Request, user_id: str = "default", stale_days: int = 30) -> dict[str, Any]:
    """Detect stale nodes, orphaned clusters, and suggest cleanup actions."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        with _neo4j_session(driver) as session:
            # Orphaned nodes: no relationships at all
            orphans: list[dict[str, Any]] = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "AND NOT (n)--() "
                "RETURN n.name AS name, n.pagerank AS pagerank, n.community AS community "
                "ORDER BY COALESCE(n.pagerank, 0) DESC LIMIT 50"
            ).data()

            # Low-connectivity nodes (only 1 relationship)
            low_conn: list[dict[str, Any]] = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL "
                "WITH n, size([(n)--() | 1]) AS degree "
                "WHERE degree = 1 "
                "RETURN n.name AS name, n.pagerank AS pagerank, degree "
                "ORDER BY COALESCE(n.pagerank, 0) ASC LIMIT 30"
            ).data()

            # Small isolated clusters (community size <= 2)
            small_clusters: list[dict[str, Any]] = session.run(
                "MATCH (n) WHERE n.community IS NOT NULL AND n.name IS NOT NULL "
                "WITH n.community AS comm, collect(n.name) AS members, count(*) AS size "
                "WHERE size <= 2 "
                "RETURN comm, members, size ORDER BY size"
            ).data()

            # Total node count
            total = session.run(
                "MATCH (n) WHERE n.name IS NOT NULL RETURN count(n) AS total"
            ).single()
            total_count = total["total"] if total else 0

        driver.close()
        mark_neo4j_ok()
    except Exception as e:
        mark_neo4j_down()
        logger.warning("graph/drift failed: %s", e)
        return {"ok": False, "available": False}

    # Check memory staleness via Qdrant
    stale_entities: list[dict[str, Any]] = []
    try:
        mem = _get_memory(request)
        raw: Any = await asyncio.to_thread(mem.get_all, user_id=user_id, limit=500)
        entries: list[dict[str, Any]] = _as_result_list(raw)
        if entries:
            now = datetime.now(UTC)
            entity_dates: dict[str, datetime] = {}
            for entry in entries:
                meta: dict[str, Any] = cast('dict[str, Any]', entry.get("metadata", {}) or {})
                created: Any = entry.get("created_at") or meta.get("created_at")
                if not created:
                    continue
                try:
                    dt = datetime.fromisoformat(str(created).replace("Z", "+00:00"))
                    entry_text: str = str(entry.get("memory", ""))
                    words: list[str] = re.findall(r'\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b', entry_text)
                    for word in words:
                        if word not in entity_dates or dt > entity_dates[word]:
                            entity_dates[word] = dt
                except (ValueError, TypeError):
                    pass

            for entity, last_seen in entity_dates.items():
                age_days = (now - last_seen).days
                if age_days > stale_days:
                    stale_entities.append({
                        "name": entity,
                        "last_seen": last_seen.isoformat(),
                        "age_days": age_days,
                    })
            stale_entities.sort(key=lambda x: -x["age_days"])
    except Exception:
        pass  # Non-fatal

    # Generate suggested actions
    suggestions: list[dict[str, str]] = []
    for o in orphans[:5]:
        suggestions.append({
            "type": "delete",
            "entity": o["name"],
            "reason": "Orphaned node (no relationships)",
        })
    for s in stale_entities[:5]:
        suggestions.append({
            "type": "review",
            "entity": s["name"],
            "reason": f"Stale — last mentioned {s['age_days']}d ago",
        })
    for cluster in small_clusters[:3]:
        members = cluster["members"]
        suggestions.append({
            "type": "merge_or_delete",
            "entity": ", ".join(members),
            "reason": f"Isolated cluster of {cluster['size']} nodes",
        })

    return {
        "ok": True,
        "total_nodes": total_count,
        "orphaned_nodes": orphans,
        "low_connectivity": low_conn,
        "small_clusters": small_clusters,
        "stale_entities": stale_entities[:20],
        "suggestions": suggestions,
        "suggestion_count": len(suggestions),
    }
