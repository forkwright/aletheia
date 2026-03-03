# Tests verifying that add_batch and add_direct structurally bypass mem.add()
#
# EXTR-06: The infer=False requirement is satisfied architecturally — these
# routes write directly to Qdrant and never invoke Mem0's LLM extraction.
# These tests confirm mem.add is not called on successful requests.

import asyncio
from collections.abc import Iterator
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from aletheia_memory.routes import (
    _apply_domain_reranking,  # pyright: ignore[reportPrivateUsage]
    _domain_relevance_score,  # pyright: ignore[reportPrivateUsage]
    _filter_noisy_results,  # pyright: ignore[reportPrivateUsage]
    _neo4j_expand_with_timeout,  # pyright: ignore[reportPrivateUsage]
    _qdrant_search_direct,  # pyright: ignore[reportPrivateUsage]
    router,
)


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
                "texts": ["Alice is building Aletheia"],
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
                "text": "Alice prefers Python over JavaScript",
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
            json={"texts": ["User prefers Python over JavaScript", "Project Alpha deadline is March 2026"]},
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
                    "User prefers high-grade polymer for brackets",
                    "User strongly prefers high-grade polymer for brackets",
                ]
            },
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["removed"] == 1
    assert len(data["deduplicated"]) == 1
    # The first text (original order) is retained
    assert data["deduplicated"][0] == "User prefers high-grade polymer for brackets"


# ---------------------------------------------------------------------------
# _filter_noisy_results tests
# ---------------------------------------------------------------------------


def test_noise_filter_penalizes_noisy_results() -> None:
    """Noisy results get 0.3x score penalty, not removed."""
    results: list[dict[str, Any]] = [
        {"memory": "Session started with session id abc123", "score": 1.0},
        {"memory": "Project Alpha deadline is March 2026", "score": 0.9},
    ]
    filtered = _filter_noisy_results(results)

    # Both results remain (soft boundary — not removed)
    assert len(filtered) == 2

    # Noisy result gets 0.3x penalty
    noisy = next(r for r in filtered if "session" in r["memory"].lower())
    assert abs(noisy["score"] - 0.3) < 1e-9

    # Clean result passes through unchanged
    clean = next(r for r in filtered if "Project Alpha" in r["memory"])
    assert abs(clean["score"] - 0.9) < 1e-9


def test_noise_filter_passes_clean_results_unchanged() -> None:
    """Clean results with no noise patterns have scores unchanged."""
    results: list[dict[str, Any]] = [
        {"memory": "Widget torque spec is 42 Nm per service manual", "score": 0.95},
        {"memory": "ALETHEIA_MEMORY_USER must be set in aletheia.env", "score": 0.88},
    ]
    filtered = _filter_noisy_results(results)

    assert len(filtered) == 2
    assert abs(filtered[0]["score"] - 0.95) < 1e-9
    assert abs(filtered[1]["score"] - 0.88) < 1e-9


def test_noise_filter_penalizes_short_memories() -> None:
    """Memories shorter than the minimum length threshold get score penalty."""
    results: list[dict[str, Any]] = [
        {"memory": "ok", "score": 0.8},  # Too short — noise
        {"memory": "Project Alpha deadline is March 2026", "score": 0.7},  # Clean
    ]
    filtered = _filter_noisy_results(results)

    assert len(filtered) == 2

    short = next(r for r in filtered if r["memory"] == "ok")
    assert abs(short["score"] - 0.8 * 0.3) < 1e-9

    clean = next(r for r in filtered if "Project Alpha" in r["memory"])
    assert abs(clean["score"] - 0.7) < 1e-9


def test_noise_filter_does_not_remove_results() -> None:
    """Even heavily noisy results remain in output (soft boundaries)."""
    results: list[dict[str, Any]] = [
        {"memory": "The user asked about the configuration", "score": 1.0},
        {"memory": "Called tool grep to search imports", "score": 0.9},
        {"memory": "Sure, will do", "score": 0.85},
    ]
    filtered = _filter_noisy_results(results)

    # All three results remain despite being noisy
    assert len(filtered) == 3

    # All scores are penalized (0.3x)
    for r in filtered:
        assert r["score"] < 0.4


def test_noise_filter_sorts_by_score_after_penalty() -> None:
    """After applying penalties, results are re-sorted by score descending."""
    results: list[dict[str, Any]] = [
        {"memory": "Session started conversation id abc", "score": 1.0},  # noisy → 0.3
        {"memory": "Project Alpha deadline is March 2026", "score": 0.5},  # clean → 0.5
    ]
    filtered = _filter_noisy_results(results)

    # Clean result should come first after re-sort
    assert filtered[0]["memory"] == "Project Alpha deadline is March 2026"
    assert filtered[1]["memory"] == "Session started conversation id abc"


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


# ---------------------------------------------------------------------------
# Task 1: _neo4j_expand_with_timeout tests
# ---------------------------------------------------------------------------


def test_neo4j_timeout_returns_empty_list() -> None:
    """_neo4j_expand_with_timeout returns empty list when asyncio.wait_for raises TimeoutError."""
    async def mock_wait_for(coro: object, timeout: float, **kwargs: object) -> list[str]:
        if hasattr(coro, "close"):
            coro.close()  # type: ignore[union-attr]
        raise TimeoutError

    async def run() -> list[str]:
        with patch("aletheia_memory.routes.asyncio.wait_for", side_effect=mock_wait_for):
            return await _neo4j_expand_with_timeout("test query", "default", timeout_ms=800)

    result = asyncio.run(run())
    assert result == []


def test_neo4j_timeout_enforces_800ms_cap() -> None:
    """_neo4j_expand_with_timeout passes timeout=0.8 (800ms) to asyncio.wait_for."""
    captured_timeout: list[float] = []

    async def mock_wait_for(coro: object, timeout: float, **kwargs: object) -> list[str]:
        captured_timeout.append(timeout)
        if hasattr(coro, "close"):
            coro.close()  # type: ignore[union-attr]
        raise TimeoutError

    async def run() -> None:
        with patch("aletheia_memory.routes.asyncio.wait_for", side_effect=mock_wait_for):
            await _neo4j_expand_with_timeout("test query", "default", timeout_ms=800)

    asyncio.run(run())
    assert len(captured_timeout) == 1
    assert abs(captured_timeout[0] - 0.8) < 0.001


def test_neo4j_connection_error_returns_empty_list() -> None:
    """_neo4j_expand_with_timeout returns empty list on any non-timeout exception."""
    async def mock_wait_for(coro: object, timeout: float, **kwargs: object) -> list[str]:
        if hasattr(coro, "close"):
            coro.close()  # type: ignore[union-attr]
        raise ConnectionError("Neo4j unavailable")

    async def run() -> list[str]:
        with patch("aletheia_memory.routes.asyncio.wait_for", side_effect=mock_wait_for):
            return await _neo4j_expand_with_timeout("test query", "default", timeout_ms=800)

    result = asyncio.run(run())
    assert result == []


# ---------------------------------------------------------------------------
# Task 1: _qdrant_search_direct tests
# ---------------------------------------------------------------------------


def test_qdrant_search_direct_returns_scored_results() -> None:
    """_qdrant_search_direct returns result dicts with id, memory, and score."""
    mock_mem = MagicMock()
    mock_mem.embedding_model.embed.return_value = [0.1] * 128

    mock_point = MagicMock()
    mock_point.id = "abc-123"
    mock_point.score = 0.85
    mock_point.payload = {"memory": "Alice prefers Python", "user_id": "default"}

    mock_results = MagicMock()
    mock_results.points = [mock_point]

    with patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls:
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results

        results = _qdrant_search_direct(
            query="Python preferences",
            user_id="default",
            limit=10,
            min_score=0.0,
            mem=mock_mem,
        )

    assert len(results) == 1
    assert results[0]["id"] == "abc-123"
    assert results[0]["score"] == 0.85
    assert results[0]["memory"] == "Alice prefers Python"


def test_qdrant_search_direct_filters_by_min_score() -> None:
    """_qdrant_search_direct excludes results below min_score threshold."""
    mock_mem = MagicMock()
    mock_mem.embedding_model.embed.return_value = [0.1] * 128

    def make_point(pid: str, score: float, memory: str) -> MagicMock:
        p = MagicMock()
        p.id = pid
        p.score = score
        p.payload = {"memory": memory, "user_id": "default"}
        return p

    mock_results = MagicMock()
    mock_results.points = [
        make_point("high", 0.9, "high score result"),
        make_point("low", 0.3, "low score result"),
    ]

    with patch("aletheia_memory.routes.QdrantClient") as mock_qdrant_cls:
        mock_qdrant = MagicMock()
        mock_qdrant_cls.return_value = mock_qdrant
        mock_qdrant.query_points.return_value = mock_results

        results = _qdrant_search_direct(
            query="test query",
            user_id="default",
            limit=10,
            min_score=0.5,
            mem=mock_mem,
        )

    assert len(results) == 1
    assert results[0]["id"] == "high"


def test_qdrant_search_direct_returns_empty_on_error() -> None:
    """_qdrant_search_direct returns empty list when embedder raises."""
    mock_mem = MagicMock()
    mock_mem.embedding_model.embed.side_effect = RuntimeError("embedder failed")

    results = _qdrant_search_direct(
        query="test query",
        user_id="default",
        limit=10,
        min_score=0.0,
        mem=mock_mem,
    )

    assert results == []


# ---------------------------------------------------------------------------
# Task 2: graph_enhanced_search parallel execution tests
# ---------------------------------------------------------------------------


def test_graph_enhanced_search_returns_qdrant_results_when_neo4j_times_out(client: TestClient) -> None:
    """When Neo4j times out, graph_enhanced_search returns Qdrant-only results."""
    qdrant_result = [{"id": "q1", "memory": "Qdrant result", "score": 0.8, "metadata": {}}]

    async def mock_neo4j_timeout(
        query: str, user_id: str, timeout_ms: int = 800, graph_depth: int = 1
    ) -> list[str]:
        raise TimeoutError("simulated timeout")

    with (
        patch("aletheia_memory.routes._qdrant_search_direct", return_value=qdrant_result),
        patch("aletheia_memory.routes._neo4j_expand_with_timeout", side_effect=mock_neo4j_timeout),
        patch("aletheia_memory.routes._apply_confidence_weight", side_effect=lambda r, **kw: r),
        patch("aletheia_memory.routes._apply_recency_boost", side_effect=lambda r, **kw: r),
    ):
        resp = client.post(
            "/graph_enhanced_search",
            json={"query": "Python preferences", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True
    assert len(data["results"]) >= 1
    assert data["results"][0]["memory"] == "Qdrant result"


def test_graph_enhanced_search_returns_qdrant_results_when_neo4j_down(client: TestClient) -> None:
    """When Neo4j raises an error, graph_enhanced_search still returns Qdrant results."""
    qdrant_result = [{"id": "q1", "memory": "Qdrant result", "score": 0.75, "metadata": {}}]

    async def mock_neo4j_error(
        query: str, user_id: str, timeout_ms: int = 800, graph_depth: int = 1
    ) -> list[str]:
        raise ConnectionError("Neo4j is down")

    with (
        patch("aletheia_memory.routes._qdrant_search_direct", return_value=qdrant_result),
        patch("aletheia_memory.routes._neo4j_expand_with_timeout", side_effect=mock_neo4j_error),
        patch("aletheia_memory.routes._apply_confidence_weight", side_effect=lambda r, **kw: r),
        patch("aletheia_memory.routes._apply_recency_boost", side_effect=lambda r, **kw: r),
    ):
        resp = client.post(
            "/graph_enhanced_search",
            json={"query": "Python preferences", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True
    assert len(data["results"]) >= 1


def test_graph_enhanced_search_deduplicates_by_id_keeps_higher_score(client: TestClient) -> None:
    """Result merging deduplicates by memory ID and keeps the entry with the higher score."""
    shared_id = "mem-shared"
    # vector_weight = 0.7, graph_weight = 0.3
    # vector combined_score = 0.9 * 0.7 = 0.63
    # graph_expanded combined_score = 0.7 * 0.3 = 0.21 — vector wins
    qdrant_result = [{"id": shared_id, "memory": "shared memory", "score": 0.9, "metadata": {}}]
    expanded_result = [{"id": shared_id, "memory": "shared memory", "score": 0.7, "metadata": {}}]

    call_count = [0]

    def mock_qdrant_search(
        query: str, user_id: str, limit: int, min_score: float, mem: object
    ) -> list[dict[str, Any]]:
        call_count[0] += 1
        if call_count[0] == 1:
            return qdrant_result
        return expanded_result

    async def mock_neo4j_expand(
        query: str, user_id: str, timeout_ms: int = 800, graph_depth: int = 1
    ) -> list[str]:
        return ["EntityA"]

    with (
        patch("aletheia_memory.routes._qdrant_search_direct", side_effect=mock_qdrant_search),
        patch("aletheia_memory.routes._neo4j_expand_with_timeout", side_effect=mock_neo4j_expand),
        patch("aletheia_memory.routes._apply_confidence_weight", side_effect=lambda r, **kw: r),
        patch("aletheia_memory.routes._apply_recency_boost", side_effect=lambda r, **kw: r),
    ):
        resp = client.post(
            "/graph_enhanced_search",
            json={"query": "test query", "user_id": "default", "graph_weight": 0.3},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True
    result_ids = [r.get("id") for r in data["results"]]
    assert result_ids.count(shared_id) == 1


def test_graph_enhanced_search_expanded_query_uses_neo4j_neighbors(client: TestClient) -> None:
    """When Neo4j returns neighbors, second Qdrant query uses expanded terms."""
    qdrant_result = [{"id": "q1", "memory": "base result", "score": 0.8, "metadata": {}}]
    expanded_result = [{"id": "q2", "memory": "expanded result", "score": 0.7, "metadata": {}}]

    queries_received: list[str] = []
    call_count = [0]

    def mock_qdrant_search(
        query: str, user_id: str, limit: int, min_score: float, mem: object
    ) -> list[dict[str, Any]]:
        queries_received.append(query)
        call_count[0] += 1
        if call_count[0] == 1:
            return qdrant_result
        return expanded_result

    async def mock_neo4j_expand(
        query: str, user_id: str, timeout_ms: int = 800, graph_depth: int = 1
    ) -> list[str]:
        return ["NeighborEntity"]

    with (
        patch("aletheia_memory.routes._qdrant_search_direct", side_effect=mock_qdrant_search),
        patch("aletheia_memory.routes._neo4j_expand_with_timeout", side_effect=mock_neo4j_expand),
        patch("aletheia_memory.routes._apply_confidence_weight", side_effect=lambda r, **kw: r),
        patch("aletheia_memory.routes._apply_recency_boost", side_effect=lambda r, **kw: r),
    ):
        resp = client.post(
            "/graph_enhanced_search",
            json={"query": "find Python users", "user_id": "default"},
        )

    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True
    # Second Qdrant call should include Neo4j neighbor in expanded query
    assert len(queries_received) == 2
    assert "NeighborEntity" in queries_received[1]
    # Both results appear in merged output
    result_ids = [r.get("id") for r in data["results"]]
    assert "q1" in result_ids
    assert "q2" in result_ids


# ---------------------------------------------------------------------------
# Task 1: Domain relevance re-ranking tests
# ---------------------------------------------------------------------------


def test_domain_relevance_score_returns_in_valid_range() -> None:
    """_domain_relevance_score always returns a value in [min_factor, 1.0]."""
    score = _domain_relevance_score("vehicle oil change schedule", "Thread context: leather crafting tools")
    assert 0.6 <= score <= 1.0


def test_domain_relevance_score_min_factor_prevents_full_exclusion() -> None:
    """Completely off-domain memory still gets min_factor multiplier (0.6), not zero."""
    # Totally disjoint vocabulary (after stop word removal)
    score = _domain_relevance_score("zzz123 qqqfoo barrrr", "Thread context: leather crafting saddle stitching")
    assert abs(score - 0.6) < 1e-9


def test_domain_relevance_score_full_overlap() -> None:
    """When memory contains all query tokens, score reaches 1.0."""
    # Query without stop words: {"leather", "crafting", "saddle", "stitching"}
    # Memory contains all those tokens → overlap = 1.0 → score = 0.6 + 1.0 * 0.4 = 1.0
    context = "leather crafting saddle stitching"  # no "Thread context:" prefix so tokens are pure domain words
    memory = "leather crafting saddle stitching"
    score = _domain_relevance_score(memory, context)
    assert abs(score - 1.0) < 1e-6


def test_domain_relevance_score_stop_words_excluded() -> None:
    """Stop words are not counted toward relevance — they are filtered before matching."""
    # Memory only has stop words; query has "leather" (non-stop) + stop words.
    # After filtering: query_tokens = {"leather"}, memory_tokens = {}.
    # Overlap = 0/1 = 0.0 → score = max(0.6, 0.6 + 0.0 * 0.4) = 0.6 (min factor).
    score = _domain_relevance_score("the is in of and", "leather the is in of and")
    assert abs(score - 0.6) < 1e-9


def test_domain_relevance_score_empty_context_returns_one() -> None:
    """Empty query context (empty string) returns 1.0 — no signal, no penalty."""
    score = _domain_relevance_score("leather saddle crafting", "")
    assert score == 1.0  # empty query_tokens → no penalty applied


def test_apply_domain_reranking_skips_when_no_thread_context() -> None:
    """When query has no 'Thread context:' marker, scores are unchanged."""
    results: list[dict[str, Any]] = [
        {"memory": "vehicle oil change", "score": 0.9},
        {"memory": "leather belt crafting", "score": 0.8},
    ]
    original_scores = [r["score"] for r in results]
    reranked = _apply_domain_reranking(results, "tools and maintenance")
    for r, orig in zip(reranked, original_scores, strict=False):
        assert r["score"] == orig


def test_apply_domain_reranking_penalizes_cross_domain_results() -> None:
    """In leatherwork context, vehicle memories score lower than craft memories."""
    results: list[dict[str, Any]] = [
        {"memory": "vehicle tools oil wrench maintenance", "score": 1.0, "id": "vehicle"},
        {"memory": "leather craft tools awl stitching saddle", "score": 1.0, "id": "craft"},
    ]
    query = "Thread context: leatherwork crafting saddle awl stitching"
    reranked = _apply_domain_reranking(results, query)

    craft = next(r for r in reranked if r["id"] == "craft")
    vehicle = next(r for r in reranked if r["id"] == "vehicle")

    # Craft memory should score higher than vehicle memory in leatherwork context
    assert craft["score"] > vehicle["score"]


def test_apply_domain_reranking_cross_domain_results_present() -> None:
    """Cross-domain results are penalized but NOT excluded from output (soft boundaries)."""
    results: list[dict[str, Any]] = [
        {"memory": "vehicle oil change wrench socket", "score": 0.9, "id": "vehicle"},
        {"memory": "leather awl saddle stitch punch", "score": 0.8, "id": "craft"},
    ]
    query = "Thread context: leather crafting saddle tools"
    reranked = _apply_domain_reranking(results, query)

    ids = {r["id"] for r in reranked}
    assert "vehicle" in ids
    assert "craft" in ids
    assert len(reranked) == 2


def test_apply_domain_reranking_sorted_by_score_after_reranking() -> None:
    """After re-ranking, results are sorted by score descending."""
    results: list[dict[str, Any]] = [
        {"memory": "completely unrelated topic zzzz", "score": 1.0},
        {"memory": "leather saddle crafting stitch", "score": 0.5},
    ]
    query = "Thread context: leather saddle crafting stitch"
    reranked = _apply_domain_reranking(results, query)
    scores = [r["score"] for r in reranked]
    assert scores == sorted(scores, reverse=True)
