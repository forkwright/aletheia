# Tests for RELATES_TO backfill migration: batching, reclassification, deletion, checkpointing

import asyncio
import json
from unittest.mock import MagicMock, patch

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_edge(rid: int, source: str = "Alice", target: str = "Python") -> dict:
    return {"rid": rid, "source": source, "target": target, "props": {}}


def _mock_session(edges: list[dict] | None = None, stats: dict | None = None):
    """Build a mock Neo4j session."""
    session = MagicMock()

    # Default stats
    default_stats = {"relates_to": len(edges or []), "total": len(edges or []) + 5, "rate": 0.0}
    s = stats or default_stats

    stats_record = MagicMock()
    stats_record.__getitem__ = lambda self, key: s.get(key, 0)

    stats_result = MagicMock()
    stats_result.single.return_value = stats_record

    # Query routing: first call may be stats, subsequent calls return edges
    edge_records = []
    for e in (edges or []):
        rec = MagicMock()
        rec.__getitem__ = lambda self, key, _e=e: _e.get(key)
        edge_records.append(rec)

    edges_result = MagicMock()
    edges_result.__iter__ = lambda self: iter(edge_records)

    session.run.side_effect = [stats_result, edges_result]
    return session


# ---------------------------------------------------------------------------
# test_dry_run_does_not_modify
# ---------------------------------------------------------------------------


def test_dry_run_does_not_modify(tmp_path):
    """dry_run=True: no CREATE or DELETE queries issued to Neo4j."""
    from backfill_relates_to import process_batch

    mock_session = MagicMock()
    mock_client = MagicMock()

    edge = _make_edge(1, "Alice", "Python")

    async def _run():
        mock_client.messages.create = MagicMock(
            return_value=MagicMock(content=[MagicMock(text="USES")])
        )
        processed = set()
        await process_batch(
            client=mock_client,
            model="claude-test",
            batch=[edge],
            dry_run=True,
            session=mock_session,
            processed_ids=processed,
        )
        return processed

    processed = asyncio.run(_run())

    # Dry run: no Neo4j mutations
    mock_session.run.assert_not_called()
    # Dry run: processed_ids NOT updated
    assert len(processed) == 0


# ---------------------------------------------------------------------------
# test_reclassification_creates_typed_edge
# ---------------------------------------------------------------------------


def test_reclassification_creates_typed_edge(tmp_path):
    """LLM returns KNOWS -> CREATE KNOWS edge and DELETE old RELATES_TO."""
    import backfill_relates_to as bfm

    mock_session = MagicMock()
    mock_client = MagicMock()
    mock_client.messages.create = MagicMock(
        return_value=MagicMock(content=[MagicMock(text="KNOWS")])
    )

    edge = _make_edge(42, "Alice", "Bob")

    async def _run():
        processed = set()
        with patch.object(bfm, "save_checkpoint"):
            await bfm.process_batch(
                client=mock_client,
                model="claude-test",
                batch=[edge],
                dry_run=False,
                session=mock_session,
                processed_ids=processed,
            )
        return processed

    processed = asyncio.run(_run())

    # Should issue apply_reclassification query (CREATE + DELETE in one query)
    assert mock_session.run.called
    call_args = mock_session.run.call_args
    cypher = call_args[0][0]
    assert "KNOWS" in cypher
    assert "DELETE" in cypher

    # Edge ID tracked as processed
    assert 42 in processed


# ---------------------------------------------------------------------------
# test_unclassifiable_edge_deleted
# ---------------------------------------------------------------------------


def test_unclassifiable_edge_deleted(tmp_path):
    """LLM returns DELETE -> only DELETE query issued, no CREATE."""
    import backfill_relates_to as bfm

    mock_session = MagicMock()
    mock_client = MagicMock()
    mock_client.messages.create = MagicMock(
        return_value=MagicMock(content=[MagicMock(text="DELETE")])
    )

    edge = _make_edge(99, "Cody", "Unknown")

    async def _run():
        processed = set()
        with patch.object(bfm, "save_checkpoint"):
            await bfm.process_batch(
                client=mock_client,
                model="claude-test",
                batch=[edge],
                dry_run=False,
                session=mock_session,
                processed_ids=processed,
            )
        return processed

    processed = asyncio.run(_run())

    assert mock_session.run.called
    call_args = mock_session.run.call_args
    cypher = call_args[0][0]
    # DELETE-only query — should not contain CREATE
    assert "DELETE" in cypher
    assert "CREATE" not in cypher

    assert 99 in processed


# ---------------------------------------------------------------------------
# test_checkpoint_saves_progress
# ---------------------------------------------------------------------------


def test_checkpoint_saves_progress(tmp_path):
    """Checkpoint file is written with processed edge IDs after each edge."""
    import backfill_relates_to as bfm

    mock_session = MagicMock()
    mock_client = MagicMock()
    mock_client.messages.create = MagicMock(
        return_value=MagicMock(content=[MagicMock(text="KNOWS")])
    )

    checkpoint_path = tmp_path / "backfill_state.json"

    edges = [_make_edge(i) for i in range(3)]

    async def _run():
        processed = set()
        with patch.object(bfm, "CHECKPOINT_FILE", checkpoint_path):
            await bfm.process_batch(
                client=mock_client,
                model="claude-test",
                batch=edges,
                dry_run=False,
                session=mock_session,
                processed_ids=processed,
            )
        return processed

    processed = asyncio.run(_run())

    assert checkpoint_path.exists()
    data = json.loads(checkpoint_path.read_text())
    assert set(data["processed_ids"]) == {0, 1, 2}
    assert processed == {0, 1, 2}


# ---------------------------------------------------------------------------
# test_checkpoint_resumes
# ---------------------------------------------------------------------------


def test_checkpoint_resumes(tmp_path):
    """Already-processed edge IDs in checkpoint are skipped on resume."""
    import backfill_relates_to as bfm

    checkpoint_path = tmp_path / "backfill_state.json"
    checkpoint_path.write_text(json.dumps({"processed_ids": [0, 1]}))

    with patch.object(bfm, "CHECKPOINT_FILE", checkpoint_path):
        loaded = bfm.load_checkpoint()

    assert loaded == {0, 1}

    # Simulate the filtering step (done in run_backfill)
    all_edges = [_make_edge(i) for i in range(4)]
    pending = [e for e in all_edges if e["rid"] not in loaded]

    assert [e["rid"] for e in pending] == [2, 3]


# ---------------------------------------------------------------------------
# test_batch_sizing
# ---------------------------------------------------------------------------


def test_batch_sizing():
    """120 edges with batch_size=50 should produce batches of 50, 50, 20."""
    edges = [_make_edge(i) for i in range(120)]
    batch_size = 50

    batches = [
        edges[i : i + batch_size]
        for i in range(0, len(edges), batch_size)
    ]

    assert len(batches) == 3
    assert len(batches[0]) == 50
    assert len(batches[1]) == 50
    assert len(batches[2]) == 20
