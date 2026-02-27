# Tests for memory evolution — exponential decay, reinforcement, and decay endpoint behavior
import math
from typing import Any
from unittest.mock import MagicMock, patch

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from aletheia_memory.evolution import evolution_router, exponential_decay_penalty

# ---------------------------------------------------------------------------
# exponential_decay_penalty unit tests
# ---------------------------------------------------------------------------


def test_decay_penalty_at_zero_days() -> None:
    """0 days inactive -> multiplier 1.0 (no decay)."""
    result = exponential_decay_penalty(0)
    assert result == pytest.approx(1.0, abs=1e-9)


def test_decay_penalty_at_14_days_half_life() -> None:
    """14 days -> ~0.50 (half-life at lambda=0.05)."""
    result = exponential_decay_penalty(14)
    expected = math.exp(-0.05 * 14)
    assert result == pytest.approx(expected, abs=1e-6)
    assert 0.49 < result < 0.51


def test_decay_penalty_at_30_days() -> None:
    """30 days -> ~0.22."""
    result = exponential_decay_penalty(30)
    expected = math.exp(-0.05 * 30)
    assert result == pytest.approx(expected, abs=1e-6)
    assert 0.21 < result < 0.24


def test_decay_penalty_at_90_days() -> None:
    """90 days -> ~0.01 (near-zero salience)."""
    result = exponential_decay_penalty(90)
    expected = math.exp(-0.05 * 90)
    assert result == pytest.approx(expected, abs=1e-6)
    assert result < 0.02


def test_decay_penalty_uses_math_exp() -> None:
    """Verify formula matches math.exp directly — not linear."""
    for days in [0, 7, 14, 30, 60, 90]:
        assert exponential_decay_penalty(days) == pytest.approx(math.exp(-0.05 * days), abs=1e-9)


def test_decay_penalty_custom_lambda() -> None:
    """Custom lambda changes half-life."""
    # lambda=0.1 gives ~7-day half-life
    result = exponential_decay_penalty(7, lambda_=0.1)
    assert result == pytest.approx(math.exp(-0.1 * 7), abs=1e-9)


def test_decay_penalty_is_always_in_range() -> None:
    """Multiplier stays in [0, 1] for all positive days."""
    for days in [0, 1, 7, 14, 30, 90, 365]:
        result = exponential_decay_penalty(days)
        assert 0.0 <= result <= 1.0


# ---------------------------------------------------------------------------
# Endpoint behavior tests — reinforce and decay via TestClient
# ---------------------------------------------------------------------------


def _make_app() -> FastAPI:
    app = FastAPI()
    app.include_router(evolution_router)
    return app


@pytest.fixture
def client() -> TestClient:
    return TestClient(_make_app())


def test_reinforce_updates_last_accessed(client: TestClient) -> None:
    """Reinforce endpoint updates last_accessed to current time in Neo4j."""
    mock_record = {"count": 1}

    mock_session = MagicMock()
    mock_session.__enter__ = MagicMock(return_value=mock_session)
    mock_session.__exit__ = MagicMock(return_value=False)
    mock_run_result = MagicMock()
    mock_run_result.single.return_value = mock_record
    mock_session.run.return_value = mock_run_result

    mock_driver = MagicMock()
    mock_driver.session.return_value = mock_session

    with (
        patch("aletheia_memory.evolution.neo4j_available", return_value=True),
        patch("aletheia_memory.evolution.neo4j_driver", return_value=mock_driver),
    ):
        response = client.post(
            "/evolution/reinforce",
            json={"memory_id": "mem-abc", "user_id": "default"},
        )

    assert response.status_code == 200
    data: dict[str, Any] = response.json()
    assert data["ok"] is True
    assert data["reinforced"] is True
    assert data["memory_id"] == "mem-abc"

    # Verify the Cypher query sets last_accessed
    cypher_call = mock_session.run.call_args[0][0]
    assert "last_accessed" in cypher_call


def test_reinforce_does_not_set_last_decayed(client: TestClient) -> None:
    """Reinforce endpoint does NOT touch last_decayed — only tracks access."""
    mock_record = {"count": 2}

    mock_session = MagicMock()
    mock_session.__enter__ = MagicMock(return_value=mock_session)
    mock_session.__exit__ = MagicMock(return_value=False)
    mock_run_result = MagicMock()
    mock_run_result.single.return_value = mock_record
    mock_session.run.return_value = mock_run_result

    mock_driver = MagicMock()
    mock_driver.session.return_value = mock_session

    with (
        patch("aletheia_memory.evolution.neo4j_available", return_value=True),
        patch("aletheia_memory.evolution.neo4j_driver", return_value=mock_driver),
    ):
        client.post(
            "/evolution/reinforce",
            json={"memory_id": "mem-xyz", "user_id": "default"},
        )

    cypher_call = mock_session.run.call_args[0][0]
    assert "last_decayed" not in cypher_call


def test_decay_does_not_modify_last_accessed() -> None:
    """Decay endpoint writes decay_count and last_decayed — never last_accessed."""
    mock_mem = MagicMock()
    mock_mem.get_all.return_value = {"results": [{"id": "mem-001", "memory": "some fact"}]}

    mock_session = MagicMock()
    mock_session.__enter__ = MagicMock(return_value=mock_session)
    mock_session.__exit__ = MagicMock(return_value=False)

    # First call: fetch recently_accessed set (returns empty — no exemptions)
    mock_result_empty = MagicMock()
    mock_result_empty.__iter__ = MagicMock(return_value=iter([]))
    # Second call: the decay write
    mock_result_decay = MagicMock()
    mock_session.run.side_effect = [mock_result_empty, mock_result_decay]

    mock_driver = MagicMock()
    mock_driver.session.return_value = mock_session

    # Decay endpoint reads request.app.state.memory — wire it directly
    app = _make_app()
    app.state.memory = mock_mem

    with (
        patch("aletheia_memory.evolution.neo4j_available", return_value=True),
        patch("aletheia_memory.evolution.neo4j_driver", return_value=mock_driver),
        patch("aletheia_memory.evolution.asyncio.to_thread", side_effect=lambda f, *a, **kw: f(*a, **kw)),
    ):
        decay_client = TestClient(app)
        response = decay_client.post(
            "/evolution/decay",
            json={"user_id": "default", "days_inactive": 30},
        )

    assert response.status_code == 200

    # Check that no decay Cypher sets last_accessed
    for call in mock_session.run.call_args_list:
        cypher = call[0][0] if call[0] else ""
        if "last_decayed" in cypher or "decay_count" in cypher:
            assert "last_accessed" not in cypher, (
                f"Decay Cypher must not modify last_accessed: {cypher}"
            )
