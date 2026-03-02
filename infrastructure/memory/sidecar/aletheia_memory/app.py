# Aletheia Memory Sidecar v2
# Three-tier LLM backend: Anthropic OAuth > Anthropic API key > Ollama > embedding-only

import logging
import os
from collections.abc import AsyncIterator, Awaitable, Callable
from contextlib import asynccontextmanager
from typing import Any

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from mem0 import Memory
from starlette.responses import Response

from neo4j import GraphDatabase
from qdrant_client import QdrantClient

from .config import LLM_BACKEND, NEO4J_PASSWORD, NEO4J_URL, NEO4J_USER, QDRANT_HOST, QDRANT_PORT, build_mem0_config
from .discovery import discovery_router
from .evolution import evolution_router
from .graph import set_shared_driver
from .graph_extraction import init_pipeline
from .llm_backend import refresh_oauth_token
from .routes import foresight_router, router, set_shared_qdrant
from .temporal import ensure_temporal_schema, temporal_router

log = logging.getLogger("aletheia.memory")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(name)s %(levelname)s %(message)s")


def _patch_anthropic_params() -> None:
    """Patch Mem0's Anthropic LLM for API compatibility.

    Fixes:
    1. Anthropic rejects temperature + top_p together
    2. Mem0 sends OpenAI-style tool schemas, Anthropic needs its own format
    3. Anthropic returns tool_use blocks, Mem0 expects OpenAI-style tool_calls dict
    """
    from mem0.llms import anthropic as anthropic_llm

    def _patched_generate(
        self: Any,
        messages: list[dict[str, Any]],
        response_format: Any = None,
        tools: list[dict[str, Any]] | None = None,
        tool_choice: str | dict[str, Any] = "auto",
        **kwargs: Any,
    ) -> dict[str, list[dict[str, Any]]] | Any:
        kwargs.pop("top_p", None)
        params: dict[str, Any] = self._get_supported_params(messages=messages, **kwargs)
        params.pop("top_p", None)

        filtered_messages: list[dict[str, Any]] = []
        system_message: str = ""
        for msg in messages:
            if msg["role"] == "system":
                system_message = str(msg["content"])
            else:
                filtered_messages.append(msg)

        params.update({
            "model": self.config.model,
            "messages": filtered_messages,
            "system": system_message,
        })

        if tools:
            anthropic_tools: list[dict[str, Any]] = []
            for tool in tools:
                if tool.get("type") == "function" and "function" in tool:
                    fn: dict[str, Any] = tool["function"]
                    anthropic_tools.append({
                        "name": fn["name"],
                        "description": fn.get("description", ""),
                        "input_schema": fn.get("parameters", {}),
                    })
                else:
                    anthropic_tools.append(tool)
            params["tools"] = anthropic_tools
            if isinstance(tool_choice, str):
                params["tool_choice"] = {"type": tool_choice}
            else:
                params["tool_choice"] = tool_choice

        response: Any = self.client.messages.create(**params)

        if tools:
            tool_calls: list[dict[str, Any]] = []
            for block in response.content:
                if block.type == "tool_use":
                    tool_calls.append({
                        "name": block.name,
                        "arguments": block.input,
                    })
            return {"tool_calls": tool_calls}

        return response.content[0].text

    anthropic_llm.AnthropicLLM.generate_response = _patched_generate


def _patch_openai_embedder_for_voyage() -> None:
    """Voyage API doesn't support the `dimensions` parameter."""
    from mem0.embeddings import openai as openai_emb

    def _patched_embed(self: Any, text: str, memory_action: Any = None) -> Any:
        text = text.replace("\n", " ")
        return (
            self.client.embeddings.create(input=[text], model=self.config.model)
            .data[0]
            .embedding
        )

    openai_emb.OpenAIEmbedding.embed = _patched_embed


def _inject_oauth_llm(mem: Memory, backend: dict[str, Any]) -> None:
    """For OAuth mode: replace Mem0's LLM instance with our OAuth-authenticated one."""
    if backend["provider"] == "anthropic-oauth" and backend["llm_instance"]:
        if hasattr(mem, 'llm'):
            mem.llm = backend["llm_instance"]
        log.debug("Refreshed LLM provider reference")  # LLM instance ref only, no credentials logged


memory: Memory | None = None
_active_backend: dict[str, Any] = LLM_BACKEND


def refresh_pipeline_on_token_rotate(app: FastAPI) -> None:
    """Reinitialize the SimpleKGPipeline after an OAuth token rotation.

    Call this whenever refresh_oauth_token() detects a new token so that
    the graph extraction LLM adapter uses the updated credentials.
    """
    global _active_backend
    updated: dict[str, Any] = refresh_oauth_token(_active_backend)
    if updated is not _active_backend:
        _active_backend = updated
    new_pipeline: object | None = init_pipeline(_active_backend)
    app.state.graph_pipeline = new_pipeline
    log.info("Graph pipeline reinitialized after OAuth token rotation")


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    global memory, _active_backend

    _active_backend = LLM_BACKEND
    tier: Any = _active_backend["tier"]
    provider: Any = _active_backend["provider"]
    model: Any = _active_backend["model"]

    log.info("Starting with Tier %s: %s%s", tier, provider, f" ({model})" if model else "")

    if provider in ("anthropic-oauth", "anthropic-apikey"):
        _patch_anthropic_params()

    config: dict[str, Any] = build_mem0_config(_active_backend)
    if config["embedder"]["provider"] == "openai":
        _patch_openai_embedder_for_voyage()

    if tier == 3:
        log.warning("Tier 3: embedding-only mode. Fact extraction disabled.")
        config["llm"] = {
            "provider": "anthropic",
            "config": {
                "model": "claude-haiku-4-5-20251001",
                "api_key": "tier3-no-llm",
                "temperature": 0.1,
                "max_tokens": 100,
            },
        }
        memory = Memory.from_config(config)
    else:
        memory = Memory.from_config(config)
        if provider == "anthropic-oauth":
            _inject_oauth_llm(memory, _active_backend)

    # Create shared database clients — reused across all requests (#341)
    neo4j_drv = GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD)) if NEO4J_PASSWORD else None
    set_shared_driver(neo4j_drv)
    qdrant_cl = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
    set_shared_qdrant(qdrant_cl)

    app.state.memory = memory
    app.state.backend = _active_backend
    app.state.graph_pipeline = init_pipeline(_active_backend)
    app.state.neo4j_driver = neo4j_drv
    app.state.qdrant_client = qdrant_cl
    await ensure_temporal_schema()

    log.info("Memory sidecar ready")
    yield

    # Cleanup
    if neo4j_drv:
        neo4j_drv.close()
    qdrant_cl.close()
    memory = None


app = FastAPI(title="Aletheia Memory", version="2.0.0", lifespan=lifespan)

AUTH_TOKEN = os.environ.get("ALETHEIA_MEMORY_TOKEN", "")


@app.middleware("http")
async def auth_middleware(
    request: Request,
    call_next: Callable[[Request], Awaitable[Response]],
) -> Response:
    if AUTH_TOKEN and request.url.path != "/health":
        auth = request.headers.get("authorization", "")
        if not auth.startswith("Bearer ") or auth[7:] != AUTH_TOKEN:
            return JSONResponse(status_code=401, content={"error": "Unauthorized"})
    return await call_next(request)


app.include_router(router)
app.include_router(foresight_router)
app.include_router(temporal_router)
app.include_router(evolution_router)
app.include_router(discovery_router)
