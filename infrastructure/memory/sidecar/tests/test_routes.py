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


# ---------------------------------------------------------------------------
# /dedup/batch tests
# ---------------------------------------------------------------------------


def test_dedup_batch_empty_input(client: TestClient) -> None:
    """Empty text list returns empty deduplicated list and 0 removed."""
    resp = client.post("/dedup/batch", json={"texts": []})
    assert resp.status_code == 200
    data = resp.json()
    assert data["deduplicated"] == []
    assert data["removed"] == 0


def test_dedup_batch_no_duplicates(client: TestClient) -> None:
    """Distinct texts (orthogonal vectors) are all kept."""
    # Two orthogonal vectors: similarity = 0.0
    vec_a = [1.0] + [0.0] * 127
    vec_b = [0.0, 1.0] + [0.0] * 126

    with patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed:
        mock_embed.return_value = [vec_a, vec_b]
        resp = client.post(
            "/dedup/batch",
            json={"texts": ["User prefers Python over JavaScript", "Baby #2 due October 2026"]},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["removed"] == 0
    assert len(data["deduplicated"]) == 2


def test_dedup_batch_removes_near_duplicates(client: TestClient) -> None:
    """When two texts have cosine similarity >= threshold, the second is dropped."""
    # Identical vectors → similarity = 1.0, which exceeds default threshold 0.90
    vec = [1.0] + [0.0] * 127

    with patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed:
        mock_embed.return_value = [vec, vec]
        resp = client.post(
            "/dedup/batch",
            json={
                "texts": [
                    "User prefers chrome-tanned leather for belts",
                    "User strongly prefers chrome-tanned leather for belts",
                ]
            },
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["removed"] == 1
    assert len(data["deduplicated"]) == 1
    # The first text (original order) is retained
    assert data["deduplicated"][0] == "User prefers chrome-tanned leather for belts"


def test_dedup_batch_respects_threshold(client: TestClient) -> None:
    """Threshold controls dedup aggressiveness: higher threshold → less dedup."""
    import math

    # vec_a and vec_b have similarity ~0.949
    vec_a = [1.0, 1.0] + [0.0] * 126
    vec_b = [1.0, 0.5] + [0.0] * 126
    mag_a = math.sqrt(1.0 + 1.0)
    mag_b = math.sqrt(1.0 + 0.25)
    sim = (1.0 * 1.0 + 1.0 * 0.5) / (mag_a * mag_b)

    with patch("aletheia_memory.routes._embed_texts", new_callable=AsyncMock) as mock_embed:
        # Default threshold 0.90 — similarity ~0.949 → duplicate removed
        mock_embed.return_value = [vec_a, vec_b]
        resp_default = client.post(
            "/dedup/batch",
            json={"texts": ["fact about leather belts", "fact about chrome leather belts"]},
        )
        assert resp_default.status_code == 200
        assert resp_default.json()["removed"] == 1, f"Expected 1 removed at default threshold (sim={sim:.3f})"

        # Threshold 0.99 — similarity ~0.949 < 0.99 → both kept
        mock_embed.return_value = [vec_a, vec_b]
        resp_strict = client.post(
            "/dedup/batch",
            json={
                "texts": ["fact about leather belts", "fact about chrome leather belts"],
                "threshold": 0.99,
            },
        )
        assert resp_strict.status_code == 200
        assert resp_strict.json()["removed"] == 0, f"Expected 0 removed at threshold=0.99 (sim={sim:.3f})"
