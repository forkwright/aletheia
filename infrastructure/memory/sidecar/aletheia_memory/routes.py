# API routes for Aletheia memory sidecar

from __future__ import annotations

import asyncio
import json
import logging
from pathlib import Path
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from .config import NEO4J_PASSWORD, NEO4J_URL, NEO4J_USER, OLLAMA_BASE_URL, QDRANT_HOST, QDRANT_PORT

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


@router.post("/add")
async def add_memory(req: AddRequest, request: Request):
    mem = request.app.state.memory
    kwargs: dict[str, Any] = {"user_id": req.user_id}
    if req.agent_id:
        kwargs["agent_id"] = req.agent_id
    if req.metadata:
        kwargs["metadata"] = req.metadata

    try:
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
        results = await asyncio.to_thread(mem.search, req.query, **kwargs)
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
        results = await asyncio.to_thread(mem.get_all, **kwargs)
        entries = results.get("results", results) if isinstance(results, dict) else results
        if isinstance(entries, list):
            entries = entries[:limit]
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
            r = await client.get(OLLAMA_BASE_URL.rstrip("/") + "/api/tags")
            checks["ollama"] = "ok" if r.status_code == 200 else f"status {r.status_code}"
        except Exception as e:
            checks["ollama"] = f"error: {e}"

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


@router.post("/import_file")
async def import_facts_file(request: Request, file_path: str, user_id: str = "ck"):
    """Import facts from a JSONL file on the server filesystem."""
    mem = request.app.state.memory
    path = Path(file_path)
    if not path.exists():
        raise HTTPException(status_code=404, detail=f"File not found: {file_path}")

    imported = 0
    errors = []
    with open(path) as f:
        for i, line in enumerate(f):
            line = line.strip()
            if not line:
                continue
            try:
                fact = json.loads(line)
            except json.JSONDecodeError:
                errors.append({"index": i, "error": "invalid JSON"})
                continue

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

            try:
                await asyncio.to_thread(mem.add, text, user_id=user_id, metadata=metadata)
                imported += 1
            except Exception as e:
                errors.append({"index": i, "error": str(e)})
                if len(errors) > 20:
                    break

    return {"ok": True, "imported": imported, "total_lines": i + 1, "errors": errors}
