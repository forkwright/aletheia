# Aletheia Memory Sidecar v2
# Three-tier LLM backend: Anthropic OAuth > Anthropic API key > Ollama > embedding-only

import logging
import os
from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from mem0 import Memory

from .config import MEM0_CONFIG, LLM_BACKEND, build_mem0_config
from .llm_backend import refresh_oauth_token
from .routes import router, foresight_router
from .discovery import discovery_router
from .evolution import evolution_router
from .temporal import temporal_router, ensure_temporal_schema

log = logging.getLogger("aletheia.memory")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(name)s %(levelname)s %(message)s")


def _patch_anthropic_params():
    """Patch Mem0's Anthropic LLM for API compatibility.

    Fixes:
    1. Anthropic rejects temperature + top_p together
    2. Mem0 sends OpenAI-style tool schemas, Anthropic needs its own format
    3. Anthropic returns tool_use blocks, Mem0 expects OpenAI-style tool_calls dict
    """
    from mem0.llms import anthropic as anthropic_llm

    def _patched_generate(self, messages, response_format=None, tools=None, tool_choice="auto", **kwargs):
        kwargs.pop("top_p", None)
        params = self._get_supported_params(messages=messages, **kwargs)
        params.pop("top_p", None)

        filtered_messages = []
        system_message = ""
        for msg in messages:
            if msg["role"] == "system":
                system_message = msg["content"]
            else:
                filtered_messages.append(msg)

        params.update({
            "model": self.config.model,
            "messages": filtered_messages,
            "system": system_message,
        })

        if tools:
            anthropic_tools = []
            for tool in tools:
                if tool.get("type") == "function" and "function" in tool:
                    fn = tool["function"]
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

        response = self.client.messages.create(**params)

        if tools:
            tool_calls = []
            for block in response.content:
                if block.type == "tool_use":
                    tool_calls.append({
                        "name": block.name,
                        "arguments": block.input,
                    })
            return {"tool_calls": tool_calls}

        return response.content[0].text

    anthropic_llm.AnthropicLLM.generate_response = _patched_generate


def _patch_openai_embedder_for_voyage():
    """Voyage API doesn't support the `dimensions` parameter."""
    from mem0.embeddings import openai as openai_emb

    def _patched_embed(self, text, memory_action=None):
        text = text.replace("\n", " ")
        return (
            self.client.embeddings.create(input=[text], model=self.config.model)
            .data[0]
            .embedding
        )

    openai_emb.OpenAIEmbedding.embed = _patched_embed


def _inject_oauth_llm(mem: Memory, backend: dict):
    """For OAuth mode: replace Mem0's LLM instance with our OAuth-authenticated one."""
    if backend["provider"] == "anthropic-oauth" and backend["llm_instance"]:
        if hasattr(mem, 'llm'):
            mem.llm = backend["llm_instance"]
        if hasattr(mem, 'graph') and mem.graph and hasattr(mem.graph, 'llm'):
            mem.graph.llm = backend["llm_instance"]
        log.info("Injected OAuth LLM into Mem0 Memory instance")


memory: Memory | None = None
_active_backend: dict = LLM_BACKEND


@asynccontextmanager
async def lifespan(app: FastAPI):
    global memory, _active_backend

    _active_backend = LLM_BACKEND
    tier = _active_backend["tier"]
    provider = _active_backend["provider"]
    model = _active_backend["model"]

    log.info(f"Starting with Tier {tier}: {provider}" + (f" ({model})" if model else ""))

    # Apply patches based on backend
    if provider in ("anthropic-oauth", "anthropic-apikey"):
        _patch_anthropic_params()

    config = build_mem0_config(_active_backend)
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

    app.state.memory = memory
    app.state.backend = _active_backend
    await ensure_temporal_schema()

    log.info("Memory sidecar ready")
    yield

    memory = None


app = FastAPI(title="Aletheia Memory", version="2.0.0", lifespan=lifespan)

AUTH_TOKEN = os.environ.get("ALETHEIA_MEMORY_TOKEN", "")


@app.middleware("http")
async def auth_middleware(request: Request, call_next):
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
