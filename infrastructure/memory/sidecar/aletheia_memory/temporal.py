# Temporal memory layer -- Graphiti-inspired episode tracking and bi-temporal queries
# NOTE: Do NOT add future annotations -- see routes.py comment.

import asyncio
import contextlib
import logging
import uuid
from datetime import UTC, datetime
from typing import Any, LiteralString, cast

from fastapi import APIRouter, HTTPException, Request
from neo4j import Driver as Neo4jDriver
from neo4j import Session as Neo4jSession
from pydantic import BaseModel, Field
from qdrant_client import QdrantClient
from qdrant_client.models import FieldCondition, Filter, MatchValue

from .config import QDRANT_HOST, QDRANT_PORT
from .graph import mark_neo4j_down, mark_neo4j_ok, neo4j_available, neo4j_driver

logger = logging.getLogger("aletheia_memory.temporal")
temporal_router = APIRouter(prefix="/temporal")

_neo4j_driver = neo4j_driver


def _open_session(driver: Neo4jDriver) -> Neo4jSession:
    """Open a neo4j session with explicit typing.

    The neo4j Driver.session() method has untyped **kwargs in its stub,
    which causes reportUnknownMemberType under strict pyright. This helper
    isolates the workaround so every call site stays clean.
    """
    _driver: Any = driver
    session: Neo4jSession = _driver.session()
    return session


TEMPORAL_SCHEMA = """
CREATE CONSTRAINT episode_id IF NOT EXISTS FOR (e:Episode) REQUIRE e.id IS UNIQUE;
CREATE INDEX episode_occurred IF NOT EXISTS FOR (e:Episode) ON (e.occurred_at);
CREATE INDEX episode_recorded IF NOT EXISTS FOR (e:Episode) ON (e.recorded_at);
CREATE INDEX temporal_edge_valid IF NOT EXISTS FOR ()-[r:TEMPORAL_FACT]-() ON (r.valid_from);
"""


async def ensure_temporal_schema() -> None:
    if not neo4j_available():
        logger.info("Neo4j unavailable -- skipping temporal schema setup")
        return
    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            for stmt in TEMPORAL_SCHEMA.strip().split(";"):
                stmt = stmt.strip()
                if stmt:
                    with contextlib.suppress(Exception):
                        session.run(stmt)
        driver.close()
        mark_neo4j_ok()
        logger.info("Temporal schema constraints ensured")
    except Exception as e:
        mark_neo4j_down()
        logger.warning("Temporal schema setup failed (non-fatal): %s", e)


# --- Models ---

class EpisodeCreate(BaseModel):
    content: str
    agent_id: str
    session_id: str | None = None
    occurred_at: str | None = None
    source: str = "conversation"
    entities: list[str] = Field(default_factory=list)
    metadata: dict[str, Any] = Field(default_factory=dict)


class TemporalFactCreate(BaseModel):
    subject: str
    predicate: str
    object: str
    occurred_at: str | None = None
    confidence: float = Field(default=0.9, ge=0.0, le=1.0)
    source_episode_id: str | None = None


class TemporalQuery(BaseModel):
    since: str | None = None
    until: str | None = None
    entity: str | None = None
    agent_id: str | None = None
    limit: int = Field(default=20, ge=1, le=100)


class InvalidateRequest(BaseModel):
    subject: str
    predicate: str
    object: str | None = None
    reason: str = ""


class InvalidateTextRequest(BaseModel):
    text: str
    user_id: str = "default"
    reason: str = "contradiction_detected"


INVALIDATE_TEXT_COLLECTION = "aletheia_memories"
INVALIDATE_TEXT_SIMILARITY_THRESHOLD = 0.80


@temporal_router.post("/facts/invalidate_text")
async def invalidate_text(req: InvalidateTextRequest, request: Request) -> dict[str, Any]:
    """Invalidate a temporal fact by semantic matching against free-form text.

    Embeds the contradiction string, searches Qdrant for the most similar
    active temporal fact (cosine similarity >= 0.80), then marks it invalid
    in Neo4j (valid_to = now) and flags it in Qdrant payload.

    Anti-pattern: Does NOT parse contradiction strings into triples.
    Semantic embedding search is used instead.
    """
    mem: Any = getattr(request.app.state, "memory", None)
    if mem is None:
        raise HTTPException(status_code=503, detail="Memory not initialized")

    text = req.text.strip()
    if not text:
        raise HTTPException(status_code=400, detail="text must not be empty")

    try:
        embedder: Any = mem.embedding_model
        vector: list[float] = await asyncio.to_thread(embedder.embed, text)
    except Exception as e:
        logger.warning("invalidate_text: embedding failed: %s", e)
        raise HTTPException(status_code=503, detail="Embedding unavailable") from e

    try:
        client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
        results = client.query_points(
            collection_name=INVALIDATE_TEXT_COLLECTION,
            query=vector,
            query_filter=Filter(
                must=[
                    FieldCondition(key="user_id", match=MatchValue(value=req.user_id)),
                ],
                must_not=[
                    FieldCondition(key="invalidated", match=MatchValue(value=True)),
                ],
            ),
            limit=1,
            with_payload=True,
        )
    except Exception as e:
        logger.warning("invalidate_text: Qdrant search failed: %s", e)
        raise HTTPException(status_code=503, detail="Vector search unavailable") from e

    if not results.points or results.points[0].score < INVALIDATE_TEXT_SIMILARITY_THRESHOLD:
        logger.info(
            "invalidate_text: no match above threshold %.2f for: %.80s",
            INVALIDATE_TEXT_SIMILARITY_THRESHOLD,
            text,
        )
        return {"invalidated": False, "reason": "no_match_above_threshold"}

    matched = results.points[0]
    matched_payload: dict[str, Any] = matched.payload or {}
    matched_text: str = matched_payload.get("data") or matched_payload.get("memory") or ""
    similarity: float = matched.score
    matched_id = matched.id

    now = datetime.now(UTC).isoformat()

    # Mark invalidated in Qdrant payload
    try:
        client.set_payload(
            collection_name=INVALIDATE_TEXT_COLLECTION,
            payload={
                "invalidated": True,
                "invalidated_reason": req.reason,
                "invalidated_at": now,
            },
            points=[matched_id],
        )
    except Exception as e:
        logger.warning("invalidate_text: Qdrant payload update failed: %s", e)

    # Mark invalidated in Neo4j temporal facts (best-effort, non-blocking)
    if neo4j_available():
        try:
            driver = neo4j_driver()
            with _open_session(driver) as session:
                session.run(
                    """
                    MATCH ()-[r:TEMPORAL_FACT]->()
                    WHERE r.valid_to IS NULL AND r.qdrant_id = $qdrant_id
                    SET r.valid_to = $now, r.invalidation_reason = $reason
                    """,
                    qdrant_id=str(matched_id),
                    now=now,
                    reason=req.reason,
                )
                # Also try subject/object match via matched memory text (best-effort)
                if matched_text:
                    session.run(
                        """
                        MATCH (s)-[r:TEMPORAL_FACT]->(o)
                        WHERE r.valid_to IS NULL
                          AND (toLower(s.name + ' ' + r.predicate + ' ' + o.name) CONTAINS toLower($fragment)
                               OR toLower(o.name) CONTAINS toLower($fragment))
                        SET r.valid_to = $now, r.invalidation_reason = $reason
                        """,
                        fragment=matched_text[:100],
                        now=now,
                        reason=req.reason,
                    )
            driver.close()
            mark_neo4j_ok()
        except Exception as e:
            mark_neo4j_down()
            logger.warning("invalidate_text: Neo4j update failed (non-fatal): %s", e)

    logger.info(
        "invalidate_text: invalidated id=%s similarity=%.3f reason=%s text=%.80s",
        matched_id,
        similarity,
        req.reason,
        text,
    )
    return {
        "invalidated": True,
        "matched_text": matched_text,
        "similarity": similarity,
    }


# --- Episode endpoints ---

@temporal_router.post("/episodes")
async def create_episode(req: EpisodeCreate) -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    episode_id = f"ep_{uuid.uuid4().hex[:12]}"
    now = datetime.now(UTC).isoformat()
    occurred = req.occurred_at or now

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            session.run(
                """
                CREATE (e:Episode {
                    id: $id,
                    content_preview: $preview,
                    agent_id: $agent_id,
                    session_id: $session_id,
                    source: $source,
                    occurred_at: $occurred_at,
                    recorded_at: $recorded_at
                })
                """,
                id=episode_id,
                preview=req.content[:500],
                agent_id=req.agent_id,
                session_id=req.session_id or "",
                source=req.source,
                occurred_at=occurred,
                recorded_at=now,
            )

            for entity_name in req.entities[:20]:
                session.run(
                    """
                    MERGE (ent:Entity {name: $name})
                    WITH ent
                    MATCH (ep:Episode {id: $ep_id})
                    CREATE (ep)-[:MENTIONS {occurred_at: $occurred_at}]->(ent)
                    """,
                    name=entity_name,
                    ep_id=episode_id,
                    occurred_at=occurred,
                )
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "episode_id": episode_id,
            "entities_linked": len(req.entities[:20]),
            "occurred_at": occurred,
            "recorded_at": now,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("create_episode failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "reason": "graph_unavailable"}


@temporal_router.get("/episodes")
async def list_episodes(
    since: str | None = None,
    until: str | None = None,
    agent_id: str | None = None,
    entity: str | None = None,
    limit: int = 20,
) -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False, "episodes": [], "count": 0}

    conditions: list[str] = []
    params: dict[str, Any] = {"limit": min(limit, 100)}

    if since:
        conditions.append("e.occurred_at >= $since")
        params["since"] = since
    if until:
        conditions.append("e.occurred_at <= $until")
        params["until"] = until
    if agent_id:
        conditions.append("e.agent_id = $agent_id")
        params["agent_id"] = agent_id

    where = "WHERE " + " AND ".join(conditions) if conditions else ""

    if entity:
        query = (
            "MATCH (e:Episode)-[:MENTIONS]->(ent:Entity) "
            "WHERE toLower(ent.name) CONTAINS toLower($entity) "
            + (f"AND {' AND '.join(conditions)} " if conditions else "")
            + "RETURN e ORDER BY e.occurred_at DESC LIMIT $limit"
        )
        params["entity"] = entity
    else:
        query = f"MATCH (e:Episode) {where} RETURN e ORDER BY e.occurred_at DESC LIMIT $limit"

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            result = session.run(cast("LiteralString", query), **params)
            episodes: list[dict[str, Any]] = []
            for record in result:
                node: Any = record["e"]
                episodes.append({
                    "id": node["id"],
                    "content_preview": node.get("content_preview", ""),
                    "agent_id": node.get("agent_id", ""),
                    "session_id": node.get("session_id", ""),
                    "source": node.get("source", ""),
                    "occurred_at": str(node.get("occurred_at", "")),
                    "recorded_at": str(node.get("recorded_at", "")),
                })
        driver.close()
        mark_neo4j_ok()
        return {"ok": True, "episodes": episodes, "count": len(episodes)}
    except Exception as e:
        mark_neo4j_down()
        logger.warning("list_episodes failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "episodes": [], "count": 0}


# --- Temporal fact endpoints ---

@temporal_router.post("/facts")
async def create_temporal_fact(req: TemporalFactCreate) -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    now = datetime.now(UTC).isoformat()
    occurred = req.occurred_at or now

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            single_result = session.run(
                """
                MATCH (s:Entity {name: $subject})-[r:TEMPORAL_FACT]->(o)
                WHERE r.predicate = $predicate AND r.valid_to IS NULL
                SET r.valid_to = $now, r.invalidated_by = $new_object
                RETURN count(r) AS invalidated
                """,
                subject=req.subject,
                predicate=req.predicate,
                now=now,
                new_object=req.object,
            ).single()
            invalidated: Any = single_result["invalidated"] if single_result is not None else 0

            session.run(
                """
                MERGE (s:Entity {name: $subject})
                MERGE (o:Entity {name: $object})
                CREATE (s)-[:TEMPORAL_FACT {
                    predicate: $predicate,
                    valid_from: $valid_from,
                    valid_to: null,
                    occurred_at: $occurred_at,
                    recorded_at: $recorded_at,
                    confidence: $confidence,
                    source_episode_id: $source_ep
                }]->(o)
                """,
                subject=req.subject,
                object=req.object,
                predicate=req.predicate,
                valid_from=now,
                occurred_at=occurred,
                recorded_at=now,
                confidence=req.confidence,
                source_ep=req.source_episode_id or "",
            )

            if req.source_episode_id:
                session.run(
                    """
                    MATCH (ep:Episode {id: $ep_id})
                    MATCH (s:Entity {name: $subject})-[r:TEMPORAL_FACT]->(o:Entity {name: $object})
                    WHERE r.recorded_at = $recorded_at
                    CREATE (ep)-[:PRODUCED]->(s)
                    """,
                    ep_id=req.source_episode_id,
                    subject=req.subject,
                    object=req.object,
                    recorded_at=now,
                )
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "fact": f"{req.subject} {req.predicate} {req.object}",
            "valid_from": now,
            "invalidated_previous": invalidated,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("create_temporal_fact failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "reason": "graph_unavailable"}


@temporal_router.post("/facts/invalidate")
async def invalidate_fact(req: InvalidateRequest) -> dict[str, Any]:
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    now = datetime.now(UTC).isoformat()

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            conditions = "r.predicate = $predicate AND r.valid_to IS NULL"
            params: dict[str, Any] = {
                "subject": req.subject,
                "predicate": req.predicate,
                "now": now,
                "reason": req.reason,
            }
            if req.object:
                conditions += " AND o.name = $object"
                params["object"] = req.object

            single_result = session.run(
                f"""
                MATCH (s:Entity {{name: $subject}})-[r:TEMPORAL_FACT]->(o)
                WHERE {conditions}
                SET r.valid_to = $now, r.invalidation_reason = $reason
                RETURN count(r) AS invalidated,
                       collect(o.name) AS objects
                """,
                **params,
            ).single()
            invalidated: Any = single_result["invalidated"] if single_result is not None else 0
            objects: Any = single_result["objects"] if single_result is not None else []
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "invalidated": invalidated,
            "subject": req.subject,
            "predicate": req.predicate,
            "affected_objects": objects,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("invalidate_fact failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "reason": "graph_unavailable"}


# --- Temporal query endpoints ---

@temporal_router.post("/since")
async def query_since(req: TemporalQuery) -> dict[str, Any]:
    """What's changed since a given time? Returns new facts and invalidated facts."""
    if not req.since:
        raise HTTPException(status_code=400, detail="'since' timestamp required")

    if not neo4j_available():
        return {"ok": False, "available": False, "new_facts": [], "invalidated_facts": [], "new_episodes": []}

    params: dict[str, Any] = {"since": req.since, "limit": req.limit}
    entity_filter = ""
    if req.entity:
        entity_filter = "AND (toLower(s.name) CONTAINS toLower($entity) OR toLower(o.name) CONTAINS toLower($entity))"
        params["entity"] = req.entity

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            new_facts: list[dict[str, Any]] = session.run(
                f"""
                MATCH (s)-[r:TEMPORAL_FACT]->(o)
                WHERE r.recorded_at >= $since {entity_filter}
                RETURN s.name AS subject, r.predicate AS predicate, o.name AS object,
                       r.valid_from AS valid_from, r.confidence AS confidence,
                       r.recorded_at AS recorded_at
                ORDER BY r.recorded_at DESC
                LIMIT $limit
                """,
                **params,
            ).data()

            invalidated: list[dict[str, Any]] = session.run(
                f"""
                MATCH (s)-[r:TEMPORAL_FACT]->(o)
                WHERE r.valid_to IS NOT NULL AND r.valid_to >= $since {entity_filter}
                RETURN s.name AS subject, r.predicate AS predicate, o.name AS object,
                       r.valid_from AS valid_from, r.valid_to AS valid_to,
                       r.invalidation_reason AS reason
                ORDER BY r.valid_to DESC
                LIMIT $limit
                """,
                **params,
            ).data()

            ep_params: dict[str, Any] = {"since": req.since, "limit": req.limit}
            ep_filter = ""
            if req.agent_id:
                ep_filter = "AND e.agent_id = $agent_id"
                ep_params["agent_id"] = req.agent_id
            episodes: list[dict[str, Any]] = session.run(
                f"""
                MATCH (e:Episode)
                WHERE e.recorded_at >= $since {ep_filter}
                RETURN e.id AS id, e.agent_id AS agent_id, e.source AS source,
                       e.occurred_at AS occurred_at, e.content_preview AS preview
                ORDER BY e.occurred_at DESC
                LIMIT $limit
                """,
                **ep_params,
            ).data()
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "since": req.since,
            "new_facts": new_facts,
            "invalidated_facts": invalidated,
            "new_episodes": episodes,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("query_since failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "new_facts": [], "invalidated_facts": [], "new_episodes": []}


@temporal_router.post("/what_changed")
async def what_changed(req: TemporalQuery) -> dict[str, Any]:
    """What changed for a specific entity over time?"""
    if not req.entity:
        raise HTTPException(status_code=400, detail="'entity' required")

    if not neo4j_available():
        return {"ok": False, "available": False, "active_facts": [], "historical_facts": [], "episodes": []}

    params: dict[str, Any] = {"entity": req.entity, "limit": req.limit}
    time_filter = ""
    if req.since:
        time_filter += " AND r.recorded_at >= $since"
        params["since"] = req.since
    if req.until:
        time_filter += " AND r.recorded_at <= $until"
        params["until"] = req.until

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            facts: list[dict[str, Any]] = session.run(
                f"""
                MATCH (s)-[r:TEMPORAL_FACT]->(o)
                WHERE (toLower(s.name) CONTAINS toLower($entity)
                       OR toLower(o.name) CONTAINS toLower($entity))
                {time_filter}
                RETURN s.name AS subject, r.predicate AS predicate, o.name AS object,
                       r.valid_from AS valid_from, r.valid_to AS valid_to,
                       r.confidence AS confidence, r.recorded_at AS recorded_at,
                       r.invalidation_reason AS reason
                ORDER BY r.recorded_at DESC
                LIMIT $limit
                """,
                **params,
            ).data()

            ep_params: dict[str, Any] = {"entity": req.entity, "limit": req.limit}
            ep_time_filter = ""
            if req.since:
                ep_time_filter += " AND e.occurred_at >= $since"
                ep_params["since"] = req.since
            episodes: list[dict[str, Any]] = session.run(
                f"""
                MATCH (e:Episode)-[:MENTIONS]->(ent:Entity)
                WHERE toLower(ent.name) CONTAINS toLower($entity) {ep_time_filter}
                RETURN e.id AS id, e.agent_id AS agent_id, e.source AS source,
                       e.occurred_at AS occurred_at, e.content_preview AS preview
                ORDER BY e.occurred_at DESC
                LIMIT $limit
                """,
                **ep_params,
            ).data()
        driver.close()
        mark_neo4j_ok()

        active: list[dict[str, Any]] = [f for f in facts if f.get("valid_to") is None]
        historical: list[dict[str, Any]] = [f for f in facts if f.get("valid_to") is not None]

        return {
            "ok": True,
            "entity": req.entity,
            "active_facts": active,
            "historical_facts": historical,
            "episodes": episodes,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("what_changed failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "active_facts": [], "historical_facts": [], "episodes": []}


@temporal_router.post("/at_time")
async def knowledge_at_time(req: TemporalQuery) -> dict[str, Any]:
    """What was the state of knowledge at a specific point in time?"""
    timestamp = req.until or req.since
    if not timestamp:
        raise HTTPException(status_code=400, detail="'since' or 'until' timestamp required (used as point-in-time)")

    if not neo4j_available():
        return {"ok": False, "available": False, "facts": [], "count": 0}

    params: dict[str, Any] = {"timestamp": timestamp, "limit": req.limit}
    entity_filter = ""
    if req.entity:
        entity_filter = "AND (toLower(s.name) CONTAINS toLower($entity) OR toLower(o.name) CONTAINS toLower($entity))"
        params["entity"] = req.entity

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            facts: list[dict[str, Any]] = session.run(
                f"""
                MATCH (s)-[r:TEMPORAL_FACT]->(o)
                WHERE r.valid_from <= $timestamp
                  AND (r.valid_to IS NULL OR r.valid_to > $timestamp)
                  {entity_filter}
                RETURN s.name AS subject, r.predicate AS predicate, o.name AS object,
                       r.valid_from AS valid_from, r.confidence AS confidence
                ORDER BY r.valid_from DESC
                LIMIT $limit
                """,
                **params,
            ).data()
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "at_time": timestamp,
            "facts": facts,
            "count": len(facts),
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("knowledge_at_time failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False, "facts": [], "count": 0}


@temporal_router.get("/stats")
async def temporal_stats() -> dict[str, Any]:
    """Statistics on the temporal knowledge graph."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        with _open_session(driver) as session:
            ep_record = session.run("MATCH (e:Episode) RETURN count(e) AS c").single()
            episodes: Any = ep_record["c"] if ep_record is not None else 0
            af_record = session.run(
                "MATCH ()-[r:TEMPORAL_FACT]->() WHERE r.valid_to IS NULL RETURN count(r) AS c"
            ).single()
            active_facts: Any = af_record["c"] if af_record is not None else 0
            hf_record = session.run(
                "MATCH ()-[r:TEMPORAL_FACT]->() WHERE r.valid_to IS NOT NULL RETURN count(r) AS c"
            ).single()
            historical_facts: Any = hf_record["c"] if hf_record is not None else 0
            mn_record = session.run(
                "MATCH ()-[r:MENTIONS]->() RETURN count(r) AS c"
            ).single()
            mentions: Any = mn_record["c"] if mn_record is not None else 0

            recent: list[dict[str, Any]] = session.run(
                "MATCH (e:Episode) RETURN e.agent_id AS agent, e.source AS source, "
                "e.occurred_at AS occurred_at ORDER BY e.occurred_at DESC LIMIT 5"
            ).data()

            top_mentioned: list[dict[str, Any]] = session.run(
                "MATCH (e:Episode)-[:MENTIONS]->(ent:Entity) "
                "RETURN ent.name AS entity, count(e) AS mentions "
                "ORDER BY mentions DESC LIMIT 10"
            ).data()
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "episodes": episodes,
            "active_facts": active_facts,
            "historical_facts": historical_facts,
            "total_facts": active_facts + historical_facts,
            "mentions": mentions,
            "recent_episodes": recent,
            "top_mentioned_entities": top_mentioned,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("temporal_stats failed (Neo4j may be down): %s", e)
        return {"ok": False, "available": False}
