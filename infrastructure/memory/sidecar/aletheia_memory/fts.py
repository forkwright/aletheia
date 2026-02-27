"""
FTS5 full-text search index for BM25 keyword retrieval.

Complements Qdrant vector search — BM25 is strong where vectors are weak
(exact terms, proper nouns, config keys, error codes).

Storage: SQLite FTS5 virtual table, colocated with sidecar.
Lifecycle: populated on ingest (/add, /add_direct, /add_batch),
           queried during /search when hybrid=true.
"""

import logging
import os
import sqlite3
import threading
from dataclasses import dataclass

logger = logging.getLogger("aletheia.fts")

# Default path — next to sidecar data
_DB_PATH = os.environ.get("FTS_DB_PATH", os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "data", "fts_index.db"
))

_local = threading.local()


def _get_conn() -> sqlite3.Connection:
    """Thread-local SQLite connection (FTS5 is not thread-safe across connections)."""
    conn: sqlite3.Connection | None = getattr(_local, "conn", None)
    if conn is None:
        os.makedirs(os.path.dirname(_DB_PATH), exist_ok=True)
        conn = sqlite3.connect(_DB_PATH, check_same_thread=False)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA synchronous=NORMAL")
        _ensure_schema(conn)
        _local.conn = conn
    return conn


def _ensure_schema(conn: sqlite3.Connection) -> None:
    """Create FTS5 table and metadata table if they don't exist."""
    conn.execute("""
        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            memory_id UNINDEXED,
            agent_id UNINDEXED,
            user_id UNINDEXED,
            content,
            tokenize='porter unicode61'
        )
    """)
    # Metadata table for tracking indexed point IDs (avoid duplicates on re-index)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS fts_meta (
            memory_id TEXT PRIMARY KEY,
            indexed_at TEXT DEFAULT (datetime('now'))
        )
    """)
    conn.commit()


def index_memory(memory_id: str, content: str, agent_id: str = "", user_id: str = "") -> None:
    """Index a single memory into FTS5. Idempotent — skips if already indexed."""
    conn = _get_conn()
    # Check if already indexed
    row = conn.execute("SELECT 1 FROM fts_meta WHERE memory_id = ?", (memory_id,)).fetchone()
    if row:
        return
    conn.execute(
        "INSERT INTO memories_fts (memory_id, agent_id, user_id, content) VALUES (?, ?, ?, ?)",
        (memory_id, agent_id, user_id, content),
    )
    conn.execute(
        "INSERT OR IGNORE INTO fts_meta (memory_id) VALUES (?)",
        (memory_id,),
    )
    conn.commit()


def index_batch(items: list[dict]) -> int:
    """Batch index memories. Each item: {memory_id, content, agent_id?, user_id?}.
    Returns count of newly indexed items."""
    conn = _get_conn()
    count = 0
    for item in items:
        mid = item["memory_id"]
        row = conn.execute("SELECT 1 FROM fts_meta WHERE memory_id = ?", (mid,)).fetchone()
        if row:
            continue
        conn.execute(
            "INSERT INTO memories_fts (memory_id, agent_id, user_id, content) VALUES (?, ?, ?, ?)",
            (mid, item.get("agent_id", ""), item.get("user_id", ""), item["content"]),
        )
        conn.execute("INSERT OR IGNORE INTO fts_meta (memory_id) VALUES (?)", (mid,))
        count += 1
    conn.commit()
    return count


def remove_memory(memory_id: str) -> None:
    """Remove a memory from the FTS index."""
    conn = _get_conn()
    conn.execute("DELETE FROM memories_fts WHERE memory_id = ?", (memory_id,))
    conn.execute("DELETE FROM fts_meta WHERE memory_id = ?", (memory_id,))
    conn.commit()


@dataclass
class FtsHit:
    memory_id: str
    content: str
    agent_id: str
    user_id: str
    bm25_score: float  # Negative (SQLite convention) — lower is better match


def search_bm25(
    query: str,
    limit: int = 20,
    agent_id: str | None = None,
    user_id: str | None = None,
) -> list[FtsHit]:
    """BM25 keyword search via FTS5.

    Returns results sorted by relevance (best first).
    BM25 scores are negative in SQLite — we negate them so higher = better,
    matching the Qdrant convention.
    """
    conn = _get_conn()

    # Sanitize query for FTS5 — escape special characters, handle edge cases
    safe_query = _sanitize_fts_query(query)
    if not safe_query:
        return []

    try:
        # Build query with optional filters
        sql = "SELECT memory_id, content, agent_id, user_id, rank FROM memories_fts WHERE memories_fts MATCH ?"
        params: list[str | int] = [safe_query]

        if agent_id:
            # FTS5 doesn't support WHERE on UNINDEXED columns in MATCH,
            # so filter post-query
            pass
        if user_id:
            pass

        sql += " ORDER BY rank LIMIT ?"
        params.append(limit * 3 if (agent_id or user_id) else limit)  # Over-fetch if filtering

        rows = conn.execute(sql, params).fetchall()
    except sqlite3.OperationalError as e:
        logger.warning("FTS5 query failed for %r: %s", query[:50], e)
        return []

    hits: list[FtsHit] = []
    for mid, content, aid, uid, rank in rows:
        # Post-filter by agent_id/user_id if requested
        if agent_id and aid != agent_id:
            continue
        if user_id and uid != user_id:
            continue
        hits.append(FtsHit(
            memory_id=mid,
            content=content,
            agent_id=aid,
            user_id=uid,
            bm25_score=-rank,  # Negate: SQLite rank is negative, we want higher=better
        ))
        if len(hits) >= limit:
            break

    return hits


def _sanitize_fts_query(query: str) -> str:
    """Convert a natural language query into a safe FTS5 query.

    Strategy: extract alphanumeric tokens, join with implicit AND.
    This avoids FTS5 syntax errors from special characters while
    preserving keyword matching quality.
    """
    # Split into tokens, keep only alphanumeric words
    tokens = []
    for word in query.split():
        cleaned = "".join(c for c in word if c.isalnum() or c == "_")
        if cleaned and len(cleaned) > 1:  # Skip single chars
            tokens.append(cleaned)

    if not tokens:
        return ""

    # Use implicit AND (space-separated in FTS5)
    # Limit to first 20 tokens to avoid pathological queries
    return " ".join(tokens[:20])


def get_stats() -> dict:
    """Return FTS index statistics."""
    conn = _get_conn()
    total = conn.execute("SELECT COUNT(*) FROM fts_meta").fetchone()[0]
    return {"indexed_count": total, "db_path": _DB_PATH}


def close() -> None:
    """Close the thread-local connection."""
    conn: sqlite3.Connection | None = getattr(_local, "conn", None)
    if conn:
        conn.close()
        _local.conn = None
