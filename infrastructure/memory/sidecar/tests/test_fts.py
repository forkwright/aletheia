"""Tests for FTS5 keyword search module."""
import contextlib
import os
import tempfile

import pytest

# Override DB path before importing fts
_tmpdir = tempfile.mkdtemp()
os.environ["FTS_DB_PATH"] = os.path.join(_tmpdir, "test_fts.db")

from aletheia_memory.fts import (  # noqa: E402
    _sanitize_fts_query,
    close,
    get_stats,
    index_batch,
    index_memory,
    remove_memory,
    search_bm25,
)


@pytest.fixture(autouse=True)
def clean_db():
    """Reset DB between tests by clearing thread-local connection."""
    yield
    close()
    db_path = os.environ["FTS_DB_PATH"]
    if os.path.exists(db_path):
        with contextlib.suppress(OSError):
            os.remove(db_path)


class TestIndexAndSearch:
    def test_index_and_find(self):
        index_memory("m1", "Cody prefers dark roast coffee in the morning", agent_id="syn")
        index_memory("m2", "The truck needs new brake pads", agent_id="akron")
        index_memory("m3", "Kendall's birthday is in September", agent_id="syl")

        hits = search_bm25("coffee morning")
        assert len(hits) >= 1
        assert hits[0].memory_id == "m1"
        assert hits[0].bm25_score > 0  # We negate SQLite's negative rank

    def test_exact_keyword_match(self):
        """BM25 should excel at exact keyword matching — the weakness of vectors."""
        index_memory("k1", "Error code ECONNREFUSED on port 8230", agent_id="syn")
        index_memory("k2", "The connection to the database was refused", agent_id="syn")

        hits = search_bm25("ECONNREFUSED")
        assert len(hits) >= 1
        assert hits[0].memory_id == "k1"

    def test_idempotent_indexing(self):
        index_memory("dup1", "same memory text", agent_id="syn")
        index_memory("dup1", "same memory text", agent_id="syn")  # Should be no-op

        stats = get_stats()
        assert stats["indexed_count"] == 1

    def test_agent_id_filter(self):
        index_memory("a1", "shared topic about trucks", agent_id="akron")
        index_memory("a2", "shared topic about trucks and maintenance", agent_id="syn")

        hits = search_bm25("trucks", agent_id="akron")
        assert all(h.agent_id == "akron" for h in hits)

    def test_remove(self):
        index_memory("rm1", "temporary memory to delete", agent_id="syn")
        assert len(search_bm25("temporary memory")) >= 1

        remove_memory("rm1")
        assert len(search_bm25("temporary memory")) == 0

    def test_batch_index(self):
        items = [
            {"memory_id": "b1", "content": "batch item one about leather", "agent_id": "demiurge"},
            {"memory_id": "b2", "content": "batch item two about crafting", "agent_id": "demiurge"},
            {"memory_id": "b3", "content": "batch item three about tooling", "agent_id": "syn"},
        ]
        count = index_batch(items)
        assert count == 3

        # Second call should skip all
        count2 = index_batch(items)
        assert count2 == 0

        hits = search_bm25("leather")
        assert len(hits) >= 1
        assert hits[0].memory_id == "b1"


class TestSanitizeQuery:
    def test_normal_query(self):
        assert _sanitize_fts_query("hello world") == "hello world"

    def test_special_chars(self):
        result = _sanitize_fts_query('error: "ECONNREFUSED" on port 8230')
        assert "ECONNREFUSED" in result
        assert '"' not in result

    def test_single_chars_filtered(self):
        result = _sanitize_fts_query("a b cd ef")
        assert result == "cd ef"

    def test_empty_query(self):
        assert _sanitize_fts_query("") == ""
        assert _sanitize_fts_query("! @ #") == ""

    def test_long_query_truncated(self):
        tokens = " ".join(f"word{i}" for i in range(30))
        result = _sanitize_fts_query(tokens)
        assert len(result.split()) == 20


class TestStats:
    def test_stats_empty(self):
        stats = get_stats()
        assert stats["indexed_count"] == 0
        assert "db_path" in stats

    def test_stats_after_indexing(self):
        index_memory("s1", "test memory", agent_id="syn")
        stats = get_stats()
        assert stats["indexed_count"] == 1
