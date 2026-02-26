# Tests for /health endpoint semantic metrics and threshold evaluation
import asyncio
from collections import deque
from unittest.mock import MagicMock, patch

import pytest

from aletheia_memory.routes import (
    _collect_neo4j_metrics,  # pyright: ignore[reportPrivateUsage]
    _compute_p95,  # pyright: ignore[reportPrivateUsage]
    _evaluate_thresholds,  # pyright: ignore[reportPrivateUsage]
    _parse_thresholds,  # pyright: ignore[reportPrivateUsage]
)

# ---------------------------------------------------------------------------
# _compute_p95
# ---------------------------------------------------------------------------


def test_compute_p95_empty_deque_returns_none() -> None:
    assert _compute_p95(deque()) is None


def test_compute_p95_fewer_than_5_samples_returns_none() -> None:
    samples: deque[float] = deque([0.1, 0.2, 0.3, 0.4])
    assert _compute_p95(samples) is None


def test_compute_p95_exactly_5_samples_returns_value() -> None:
    samples: deque[float] = deque([0.1, 0.2, 0.3, 0.4, 0.5])
    result = _compute_p95(samples)
    assert result is not None
    # sorted=[0.1, 0.2, 0.3, 0.4, 0.5], idx = int(0.95 * 5) = 4 → 0.5
    assert result == 0.5


def test_compute_p95_100_samples_selects_95th_percentile() -> None:
    # 100 samples: 1.0, 2.0, ..., 100.0
    samples: deque[float] = deque(float(i) for i in range(1, 101))
    result = _compute_p95(samples)
    assert result is not None
    # sorted idx = int(0.95 * 100) = 95 → value 96.0 (0-indexed)
    assert result == 96.0


def test_compute_p95_respects_maxlen() -> None:
    samples: deque[float] = deque(maxlen=100)
    for i in range(1, 201):
        samples.append(float(i))
    # Should only have last 100 values: 101..200
    result = _compute_p95(samples)
    assert result is not None
    assert result >= 100.0


# ---------------------------------------------------------------------------
# _parse_thresholds
# ---------------------------------------------------------------------------


def test_parse_thresholds_none_returns_defaults() -> None:
    result = _parse_thresholds(None)
    assert result["noiseRateMax"] == 0.05
    assert result["orphanCountMax"] == 50
    assert result["relatesToRateMax"] == 0.30
    assert result["recallLatencyP95Ms"] == 1000
    assert result["flushSuccessRateMin"] == 0.95


def test_parse_thresholds_overrides_defaults() -> None:
    import json
    overrides = json.dumps({"noiseRateMax": 0.10, "orphanCountMax": 100})
    result = _parse_thresholds(overrides)
    assert result["noiseRateMax"] == 0.10
    assert result["orphanCountMax"] == 100
    # Unspecified defaults are preserved
    assert result["relatesToRateMax"] == 0.30


def test_parse_thresholds_invalid_json_returns_defaults() -> None:
    result = _parse_thresholds("not-valid-json")
    assert result["noiseRateMax"] == 0.05


# ---------------------------------------------------------------------------
# _evaluate_thresholds
# ---------------------------------------------------------------------------

_DEFAULT_THRESHOLDS = {
    "noiseRateMax": 0.05,
    "orphanCountMax": 50,
    "relatesToRateMax": 0.30,
    "recallLatencyP95Ms": 1000,
    "flushSuccessRateMin": 0.95,
}


def test_evaluate_thresholds_all_within_returns_healthy() -> None:
    result = _evaluate_thresholds(
        noise_rate=0.02,
        orphan_count=10,
        relates_to_rate=0.15,
        latency_p95_ms=200.0,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "healthy"
    assert result["thresholds"]["exceeded"] == []


def test_evaluate_thresholds_one_exceeded_returns_degraded() -> None:
    result = _evaluate_thresholds(
        noise_rate=0.08,  # exceeds noiseRateMax=0.05
        orphan_count=10,
        relates_to_rate=0.15,
        latency_p95_ms=200.0,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "degraded"
    assert "noise_rate" in result["thresholds"]["exceeded"]
    assert result["thresholds"]["values"]["noise_rate"] == pytest.approx(0.08)


def test_evaluate_thresholds_two_exceeded_returns_critical() -> None:
    result = _evaluate_thresholds(
        noise_rate=0.08,  # exceeds
        orphan_count=100,  # exceeds
        relates_to_rate=0.15,
        latency_p95_ms=200.0,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "critical"
    assert len(result["thresholds"]["exceeded"]) >= 2


def test_evaluate_thresholds_connectivity_failure_returns_critical() -> None:
    result = _evaluate_thresholds(
        noise_rate=None,
        orphan_count=None,
        relates_to_rate=None,
        latency_p95_ms=None,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=True,
    )
    assert result["status"] == "critical"


def test_evaluate_thresholds_none_metrics_not_counted_as_exceeded() -> None:
    # None means insufficient data — should not be counted as a threshold violation
    result = _evaluate_thresholds(
        noise_rate=None,
        orphan_count=None,
        relates_to_rate=None,
        latency_p95_ms=None,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "healthy"
    assert result["thresholds"]["exceeded"] == []


def test_evaluate_thresholds_relates_to_rate_exceeded() -> None:
    result = _evaluate_thresholds(
        noise_rate=0.02,
        orphan_count=5,
        relates_to_rate=0.40,  # exceeds 0.30
        latency_p95_ms=200.0,
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "degraded"
    assert "relates_to_rate" in result["thresholds"]["exceeded"]


def test_evaluate_thresholds_latency_exceeded() -> None:
    result = _evaluate_thresholds(
        noise_rate=0.02,
        orphan_count=5,
        relates_to_rate=0.15,
        latency_p95_ms=1500.0,  # exceeds 1000ms
        thresholds=_DEFAULT_THRESHOLDS,
        connectivity_failed=False,
    )
    assert result["status"] == "degraded"
    assert "recall_latency_p95_ms" in result["thresholds"]["exceeded"]


# ---------------------------------------------------------------------------
# _collect_neo4j_metrics — Neo4j failure returns None for relates_to_rate
# ---------------------------------------------------------------------------


def test_collect_neo4j_metrics_neo4j_unavailable_returns_none() -> None:
    result = asyncio.run(_collect_neo4j_metrics(neo4j_ok=False))
    assert result["relates_to_rate"] is None
    assert result["total_relationships"] == 0


def test_collect_neo4j_metrics_exception_returns_none() -> None:
    with (
        patch("aletheia_memory.routes.neo4j_driver") as mock_driver,
        patch("aletheia_memory.routes.mark_neo4j_down") as mock_down,
    ):
        mock_driver.side_effect = Exception("connection refused")
        result = asyncio.run(_collect_neo4j_metrics(neo4j_ok=True))
        assert result["relates_to_rate"] is None
        mock_down.assert_called_once()


def test_collect_neo4j_metrics_zero_total_returns_none_rate() -> None:
    with (
        patch("aletheia_memory.routes.neo4j_driver") as mock_driver,
        patch("aletheia_memory.routes.mark_neo4j_ok"),
    ):
        mock_session = MagicMock()
        mock_session.__enter__ = MagicMock(return_value=mock_session)
        mock_session.__exit__ = MagicMock(return_value=False)
        mock_driver.return_value.session.return_value = mock_session

        total_single = MagicMock()
        total_single.__getitem__ = MagicMock(return_value=0)
        relates_single = MagicMock()
        relates_single.__getitem__ = MagicMock(return_value=0)

        mock_session.run.side_effect = [
            MagicMock(single=MagicMock(return_value=total_single)),
            MagicMock(single=MagicMock(return_value=relates_single)),
        ]
        mock_driver.return_value.close = MagicMock()

        result = asyncio.run(_collect_neo4j_metrics(neo4j_ok=True))
        # 0 total relationships → rate is None
        assert result["relates_to_rate"] is None
