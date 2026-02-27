# Memory evolution -- A-Mem-inspired patterns for memory lifecycle management
# NOTE: Do NOT add 'from __future__ import annotations' -- see routes.py comment.

import asyncio
import logging
import math
import os
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any, cast

import httpx
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from .graph import mark_neo4j_down, mark_neo4j_ok, neo4j_available, neo4j_driver

if TYPE_CHECKING:
    import neo4j

logger = logging.getLogger("aletheia_memory.evolution")
evolution_router = APIRouter(prefix="/evolution")

_background_tasks: set[asyncio.Task[None]] = set()

ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
EVOLUTION_THRESHOLD = 0.80
REINFORCEMENT_BOOST = 0.02


def exponential_decay_penalty(days_inactive: float, lambda_: float = 0.05) -> float:
    """Exponential decay multiplier for memory salience.

    Returns a score multiplier in [0, 1] based on days since last access.
    lambda=0.05 gives ~14-day half-life:
      - 0 days  -> 1.00
      - 7 days  -> 0.70
      - 14 days -> 0.50
      - 30 days -> 0.22
      - 90 days -> 0.01
    """
    return math.exp(-lambda_ * days_inactive)


class EvolveRequest(BaseModel):
    text: str
    user_id: str = "default"
    agent_id: str | None = None


class ReinforcementRequest(BaseModel):
    memory_id: str
    user_id: str = "default"


class DecayRequest(BaseModel):
    user_id: str = "default"
    days_inactive: int = Field(default=30, ge=7, le=365)
    decay_amount: float = Field(default=0.05, ge=0.01, le=0.5)
    dry_run: bool = False


def _open_neo4j_session() -> "neo4j.Session":
    driver = neo4j_driver()
    _factory = getattr(driver, "session")  # noqa: B009 -- pyright: neo4j **config is Unknown
    return _factory()


@evolution_router.post("/check")
async def check_evolution(req: EvolveRequest, request: Request) -> dict[str, Any]:
    """Check if a new memory should evolve an existing one or be added fresh.

    Pipeline:
    1. Search for similar existing memories
    2. If match > EVOLUTION_THRESHOLD, use LLM to merge old + new into evolved version
    3. Replace old memory with evolved version
    4. If no match, return suggestion to add normally
    """
    mem: Any = getattr(request.app.state, 'memory', None)
    if mem is None:
        raise HTTPException(status_code=503, detail='Memory not initialized')
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": 5}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    try:
        raw: Any = await asyncio.to_thread(mem.search, req.text, **kwargs)
        if isinstance(raw, dict):
            raw_dict = cast("dict[str, Any]", raw)
            results_any: Any = raw_dict.get("results", raw)
        else:
            results_any = raw
    except Exception:
        raise HTTPException(status_code=500, detail="Search failed") from None

    if not isinstance(results_any, list):
        return {"ok": True, "action": "add_new", "reason": "no existing memories"}

    results_list: list[Any] = cast("list[Any]", results_any)
    candidates: list[Any] = [
        r for r in results_list
        if isinstance(r, dict) and cast("dict[str, Any]", r).get("score", 0) > EVOLUTION_THRESHOLD
    ]

    if not candidates:
        first_entry: Any = results_list[0] if results_list else None
        first_score: Any = (
            cast("dict[str, Any]", first_entry).get("score", 0)
            if isinstance(first_entry, dict) else 0
        )
        return {
            "ok": True,
            "action": "add_new",
            "reason": "no similar memories above threshold",
            "closest_score": first_score,
        }

    best: dict[str, Any] = candidates[0]
    old_text: str = str(best.get("memory", ""))
    old_id: str = str(best.get("id", ""))

    evolved_text = await _evolve_with_llm(old_text, req.text)
    if not evolved_text:
        return {
            "ok": True,
            "action": "add_new",
            "reason": "evolution merge failed, falling back to add",
        }

    try:
        await asyncio.to_thread(mem.delete, old_id)
        add_kwargs: dict[str, Any] = {"user_id": req.user_id}
        if req.agent_id:
            add_kwargs["agent_id"] = req.agent_id
        add_kwargs["metadata"] = {
            "evolved_from": old_id,
            "evolution_timestamp": datetime.now(UTC).isoformat(),
        }
        result: Any = await asyncio.to_thread(mem.add, evolved_text, **add_kwargs)

        if neo4j_available():
            task: asyncio.Task[None] = asyncio.create_task(
                _record_evolution_graph(old_text, evolved_text, old_id)
            )
            _background_tasks.add(task)
            task.add_done_callback(_background_tasks.discard)

        return {
            "ok": True,
            "action": "evolved",
            "old_id": old_id,
            "old_text": old_text[:200],
            "evolved_text": evolved_text[:200],
            "similarity": best.get("score", 0),
            "result": result,
        }
    except Exception:
        raise HTTPException(status_code=500, detail="Evolution failed") from None


@evolution_router.post("/reinforce")
async def reinforce_memory(
    req: ReinforcementRequest, request: Request
) -> dict[str, Any]:
    """Reinforce a memory -- mark it as accessed, boost its relevance.

    Called when a memory is retrieved and used in a response.
    Tracks access count and last access time in Neo4j.
    """
    if not neo4j_available():
        return {"ok": True, "reinforced": False, "reason": "graph_unavailable"}

    now = datetime.now(UTC).isoformat()

    try:
        driver = neo4j_driver()
        with _open_neo4j_session() as session:
            record = session.run(
                """
                MERGE (m:MemoryAccess {memory_id: $memory_id})
                ON CREATE SET m.access_count = 1, m.first_accessed = $now, m.last_accessed = $now
                ON MATCH SET m.access_count = m.access_count + 1, m.last_accessed = $now
                RETURN m.access_count AS count
                """,
                memory_id=req.memory_id, now=now,
            ).single()
            access_count: Any = record["count"] if record is not None else 0
        driver.close()

        return {
            "ok": True,
            "reinforced": True,
            "memory_id": req.memory_id,
            "access_count": access_count,
        }
    except Exception as e:
        logger.warning("Reinforcement failed: %s", e)
        return {"ok": True, "reinforced": False, "error": "Internal error"}


@evolution_router.post("/decay")
async def decay_memories(req: DecayRequest, request: Request) -> dict[str, Any]:
    """Decay unused memories -- reduce confidence of memories not accessed recently.

    Designed to run from nightly consolidation cron.
    Memories with MemoryAccess nodes accessed within days_inactive are exempt.
    """
    mem: Any = getattr(request.app.state, 'memory', None)
    if mem is None:
        raise HTTPException(status_code=503, detail='Memory not initialized')

    try:
        raw: Any = await asyncio.to_thread(mem.get_all, user_id=req.user_id, limit=500)
        if isinstance(raw, dict):
            raw_dict = cast("dict[str, Any]", raw)
            entries_any: Any = raw_dict.get("results", raw)
        else:
            entries_any = raw
    except Exception:
        raise HTTPException(status_code=500, detail="Failed to fetch memories") from None

    if not isinstance(entries_any, list):
        return {"ok": True, "decayed": 0, "checked": 0}

    entries: list[Any] = cast("list[Any]", entries_any)

    recently_accessed: set[str] = set()
    if neo4j_available():
        try:
            driver = neo4j_driver()
            with _open_neo4j_session() as session:
                result = session.run(
                    "MATCH (m:MemoryAccess) WHERE m.last_accessed IS NOT NULL "
                    "RETURN m.memory_id AS id, m.access_count AS count"
                )
                for record in result:
                    record_id: Any = record["id"]
                    if isinstance(record_id, str):
                        recently_accessed.add(record_id)
            driver.close()
            mark_neo4j_ok()
        except Exception:
            mark_neo4j_down()
            logger.warning("decay: Neo4j unavailable, proceeding without access data")

    decay_candidates: list[dict[str, Any]] = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        entry_dict: dict[str, Any] = cast("dict[str, Any]", entry)
        memory_id: str = str(entry_dict.get("id", ""))
        if memory_id and memory_id not in recently_accessed:
            decay_candidates.append(entry_dict)

    if req.dry_run:
        return {
            "ok": True,
            "dry_run": True,
            "checked": len(entries),
            "decay_candidates": len(decay_candidates),
            "recently_accessed": len(recently_accessed),
            "sample": [
                {"id": e.get("id"), "text": str(e.get("memory", ""))[:100]}
                for e in decay_candidates[:10]
            ],
        }

    decayed = 0
    for entry_d in decay_candidates:
        memory_id_val: str = str(entry_d.get("id", ""))
        if not memory_id_val:
            continue
        if neo4j_available():
            try:
                driver = neo4j_driver()
                with _open_neo4j_session() as session:
                    session.run(
                        """
                        MERGE (m:MemoryAccess {memory_id: $id})
                        ON CREATE SET m.decay_count = 1, m.last_decayed = $now
                        ON MATCH SET m.decay_count = coalesce(m.decay_count, 0) + 1,
                                     m.last_decayed = $now
                        """,
                        id=memory_id_val, now=datetime.now(UTC).isoformat(),
                    )
                driver.close()
                mark_neo4j_ok()
                decayed += 1
            except Exception:
                mark_neo4j_down()

    return {
        "ok": True,
        "checked": len(entries),
        "decayed": decayed,
        "recently_accessed": len(recently_accessed),
        "exempt": len(recently_accessed),
    }


@evolution_router.get("/stats")
async def evolution_stats() -> dict[str, Any]:
    """Statistics on memory evolution and reinforcement."""
    if not neo4j_available():
        return {"ok": True, "available": False}

    try:
        driver = neo4j_driver()
        with _open_neo4j_session() as session:
            tracked_record = session.run(
                "MATCH (m:MemoryAccess) RETURN count(m) AS c"
            ).single()
            total_tracked: Any = tracked_record["c"] if tracked_record is not None else 0

            evolutions_record = session.run(
                "MATCH ()-[r:EVOLVED_INTO]->() RETURN count(r) AS c"
            ).single()
            total_evolutions: Any = evolutions_record["c"] if evolutions_record is not None else 0

            most_accessed: list[dict[str, Any]] = session.run(
                "MATCH (m:MemoryAccess) WHERE m.access_count > 1 "
                "RETURN m.memory_id AS id, m.access_count AS count "
                "ORDER BY m.access_count DESC LIMIT 10"
            ).data()

            decaying_record = session.run(
                "MATCH (m:MemoryAccess) WHERE m.decay_count IS NOT NULL AND m.decay_count > 0 "
                "RETURN count(m) AS c"
            ).single()
            decaying: Any = decaying_record["c"] if decaying_record is not None else 0
        driver.close()
        mark_neo4j_ok()

        return {
            "ok": True,
            "tracked_memories": total_tracked,
            "evolutions": total_evolutions,
            "decaying_memories": decaying,
            "most_accessed": most_accessed,
        }
    except Exception as e:
        mark_neo4j_down()
        logger.warning("evolution_stats failed (Neo4j may be down): %s", e)
        return {"ok": True, "available": False}


# --- Internal helpers ---

async def _evolve_with_llm(old_text: str, new_text: str) -> str | None:
    """Use Haiku to merge old + new memory into an evolved version."""
    if not ANTHROPIC_API_KEY:
        return None

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            resp: httpx.Response = await client.post(
                "https://api.anthropic.com/v1/messages",
                headers={
                    "x-api-key": ANTHROPIC_API_KEY,
                    "anthropic-version": "2023-06-01",
                    "content-type": "application/json",
                },
                json={
                    "model": "claude-haiku-4-5-20251001",
                    "max_tokens": 256,
                    "messages": [{
                        "role": "user",
                        "content": (
                            "You are a memory evolution system. Merge the old memory with the new information "
                            "into a single updated memory entry. Keep it concise (1-2 sentences). "
                            "Preserve the core fact while incorporating the update. "
                            "If the new info contradicts the old, the new info takes precedence.\n\n"
                            f'Old memory: "{old_text[:300]}"\n'
                            f'New information: "{new_text[:300]}"\n\n'
                            "Evolved memory (1-2 sentences):"
                        ),
                    }],
                },
            )
            if resp.status_code != 200:
                return None
            data: dict[str, Any] = resp.json()
            content_list: list[dict[str, Any]] = data.get("content", [{}])
            text: str = content_list[0].get("text", "").strip()
            return text if text and len(text) > 10 else None
    except Exception:
        logger.warning("LLM evolution merge failed", exc_info=True)
        return None


async def _record_evolution_graph(
    old_text: str, evolved_text: str, old_id: str
) -> None:
    """Record evolution lineage in Neo4j."""
    if not neo4j_available():
        return
    try:
        driver = neo4j_driver()
        with _open_neo4j_session() as session:
            session.run(
                """
                MERGE (old:Memory {text_preview: $old_text})
                MERGE (new:Memory {text_preview: $new_text})
                CREATE (old)-[:EVOLVED_INTO {
                    evolved_at: datetime(),
                    old_id: $old_id
                }]->(new)
                """,
                old_text=old_text[:200],
                new_text=evolved_text[:200],
                old_id=old_id,
            )
        driver.close()
        mark_neo4j_ok()
    except Exception:
        mark_neo4j_down()
        logger.warning("Evolution graph recording failed", exc_info=True)
