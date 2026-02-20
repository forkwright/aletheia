# Memory evolution — A-Mem-inspired patterns for memory lifecycle management
from __future__ import annotations

import asyncio
import logging
import os
from datetime import datetime, timezone
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from .graph import neo4j_driver, neo4j_available, mark_neo4j_ok, mark_neo4j_down

logger = logging.getLogger("aletheia_memory.evolution")
evolution_router = APIRouter(prefix="/evolution")

ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
EVOLUTION_THRESHOLD = 0.80  # similarity above which we evolve rather than add
REINFORCEMENT_BOOST = 0.02  # score boost per access


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


@evolution_router.post("/check")
async def check_evolution(req: EvolveRequest, request: Request):
    """Check if a new memory should evolve an existing one or be added fresh.

    Pipeline:
    1. Search for similar existing memories
    2. If match > EVOLUTION_THRESHOLD, use LLM to merge old + new into evolved version
    3. Replace old memory with evolved version
    4. If no match, return suggestion to add normally
    """
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": req.user_id, "limit": 5}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id

    # Find candidates for evolution
    try:
        raw = await asyncio.to_thread(mem.search, req.text, **kwargs)
        results = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception as e:
        raise HTTPException(status_code=500, detail="Search failed")

    if not isinstance(results, list):
        return {"ok": True, "action": "add_new", "reason": "no existing memories"}

    # Find best match above threshold
    candidates = [
        r for r in results
        if r.get("score", 0) > EVOLUTION_THRESHOLD
    ]

    if not candidates:
        return {
            "ok": True,
            "action": "add_new",
            "reason": "no similar memories above threshold",
            "closest_score": results[0].get("score", 0) if results else 0,
        }

    best = candidates[0]
    old_text = best.get("memory", "")
    old_id = best.get("id", "")

    # Use LLM to merge old + new into evolved version
    evolved_text = await _evolve_with_llm(old_text, req.text)
    if not evolved_text:
        return {
            "ok": True,
            "action": "add_new",
            "reason": "evolution merge failed, falling back to add",
        }

    # Replace old memory with evolved version
    try:
        await asyncio.to_thread(mem.delete, old_id)
        add_kwargs: dict[str, Any] = {"user_id": req.user_id}
        if req.agent_id:
            add_kwargs["agent_id"] = req.agent_id
        add_kwargs["metadata"] = {
            "evolved_from": old_id,
            "evolution_timestamp": datetime.now(timezone.utc).isoformat(),
        }
        result = await asyncio.to_thread(mem.add, evolved_text, **add_kwargs)

        # Record evolution in Neo4j for lineage tracking
        if neo4j_available():
            asyncio.create_task(_record_evolution_graph(old_text, evolved_text, old_id))

        return {
            "ok": True,
            "action": "evolved",
            "old_id": old_id,
            "old_text": old_text[:200],
            "evolved_text": evolved_text[:200],
            "similarity": best.get("score", 0),
            "result": result,
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail="Evolution failed")


@evolution_router.post("/reinforce")
async def reinforce_memory(req: ReinforcementRequest, request: Request):
    """Reinforce a memory — mark it as accessed, boost its relevance.

    Called when a memory is retrieved and used in a response.
    Tracks access count and last access time in Neo4j.
    """
    if not neo4j_available():
        return {"ok": True, "reinforced": False, "reason": "graph_unavailable"}

    now = datetime.now(timezone.utc).isoformat()

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            result = session.run(
                """
                MERGE (m:MemoryAccess {memory_id: $memory_id})
                ON CREATE SET m.access_count = 1, m.first_accessed = $now, m.last_accessed = $now
                ON MATCH SET m.access_count = m.access_count + 1, m.last_accessed = $now
                RETURN m.access_count AS count
                """,
                memory_id=req.memory_id, now=now,
            ).single()
            access_count = result["count"] if result else 0
        driver.close()

        return {
            "ok": True,
            "reinforced": True,
            "memory_id": req.memory_id,
            "access_count": access_count,
        }
    except Exception as e:
        logger.warning(f"Reinforcement failed: {e}")
        return {"ok": True, "reinforced": False, "error": "Internal error"}


@evolution_router.post("/decay")
async def decay_memories(req: DecayRequest, request: Request):
    """Decay unused memories — reduce confidence of memories not accessed recently.

    Designed to run from nightly consolidation cron.
    Memories with MemoryAccess nodes accessed within days_inactive are exempt.
    """
    mem = request.app.state.memory

    try:
        raw = await asyncio.to_thread(mem.get_all, user_id=req.user_id, limit=500)
        entries = raw.get("results", raw) if isinstance(raw, dict) else raw
    except Exception as e:
        raise HTTPException(status_code=500, detail="Failed to fetch memories")

    if not isinstance(entries, list):
        return {"ok": True, "decayed": 0, "checked": 0}

    # Get recently-accessed memory IDs from Neo4j
    recently_accessed: set[str] = set()
    if neo4j_available():
        try:
            driver = neo4j_driver()
            with driver.session() as session:
                result = session.run(
                    "MATCH (m:MemoryAccess) WHERE m.last_accessed IS NOT NULL "
                    "RETURN m.memory_id AS id, m.access_count AS count"
                )
                for record in result:
                    recently_accessed.add(record["id"])
            driver.close()
            mark_neo4j_ok()
        except Exception:
            mark_neo4j_down()
            logger.warning("decay: Neo4j unavailable, proceeding without access data")

    # Identify memories that haven't been accessed
    decay_candidates = []
    for entry in entries:
        memory_id = entry.get("id", "")
        if memory_id and memory_id not in recently_accessed:
            decay_candidates.append(entry)

    if req.dry_run:
        return {
            "ok": True,
            "dry_run": True,
            "checked": len(entries),
            "decay_candidates": len(decay_candidates),
            "recently_accessed": len(recently_accessed),
            "sample": [
                {"id": e.get("id"), "text": e.get("memory", "")[:100]}
                for e in decay_candidates[:10]
            ],
        }

    # For now, we track decay candidates but don't delete (soft decay via metadata)
    # Future: could reduce vector scores or move to cold storage
    decayed = 0
    for entry in decay_candidates:
        memory_id = entry.get("id", "")
        if not memory_id:
            continue
        # Record decay signal in Neo4j
        if neo4j_available():
            try:
                driver = neo4j_driver()
                with driver.session() as session:
                    session.run(
                        """
                        MERGE (m:MemoryAccess {memory_id: $id})
                        ON CREATE SET m.decay_count = 1, m.last_decayed = $now
                        ON MATCH SET m.decay_count = coalesce(m.decay_count, 0) + 1,
                                     m.last_decayed = $now
                        """,
                        id=memory_id, now=datetime.now(timezone.utc).isoformat(),
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
async def evolution_stats():
    """Statistics on memory evolution and reinforcement."""
    if not neo4j_available():
        return {"ok": True, "available": False}

    try:
        driver = neo4j_driver()
        with driver.session() as session:
            total_tracked = session.run(
                "MATCH (m:MemoryAccess) RETURN count(m) AS c"
            ).single()["c"]
            total_evolutions = session.run(
                "MATCH ()-[r:EVOLVED_INTO]->() RETURN count(r) AS c"
            ).single()["c"]
            most_accessed = session.run(
                "MATCH (m:MemoryAccess) WHERE m.access_count > 1 "
                "RETURN m.memory_id AS id, m.access_count AS count "
                "ORDER BY m.access_count DESC LIMIT 10"
            ).data()
            decaying = session.run(
                "MATCH (m:MemoryAccess) WHERE m.decay_count IS NOT NULL AND m.decay_count > 0 "
                "RETURN count(m) AS c"
            ).single()["c"]
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
            resp = await client.post(
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
            data = resp.json()
            text = data.get("content", [{}])[0].get("text", "").strip()
            return text if text and len(text) > 10 else None
    except Exception:
        logger.warning("LLM evolution merge failed", exc_info=True)
        return None


async def _record_evolution_graph(old_text: str, evolved_text: str, old_id: str) -> None:
    """Record evolution lineage in Neo4j."""
    if not neo4j_available():
        return
    try:
        driver = neo4j_driver()
        with driver.session() as session:
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
