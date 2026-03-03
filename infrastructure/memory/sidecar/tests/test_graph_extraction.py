# Tests for SimpleKGPipeline config, schema enforcement, LLM adapter, and extraction logic

import asyncio
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

from aletheia_memory import graph_extraction as ge_module
from aletheia_memory.graph_extraction import (
    create_graphrag_llm,
    extract_graph,
    extract_graph_batch,
    init_pipeline,
)
from aletheia_memory.vocab import CONTROLLED_VOCAB

# Access the private _SCHEMA via getattr to avoid reportPrivateUsage
_SCHEMA_ATTR = "_SCHEMA"
_SCHEMA: dict[str, Any] = getattr(ge_module, _SCHEMA_ATTR)

# ---------------------------------------------------------------------------
# Schema correctness
# ---------------------------------------------------------------------------


def test_schema_includes_vocab() -> None:
    """Schema relationship_types matches sorted(CONTROLLED_VOCAB)."""
    assert _SCHEMA["relationship_types"] == sorted(CONTROLLED_VOCAB)


def test_schema_excludes_relates_to() -> None:
    """RELATES_TO must not appear in the schema relationship_types."""
    assert "RELATES_TO" not in _SCHEMA["relationship_types"]


def test_schema_additional_relationship_types_false() -> None:
    """additional_relationship_types must be False for hard enforcement."""
    assert _SCHEMA["additional_relationship_types"] is False


def test_schema_additional_node_types_true() -> None:
    """additional_node_types is True — new entity types are allowed."""
    assert _SCHEMA["additional_node_types"] is True


def test_schema_node_types_present() -> None:
    """Schema includes expected node type categories."""
    required = {"Person", "Organization", "Place", "Concept", "Technology"}
    assert required.issubset(set(_SCHEMA["node_types"]))


# ---------------------------------------------------------------------------
# create_graphrag_llm — API-key backend
# ---------------------------------------------------------------------------


def test_create_graphrag_llm_apikey() -> None:
    """API-key backend creates AnthropicLLM with the key directly."""
    backend: dict[str, Any] = {
        "tier": 1,
        "provider": "anthropic-apikey",
        "model": "claude-haiku-4-5-20251001",
        "config": {"config": {"api_key": "test-key-abc"}},
        "llm_instance": None,
        "oauth_token": None,
    }

    # Direct patch on the neo4j_graphrag.llm module
    mock_llm_class = MagicMock(return_value=MagicMock())
    with patch.dict("sys.modules", {"neo4j_graphrag.llm": MagicMock(AnthropicLLM=mock_llm_class)}):
        result = create_graphrag_llm(backend)

    assert result is not None
    mock_llm_class.assert_called_once_with(
        model_name="claude-haiku-4-5-20251001", api_key="test-key-abc"
    )


# ---------------------------------------------------------------------------
# create_graphrag_llm — OAuth backend
# ---------------------------------------------------------------------------


def test_create_graphrag_llm_oauth() -> None:
    """OAuth backend creates AnthropicLLM with placeholder key, then patches client."""
    backend: dict[str, Any] = {
        "tier": 1,
        "provider": "anthropic-oauth",
        "model": "claude-haiku-4-5-20251001",
        "config": None,
        "llm_instance": None,
        "oauth_token": "oauth-test-token-xyz",
    }

    mock_llm_instance = MagicMock()
    mock_llm_class = MagicMock(return_value=mock_llm_instance)
    mock_anthropic_client = MagicMock()
    mock_anthropic_sdk = MagicMock()
    mock_anthropic_sdk.Anthropic.return_value = mock_anthropic_client

    with patch.dict("sys.modules", {
        "neo4j_graphrag.llm": MagicMock(AnthropicLLM=mock_llm_class),
        "anthropic": mock_anthropic_sdk,
    }):
        result = create_graphrag_llm(backend)

    assert result is not None
    mock_llm_class.assert_called_once_with(
        model_name="claude-haiku-4-5-20251001", api_key="oauth-placeholder"
    )
    # Client should be replaced with OAuth-authenticated instance
    mock_anthropic_sdk.Anthropic.assert_called_once()
    call_kwargs = mock_anthropic_sdk.Anthropic.call_args
    assert call_kwargs.kwargs.get("auth_token") == "oauth-test-token-xyz"
    assert mock_llm_instance.anthropic_client == mock_anthropic_client


# ---------------------------------------------------------------------------
# create_graphrag_llm — Tier 3 / none backend
# ---------------------------------------------------------------------------


def test_create_graphrag_llm_none_for_tier3() -> None:
    """Tier 3 (no LLM) backend returns None — graph extraction unavailable."""
    backend: dict[str, Any] = {
        "tier": 3,
        "provider": "none",
        "model": None,
        "config": None,
        "llm_instance": None,
        "oauth_token": None,
    }
    result = create_graphrag_llm(backend)
    assert result is None


def test_create_graphrag_llm_none_for_ollama() -> None:
    """Ollama backend returns None — graphrag requires Anthropic."""
    backend: dict[str, Any] = {
        "tier": 2,
        "provider": "ollama",
        "model": "qwen2.5:7b",
        "config": {"config": {}},
        "llm_instance": None,
        "oauth_token": None,
    }
    result = create_graphrag_llm(backend)
    assert result is None


# ---------------------------------------------------------------------------
# init_pipeline — Neo4j unavailable
# ---------------------------------------------------------------------------


def test_init_pipeline_returns_none_without_neo4j() -> None:
    """init_pipeline returns None when neo4j_available() is False."""
    backend: dict[str, Any] = {
        "tier": 1,
        "provider": "anthropic-apikey",
        "model": "claude-haiku-4-5-20251001",
        "config": {"config": {"api_key": "test-key"}},
        "llm_instance": None,
        "oauth_token": None,
    }
    with patch("aletheia_memory.graph_extraction.neo4j_available", return_value=False):
        result = init_pipeline(backend)
    assert result is None


# ---------------------------------------------------------------------------
# extract_graph — success and failure paths
# ---------------------------------------------------------------------------


def test_extract_graph_returns_ok() -> None:
    """extract_graph returns {"ok": True} when pipeline.run_async succeeds."""
    async def _run() -> None:
        mock_pipeline = MagicMock()
        mock_pipeline.run_async = AsyncMock(return_value=None)

        with patch.object(ge_module, "_pipeline", mock_pipeline):
            result: dict[str, Any] = await extract_graph("Alice uses Python for data analysis.")

        assert result == {"ok": True}
        mock_pipeline.run_async.assert_called_once_with(text="Alice uses Python for data analysis.")

    asyncio.run(_run())


def test_extract_graph_handles_failure() -> None:
    """extract_graph returns {"ok": False, "reason": ...} when pipeline raises."""
    async def _run() -> None:
        mock_pipeline = MagicMock()
        mock_pipeline.run_async = AsyncMock(side_effect=RuntimeError("Neo4j connection refused"))

        with patch.object(ge_module, "_pipeline", mock_pipeline):
            result: dict[str, Any] = await extract_graph("Some text about a person.")

        assert result["ok"] is False
        assert "Neo4j connection refused" in result["reason"]

    asyncio.run(_run())


def test_extract_graph_no_pipeline_no_backend() -> None:
    """extract_graph returns no_pipeline when pipeline is None and no backend given."""
    async def _run() -> None:
        with patch.object(ge_module, "_pipeline", None):
            result: dict[str, Any] = await extract_graph("Some text.", backend=None)

        assert result == {"ok": False, "reason": "no_pipeline"}

    asyncio.run(_run())


def test_extract_graph_initializes_pipeline_from_backend() -> None:
    """extract_graph initializes pipeline from backend when _pipeline is None."""
    async def _run() -> None:
        mock_pipeline = MagicMock()
        mock_pipeline.run_async = AsyncMock(return_value=None)

        backend: dict[str, Any] = {
            "tier": 1,
            "provider": "anthropic-apikey",
            "model": "claude-haiku-4-5-20251001",
            "config": {"config": {"api_key": "key"}},
            "llm_instance": None,
            "oauth_token": None,
        }

        with (
            patch.object(ge_module, "_pipeline", None),
            patch("aletheia_memory.graph_extraction.init_pipeline", return_value=mock_pipeline) as mock_init,
        ):
            result: dict[str, Any] = await extract_graph("Test text.", backend=backend)

        mock_init.assert_called_once_with(backend)
        assert result == {"ok": True}

    asyncio.run(_run())


# ---------------------------------------------------------------------------
# extract_graph_batch
# ---------------------------------------------------------------------------


def test_extract_graph_batch_joins_texts() -> None:
    """extract_graph_batch combines texts with double newlines before calling extract_graph."""
    async def _run() -> None:
        texts = ["Fact one.", "Fact two.", "Fact three."]
        expected_combined = "Fact one.\n\nFact two.\n\nFact three."

        with patch("aletheia_memory.graph_extraction.extract_graph", new_callable=AsyncMock) as mock_extract:
            mock_extract.return_value = {"ok": True}
            result: dict[str, Any] = await extract_graph_batch(texts)

        mock_extract.assert_called_once_with(expected_combined, backend=None)
        assert result == {"ok": True}

    asyncio.run(_run())


def test_extract_graph_batch_empty() -> None:
    """extract_graph_batch returns ok for empty text list."""
    async def _run() -> None:
        result: dict[str, Any] = await extract_graph_batch([])
        assert result == {"ok": True, "reason": "empty_batch"}

    asyncio.run(_run())
