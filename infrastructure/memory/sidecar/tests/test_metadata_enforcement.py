# Tests for metadata enforcement on /add_direct and /add_batch endpoints
#
# Validates that requests missing agent_id or session_id are rejected with 400,
# and that requests with all required fields proceed past validation.
# Qdrant and Mem0 calls are mocked so no live services are required.

from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from aletheia_memory.routes import router


def _make_app() -> FastAPI:
    """Minimal FastAPI app with the memory router and a mock memory state."""
    app = FastAPI()
    app.include_router(router)

    mock_mem = MagicMock()
    mock_mem.embedding_model = MagicMock()

    @app.on_event("startup")
    async def setup_state():
        app.state.memory = mock_mem
        app.state.backend = {"tier": 1, "provider": "test"}

    return app


@pytest.fixture()
def client():
    app = _make_app()
    with TestClient(app) as c:
        yield c


# ---------------------------------------------------------------------------
# /add_direct validation
# ---------------------------------------------------------------------------


def test_add_direct_valid_metadata_reaches_processing(client):
    """Valid request with all required fields should NOT return 400."""
    # Mock Qdrant scroll (content hash dedup check) and embed to prevent real calls
    with (
        patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed,
        patch("aletheia_memory.routes._semantic_dedup_check", new_callable=AsyncMock) as mock_dedup,
        patch("aletheia_memory.routes._check_contradictions", new_callable=AsyncMock) as mock_contra,
        patch("aletheia_memory.routes.get_canonical_entities", return_value=[]),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        # Simulate no existing hash match (not a duplicate)
        mock_qdrant.scroll.return_value = ([], None)
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

    assert resp.status_code not in (400, 422), f"Unexpected status {resp.status_code}: {resp.text}"


def test_add_direct_missing_agent_id_returns_400(client):
    """/add_direct without agent_id should return 400."""
    resp = client.post(
        "/add_direct",
        json={
            "text": "Some fact",
            "session_id": "ses_abc123",
            "source": "direct",
        },
    )
    assert resp.status_code == 400
    assert "agent_id" in resp.json()["detail"]


def test_add_direct_missing_session_id_returns_400(client):
    """/add_direct without session_id should return 400."""
    resp = client.post(
        "/add_direct",
        json={
            "text": "Some fact",
            "agent_id": "syn",
            "source": "direct",
        },
    )
    assert resp.status_code == 400
    assert "session_id" in resp.json()["detail"]


def test_add_direct_missing_both_returns_400(client):
    """/add_direct without agent_id and session_id should return 400 listing both."""
    resp = client.post(
        "/add_direct",
        json={"text": "Some fact", "source": "direct"},
    )
    assert resp.status_code == 400
    detail = resp.json()["detail"]
    assert "agent_id" in detail
    assert "session_id" in detail


# ---------------------------------------------------------------------------
# /add_batch validation
# ---------------------------------------------------------------------------


def test_add_batch_valid_metadata_reaches_processing(client):
    """Valid batch request with all required fields should NOT return 400."""
    with (
        patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed,
        patch("aletheia_memory.routes._semantic_dedup_check", new_callable=AsyncMock) as mock_dedup,
        patch("aletheia_memory.routes._check_contradictions", new_callable=AsyncMock) as mock_contra,
        patch("aletheia_memory.routes.get_canonical_entities", return_value=[]),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.scroll.return_value = ([], None)
        mock_embed.return_value = [[0.1] * 512]
        mock_dedup.return_value = False
        mock_contra.return_value = []
        mock_qdrant.upsert.return_value = MagicMock()

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

    assert resp.status_code not in (400, 422), f"Unexpected status {resp.status_code}: {resp.text}"


def test_add_batch_missing_agent_id_returns_400(client):
    """/add_batch without agent_id should return 400."""
    resp = client.post(
        "/add_batch",
        json={
            "texts": ["Some fact"],
            "session_id": "ses_abc123",
            "source": "distillation",
        },
    )
    assert resp.status_code == 400
    assert "agent_id" in resp.json()["detail"]


def test_add_batch_missing_session_id_returns_400(client):
    """/add_batch without session_id should return 400."""
    resp = client.post(
        "/add_batch",
        json={
            "texts": ["Some fact"],
            "agent_id": "syn",
            "source": "distillation",
        },
    )
    assert resp.status_code == 400
    assert "session_id" in resp.json()["detail"]


def test_add_batch_empty_agent_id_returns_400(client):
    """/add_batch with empty string agent_id should return 400 (empty string is falsy)."""
    resp = client.post(
        "/add_batch",
        json={
            "texts": ["Some fact"],
            "agent_id": "",
            "session_id": "ses_abc123",
            "source": "distillation",
        },
    )
    assert resp.status_code == 400
    assert "agent_id" in resp.json()["detail"]
