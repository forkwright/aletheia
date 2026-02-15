#!/usr/bin/env python3
# Weekly pattern extraction — distill domain-general reasoning patterns from successful completions

import json
import os
import sqlite3
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import httpx
from loguru import logger

logger.remove()
logger.add(sys.stderr, format="{time:HH:mm:ss} | {level:<7} | {message}", level="INFO")

ALETHEIA_HOME = Path(os.environ.get("ALETHEIA_HOME", "/mnt/ssd/aletheia"))
SESSIONS_DB = Path(os.environ.get("SESSIONS_DB", ""))
SIDECAR_URL = os.environ.get("ALETHEIA_MEMORY_URL", "http://127.0.0.1:8230")
PATTERNS_FILE = ALETHEIA_HOME / "shared" / "memory" / "patterns.json"
MAX_PATTERNS = 5


def get_recent_sessions(days: int = 7) -> list[dict]:
    """Get sessions with successful completions from the last N days."""
    conn = sqlite3.connect(str(SESSIONS_DB))
    conn.row_factory = sqlite3.Row
    cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()

    rows = conn.execute("""
        SELECT s.id, s.nous_id, s.session_key, s.updated_at,
               COUNT(m.id) as msg_count
        FROM sessions s
        JOIN messages m ON m.session_id = s.id
        WHERE s.status = 'active'
          AND s.updated_at > ?
          AND m.role = 'assistant'
        GROUP BY s.id
        HAVING msg_count >= 3
        ORDER BY s.updated_at DESC
        LIMIT 50
    """, (cutoff,)).fetchall()

    sessions = [dict(r) for r in rows]
    conn.close()
    return sessions


def get_session_summaries(session_ids: list[str]) -> list[dict]:
    """Get distillation summaries for sessions."""
    if not session_ids:
        return []

    conn = sqlite3.connect(str(SESSIONS_DB))
    conn.row_factory = sqlite3.Row
    placeholders = ",".join("?" * len(session_ids))

    rows = conn.execute(f"""
        SELECT session_id, content, created_at
        FROM messages
        WHERE session_id IN ({placeholders})
          AND role = 'assistant'
          AND is_distilled = 0
        ORDER BY created_at DESC
    """, session_ids).fetchall()

    summaries = [dict(r) for r in rows]
    conn.close()
    return summaries


def extract_patterns(summaries: list[dict]) -> list[dict]:
    """Use sidecar's extraction to find common patterns across summaries."""
    if not summaries:
        return []

    # Group by domain (nous_id prefix)
    domain_texts: dict[str, list[str]] = {}
    for s in summaries:
        domain = s.get("session_id", "")[:8]
        if domain not in domain_texts:
            domain_texts[domain] = []
        content = s.get("content", "")
        if isinstance(content, str) and len(content) > 50:
            domain_texts[domain].append(content[:500])

    # Search for recurring themes in memory
    patterns = []
    try:
        with httpx.Client(timeout=30.0) as client:
            for query in ["reasoning approach", "problem solving", "decision making", "coordination"]:
                resp = client.post(
                    f"{SIDECAR_URL}/search",
                    json={"query": query, "user_id": "default", "limit": 5},
                )
                if resp.status_code == 200:
                    results = resp.json().get("results", [])
                    for r in results:
                        mem = r.get("memory", "")
                        if len(mem) > 20:
                            patterns.append({
                                "text": mem,
                                "score": r.get("score", 0),
                                "source": query,
                            })
    except Exception as e:
        logger.warning(f"Pattern search failed: {e}")

    # Deduplicate and rank
    seen = set()
    unique = []
    for p in sorted(patterns, key=lambda x: x["score"], reverse=True):
        text = p["text"].strip().lower()[:80]
        if text not in seen:
            seen.add(text)
            unique.append(p)

    return unique[:MAX_PATTERNS]


def save_patterns(patterns: list[dict]) -> None:
    """Save extracted patterns for bootstrap injection."""
    PATTERNS_FILE.parent.mkdir(parents=True, exist_ok=True)
    data = {
        "extracted_at": datetime.now(timezone.utc).isoformat(),
        "patterns": [{"text": p["text"], "source": p["source"]} for p in patterns],
        "count": len(patterns),
    }
    PATTERNS_FILE.write_text(json.dumps(data, indent=2))
    logger.info(f"Saved {len(patterns)} patterns to {PATTERNS_FILE}")


def main() -> None:
    days = 7
    for arg in sys.argv[1:]:
        if arg.startswith("--days="):
            days = int(arg.split("=", 1)[1])

    logger.info(f"Extracting patterns from last {days} days")

    sessions = get_recent_sessions(days)
    logger.info(f"Found {len(sessions)} active sessions")

    if not sessions:
        logger.info("No sessions to analyze")
        return

    session_ids = [s["id"] for s in sessions]
    summaries = get_session_summaries(session_ids)
    logger.info(f"Got {len(summaries)} messages to analyze")

    patterns = extract_patterns(summaries)
    logger.info(f"Extracted {len(patterns)} unique patterns")

    for i, p in enumerate(patterns):
        print(f"  {i+1}. [{p['source']}] {p['text'][:80]}")

    save_patterns(patterns)


if __name__ == "__main__":
    main()
