# Tests for the temporal sidecar endpoints, focused on invalidate_text

from collections.abc import Iterator
from unittest.mock import MagicMock, patch

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from aletheia_memory.temporal import temporal_router


def _make_app() -> FastAPI:
    app = FastAPI()
    app.include_router(temporal_router)

    mock_mem = MagicMock()
    mock_mem.embedding_model = MagicMock()
    mock_mem.embedding_model.embed = MagicMock(return_value=[0.1] * 512)

    async def setup_state() -> None:
        app.state.memory = mock_mem

    app.on_event("startup")(setup_state)  # pyright: ignore[reportDeprecated]
    return app


@pytest.fixture()
def client() -> Iterator[TestClient]:
    app = _make_app()
    with TestClient(app) as c:
        yield c


# ---------------------------------------------------------------------------
# POST /temporal/facts/invalidate_text
# ---------------------------------------------------------------------------


def test_invalidate_text_returns_no_match_below_threshold(client: TestClient) -> None:
    """When Qdrant returns a low-similarity hit, endpoint returns invalidated=false."""
    low_score_point = MagicMock()
    low_score_point.score = 0.55
    low_score_point.payload = {"data": "some stored fact", "user_id": "default"}
    low_score_point.id = "point-1"

    mock_results = MagicMock()
    mock_results.points = [low_score_point]

    with patch("aletheia_memory.temporal.QdrantClient") as mock_qdrant_cls:
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results

        resp = client.post(
            "/temporal/facts/invalidate_text",
            json={"text": "user dislikes Python", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["invalidated"] is False
    assert data["reason"] == "no_match_above_threshold"


def test_invalidate_text_returns_no_match_when_empty_results(client: TestClient) -> None:
    """When Qdrant returns no hits, endpoint returns invalidated=false."""
    mock_results = MagicMock()
    mock_results.points = []

    with patch("aletheia_memory.temporal.QdrantClient") as mock_qdrant_cls:
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results

        resp = client.post(
            "/temporal/facts/invalidate_text",
            json={"text": "no match text", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["invalidated"] is False


def test_invalidate_text_marks_fact_invalid(client: TestClient) -> None:
    """High-similarity match triggers Qdrant payload update and Neo4j invalidation."""
    high_score_point = MagicMock()
    high_score_point.score = 0.92
    high_score_point.payload = {"data": "user loves Python", "user_id": "default"}
    high_score_point.id = "point-abc"

    mock_results = MagicMock()
    mock_results.points = [high_score_point]

    mock_neo4j_session = MagicMock()
    mock_neo4j_session.__enter__ = MagicMock(return_value=mock_neo4j_session)
    mock_neo4j_session.__exit__ = MagicMock(return_value=False)

    with (
        patch("aletheia_memory.temporal.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.temporal.neo4j_available", return_value=False),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results
        mock_qdrant.set_payload.return_value = None

        resp = client.post(
            "/temporal/facts/invalidate_text",
            json={
                "text": "user actually hates Python",
                "user_id": "default",
                "reason": "contradiction_detected",
            },
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["invalidated"] is True
    assert data["matched_text"] == "user loves Python"
    assert data["similarity"] == pytest.approx(0.92, abs=1e-4)

    # Qdrant payload update was called with invalidated=True
    mock_qdrant.set_payload.assert_called_once()
    call_kwargs = mock_qdrant.set_payload.call_args.kwargs
    assert call_kwargs["payload"]["invalidated"] is True
    assert call_kwargs["payload"]["invalidated_reason"] == "contradiction_detected"
    assert call_kwargs["points"] == ["point-abc"]


def test_invalidate_text_with_neo4j_updates_temporal_fact(client: TestClient) -> None:
    """With Neo4j available, temporal fact invalidation is attempted."""
    high_score_point = MagicMock()
    high_score_point.score = 0.95
    high_score_point.payload = {"data": "user prefers coffee", "user_id": "default"}
    high_score_point.id = "point-neo4j"

    mock_results = MagicMock()
    mock_results.points = [high_score_point]

    mock_session = MagicMock()
    mock_driver = MagicMock()
    mock_driver.close = MagicMock()

    with (
        patch("aletheia_memory.temporal.QdrantClient") as mock_qdrant_cls,
        patch("aletheia_memory.temporal.neo4j_available", return_value=True),
        patch("aletheia_memory.temporal.neo4j_driver", return_value=mock_driver),
        patch("aletheia_memory.temporal._open_session", return_value=mock_session),
        patch("aletheia_memory.temporal.mark_neo4j_ok"),
    ):
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results
        mock_session.__enter__ = MagicMock(return_value=mock_session)
        mock_session.__exit__ = MagicMock(return_value=False)

        resp = client.post(
            "/temporal/facts/invalidate_text",
            json={"text": "user switched to tea", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["invalidated"] is True
    # Neo4j session.run was called for fact invalidation
    assert mock_session.run.call_count >= 1


def test_invalidate_text_rejects_empty_text(client: TestClient) -> None:
    """Empty text should return 400."""
    resp = client.post(
        "/temporal/facts/invalidate_text",
        json={"text": "   ", "user_id": "default"},
    )
    assert resp.status_code == 400


def test_invalidate_text_qdrant_failure_raises_503(client: TestClient) -> None:
    """When Qdrant is unavailable, endpoint returns 503."""
    with patch("aletheia_memory.temporal.QdrantClient") as mock_qdrant_cls:
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.side_effect = ConnectionError("Qdrant down")

        resp = client.post(
            "/temporal/facts/invalidate_text",
            json={"text": "some contradiction", "user_id": "default"},
        )

    assert resp.status_code == 503
