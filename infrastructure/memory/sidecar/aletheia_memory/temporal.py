# Temporal memory layer — Graphiti-inspired episode tracking and bi-temporal queries
# NOTE: Do NOT add future annotations — see routes.py comment.

import logging
import uuid
from datetime import datetime, timezone
from typing import Any

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from .graph import neo4j_driver, neo4j_available, mark_neo4j_ok, mark_neo4j_down

logger = logging.getLogger("aletheia_memory.temporal")
temporal_router = APIRouter(prefix="/temporal")

# Re-export for routes.py backward compat
_neo4j_driver = neo4j_driver


def _extract_entities_for_episode(text: str) -> list[str]:
    """Extract entity names from text for episode linking."""
    import re
    entities: list[str] = []
    for match in re.finditer(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b", text):
        entities.append(match.group())
    for match in re.finditer(r"\b[a-z]+[-_][a-z]+(?:[-_][a-z]+)*\b", text):
        entities.append(match.group())
    return list(set(entities))[:15]


# --- Schema bootstrap (idempotent) ---

TEMPORAL_SCHEMA = """
CREATE CONSTRAINT episode_id IF NOT EXISTS FOR (e:Episode) REQUIRE e.id IS UNIQUE;
CREATE INDEX episode_occurred IF NOT EXISTS FOR (e:Episode) ON (e.occurred_at);
CREATE INDEX episode_recorded IF NOT EXISTS FOR (e:Episode) ON (e.recorded_at);
CREATE INDEX temporal_edge_valid IF NOT EXISTS FOR ()-[r:TEMPORAL_FACT]-() ON (r.valid_from);
"""


async def ensure_temporal_schema():
    if not neo4j_available():
        logger.info("Neo4j unavailable — skipping temporal schema setup")
        return
    try:
        driver = neo4j_driver()
        with driver.session() as session:
            for stmt in TEMPORAL_SCHEMA.strip().split(";"):
                stmt = stmt.strip()
                if stmt:
                    try:
                        session.run(stmt)
                    except Exception:
                        pass  # constraint may already exist
        driver.close()
        mark_neo4j_ok()
        logger.info("Temporal schema constraints ensured")
    except Exception as e:
        mark_neo4j_down()
        logger.warning(f"Temporal schema setup failed (non-fatal): {e}")


# --- Models ---

class EpisodeCreate(BaseModel):
    content: str
    agent_id: str
    session_id: str | None = None
    occurred_at: str | None = None  # ISO datetime — when event happened
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
    since: str | None = None  # ISO datetime
    until: str | None = None  # ISO datetime
    entity: str | None = None
    agent_id: str | None = None
    limit: int = Field(default=20, ge=1, le=100)


class InvalidateRequest(BaseModel):
    subject: str
    predicate: str
    object: str | None = None
    reason: str = ""


# --- Episode endpoints ---

@temporal_router.post("/episodes")
async def create_episode(req: EpisodeCreate):
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    episode_id = f"ep_{uuid.uuid4().hex[:12]}"
    now = datetime.now(timezone.utc).isoformat()
    occurred = req.occurred_at or now

    try:
        driver = neo4j_driver()
        with driver.session() as session:
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
):
    if not neo4j_available():
        return {"ok": False, "available": False, "episodes": [], "count": 0}

    conditions = []
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
            f"MATCH (e:Episode)-[:MENTIONS]->(ent:Entity) "
            f"WHERE toLower(ent.name) CONTAINS toLower($entity) "
            + (f"AND {' AND '.join(conditions)} " if conditions else "")
            + "RETURN e ORDER BY e.occurred_at DESC LIMIT $limit"
        )
        params["entity"] = entity
    else:
        query = f"MATCH (e:Episode) {where} RETURN e ORDER BY e.occurred_at DESC LIMIT $limit"

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            result = session.run(query, **params)
            episodes = []
            for record in result:
                node = record["e"]
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
async def create_temporal_fact(req: TemporalFactCreate):
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    now = datetime.now(timezone.utc).isoformat()
    occurred = req.occurred_at or now

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            invalidated = session.run(
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
            ).single()["invalidated"]

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
async def invalidate_fact(req: InvalidateRequest):
    if not neo4j_available():
        return {"ok": False, "available": False, "reason": "graph_unavailable"}

    now = datetime.now(timezone.utc).isoformat()

    try:
        driver = neo4j_driver()
        with driver.session() as session:
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

            result = session.run(
                f"""
                MATCH (s:Entity {{name: $subject}})-[r:TEMPORAL_FACT]->(o)
                WHERE {conditions}
                SET r.valid_to = $now, r.invalidation_reason = $reason
                RETURN count(r) AS invalidated,
                       collect(o.name) AS objects
                """,
                **params,
            ).single()
            invalidated = result["invalidated"]
            objects = result["objects"]
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
async def query_since(req: TemporalQuery):
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
        with driver.session() as session:
            new_facts = session.run(
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

            invalidated = session.run(
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
            episodes = session.run(
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
async def what_changed(req: TemporalQuery):
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
        with driver.session() as session:
            facts = session.run(
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
            episodes = session.run(
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

        active = [f for f in facts if f.get("valid_to") is None]
        historical = [f for f in facts if f.get("valid_to") is not None]

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
async def knowledge_at_time(req: TemporalQuery):
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
        with driver.session() as session:
            facts = session.run(
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
async def temporal_stats():
    """Statistics on the temporal knowledge graph."""
    if not neo4j_available():
        return {"ok": False, "available": False}

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            episodes = session.run("MATCH (e:Episode) RETURN count(e) AS c").single()["c"]
            active_facts = session.run(
                "MATCH ()-[r:TEMPORAL_FACT]->() WHERE r.valid_to IS NULL RETURN count(r) AS c"
            ).single()["c"]
            historical_facts = session.run(
                "MATCH ()-[r:TEMPORAL_FACT]->() WHERE r.valid_to IS NOT NULL RETURN count(r) AS c"
            ).single()["c"]
            mentions = session.run(
                "MATCH ()-[r:MENTIONS]->() RETURN count(r) AS c"
            ).single()["c"]

            recent = session.run(
                "MATCH (e:Episode) RETURN e.agent_id AS agent, e.source AS source, "
                "e.occurred_at AS occurred_at ORDER BY e.occurred_at DESC LIMIT 5"
            ).data()

            top_mentioned = session.run(
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
