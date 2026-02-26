# Tests verifying that add_batch and add_direct structurally bypass mem.add()
#
# EXTR-06: The infer=False requirement is satisfied architecturally — these
# routes write directly to Qdrant and never invoke Mem0's LLM extraction.
# These tests confirm mem.add is not called on successful requests.

from collections.abc import Iterator
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from aletheia_memory.routes import router


def _make_app() -> FastAPI:
    app = FastAPI()
    app.include_router(router)

    mock_mem = MagicMock()
    mock_mem.embedding_model = MagicMock()

    async def setup_state() -> None:
        app.state.memory = mock_mem
        app.state.backend = {"tier": 1, "provider": "test"}

    app.on_event("startup")(setup_state)  # pyright: ignore[reportDeprecated]
    return app


@pytest.fixture()
def client() -> Iterator[TestClient]:
    app = _make_app()
    with TestClient(app) as c:
        yield c


# ---------------------------------------------------------------------------
# EXTR-06: mem.add() bypass verification
# ---------------------------------------------------------------------------


def test_add_batch_never_calls_mem_add(client: TestClient) -> None:
    """add_batch writes directly to Qdrant — mem.add() must never be invoked."""
    with (
        patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed,
        patch("aletheia_memory.routes._semantic_dedup_check", new_callable=AsyncMock) as mock_dedup,
        patch("aletheia_memory.routes._check_contradictions", new_callable=AsyncMock) as mock_contra,
        patch("aletheia_memory.routes.get_canonical_entities", return_value=[]),
        patch("aletheia_memory.routes.extract_graph_batch", new_callable=AsyncMock),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.scroll.return_value = ([], None)
        mock_qdrant.upsert.return_value = MagicMock()
        mock_embed.return_value = [[0.1] * 512]
        mock_dedup.return_value = False
        mock_contra.return_value = []

        resp = client.post(
            "/add_batch",
            json={
                "texts": ["Cody is building Aletheia"],
                "agent_id": "syn",
                "session_id": "ses_abc123",
                "source": "distillation",
                "user_id": "default",
            },
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True

    # Retrieve the mock mem from app state and verify mem.add was never called
    mem_mock: MagicMock = client.app.state.memory  # type: ignore[attr-defined]
    mem_mock.add.assert_not_called()


def test_add_direct_never_calls_mem_add(client: TestClient) -> None:
    """add_direct writes directly to Qdrant — mem.add() must never be invoked."""
    with (
        patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed,
        patch("aletheia_memory.routes._semantic_dedup_check", new_callable=AsyncMock) as mock_dedup,
        patch("aletheia_memory.routes._check_contradictions", new_callable=AsyncMock) as mock_contra,
        patch("aletheia_memory.routes.get_canonical_entities", return_value=[]),
        patch("aletheia_memory.routes.extract_graph", new_callable=AsyncMock),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        # First scroll call = content hash check (no existing)
        mock_qdrant.scroll.return_value = ([], None)
        mock_qdrant.upsert.return_value = MagicMock()
        mock_embed.return_value = [[0.1] * 512]
        mock_dedup.return_value = False
        mock_contra.return_value = []

        resp = client.post(
            "/add_direct",
            json={
                "text": "Cody prefers Python over JavaScript",
                "agent_id": "syn",
                "session_id": "ses_abc123",
                "source": "direct",
                "user_id": "default",
                "confidence": 0.8,
            },
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True

    # Verify mem.add was never called
    mem_mock: MagicMock = client.app.state.memory  # type: ignore[attr-defined]
    mem_mock.add.assert_not_called()
