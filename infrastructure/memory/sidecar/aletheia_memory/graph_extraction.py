# SimpleKGPipeline wrapper for schema-enforced graph extraction

from __future__ import annotations

import logging
from typing import Optional

from .graph import neo4j_available, neo4j_driver
from .vocab import CONTROLLED_VOCAB

log = logging.getLogger("aletheia_memory.graph_extraction")

_SCHEMA = {
    "node_types": ["Person", "Organization", "Place", "Concept", "Technology", "Vehicle", "Entity"],
    "relationship_types": sorted(CONTROLLED_VOCAB),
    "additional_node_types": True,
    "additional_relationship_types": False,  # Hard enforcement: only vocab types written to Neo4j
}

_pipeline = None


def create_graphrag_llm(backend: dict) -> Optional[object]:
    """Build a neo4j-graphrag AnthropicLLM from the active backend.

    - API-key backend: instantiate AnthropicLLM with the key directly.
    - OAuth backend: instantiate with placeholder key, then replace the internal
      client with an OAuth-authenticated anthropic.Anthropic instance.
    - Ollama / none backend: return None (graph extraction unavailable).
    """
    provider = backend.get("provider", "none")

    if provider == "anthropic-apikey":
        api_key = backend.get("config", {}).get("config", {}).get("api_key") or ""
        if not api_key:
            log.warning("create_graphrag_llm: api_key backend but no key found")
            return None
        try:
            from neo4j_graphrag.llm import AnthropicLLM
            model = backend.get("model", "claude-haiku-4-5-20251001")
            return AnthropicLLM(model_name=model, api_key=api_key)
        except Exception as exc:
            log.warning("create_graphrag_llm: AnthropicLLM init failed: %s", exc)
            return None

    if provider == "anthropic-oauth":
        token = backend.get("oauth_token")
        if not token:
            log.warning("create_graphrag_llm: OAuth backend but no token found")
            return None
        try:
            import anthropic as anthropic_sdk
            from neo4j_graphrag.llm import AnthropicLLM
            model = backend.get("model", "claude-haiku-4-5-20251001")
            llm = AnthropicLLM(model_name=model, api_key="oauth-placeholder")
            # Monkey-patch the internal client with an OAuth-authenticated one
            llm.anthropic_client = anthropic_sdk.Anthropic(
                auth_token=token,
                default_headers={"anthropic-beta": "oauth-2025-04-20"},
            )
            return llm
        except Exception as exc:
            log.warning("create_graphrag_llm: OAuth AnthropicLLM init failed: %s", exc)
            return None

    # Tier 2 (ollama) and Tier 3 (none): no graphrag LLM available
    return None


def init_pipeline(backend: dict) -> Optional[object]:
    """Build and return a SimpleKGPipeline instance, or None if unavailable.

    None is returned when:
    - Neo4j is not reachable
    - No compatible LLM backend is available (Ollama, none)
    """
    global _pipeline

    if not neo4j_available():
        log.info("init_pipeline: Neo4j unavailable, graph extraction disabled")
        return None

    llm = create_graphrag_llm(backend)
    if llm is None:
        log.info("init_pipeline: no compatible LLM for graph extraction")
        return None

    try:
        from neo4j_graphrag.experimental.pipeline.kg_builder import SimpleKGPipeline
        driver = neo4j_driver()
        pipeline = SimpleKGPipeline(
            llm=llm,
            driver=driver,
            schema=_SCHEMA,
            on_error="IGNORE",
            perform_entity_resolution=True,
        )
        _pipeline = pipeline
        log.info("init_pipeline: SimpleKGPipeline initialized with %d relationship types", len(_SCHEMA["relationship_types"]))
        return pipeline
    except Exception as exc:
        log.warning("init_pipeline: SimpleKGPipeline init failed: %s", exc)
        return None


async def extract_graph(text: str, backend: dict | None = None) -> dict:
    """Extract graph relationships from text using SimpleKGPipeline.

    Fire-and-forget safe: failures return {"ok": False} and do not propagate.
    The pipeline enforces schema at write time — only CONTROLLED_VOCAB relationship
    types are written to Neo4j (additional_relationship_types: False).
    """
    global _pipeline

    if _pipeline is None:
        if backend is None:
            return {"ok": False, "reason": "no_pipeline"}
        _pipeline = init_pipeline(backend)

    if _pipeline is None:
        return {"ok": False, "reason": "no_pipeline"}

    try:
        await _pipeline.run_async(text=text)
        return {"ok": True}
    except Exception as exc:
        log.warning("extract_graph failed (non-fatal): %s", exc)
        return {"ok": False, "reason": str(exc)}


async def extract_graph_batch(texts: list[str], backend: dict | None = None) -> dict:
    """Extract graph relationships for a batch of texts.

    Joins texts into a single string so the pipeline's text splitter handles
    chunking, avoiding N individual LLM calls for large batches.
    """
    if not texts:
        return {"ok": True, "reason": "empty_batch"}
    combined = "\n\n".join(texts)
    return await extract_graph(combined, backend=backend)
