#!/usr/bin/env python3
"""Backfill Chiron's memory from existing conversations.

Sources:
  1. Aletheia sessions.db (Chiron conversations)
  2. Claude Code JSONL transcripts (user's Claude Code sessions)

Sends each conversational exchange to the Mem0 sidecar /add endpoint
for fact extraction, embedding, and graph storage.
"""

import argparse
import os
import json
import logging
import sqlite3
import sys
import time
from pathlib import Path

import httpx

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("backfill")

SIDECAR_URL = "http://127.0.0.1:8230"
USER_ID = os.environ.get("ALETHEIA_USER", "default")
AGENT_ID = "chiron"
SESSIONS_DB = Path.home() / ".aletheia" / "sessions.db"
CLAUDE_CODE_DIR = Path(os.environ.get("CLAUDE_CODE_PROJECT_DIR", str(Path.home() / ".claude" / "projects")))

# Batch user+assistant turns into chunks of this many characters
MAX_CHUNK_CHARS = 3000


def add_memory(client: httpx.Client, text: str, source: str) -> dict | None:
    """Send text to sidecar for fact extraction."""
    if not text.strip():
        return None
    try:
        r = client.post(
            f"{SIDECAR_URL}/add",
            json={
                "text": text,
                "user_id": USER_ID,
                "agent_id": AGENT_ID,
                "metadata": {"source": source},
            },
            timeout=60.0,
        )
        r.raise_for_status()
        data = r.json()
        results = data.get("result", {}).get("results", [])
        return data if results else None
    except httpx.HTTPStatusError as e:
        log.warning(f"HTTP {e.response.status_code}: {e.response.text[:200]}")
        return None
    except Exception as e:
        log.warning(f"Error: {e}")
        return None


def extract_text_from_content(content) -> str:
    """Extract plain text from assistant content (may be JSON array of blocks)."""
    if isinstance(content, str):
        try:
            parsed = json.loads(content)
            if isinstance(parsed, list):
                return " ".join(
                    b.get("text", "") for b in parsed if b.get("type") == "text"
                )
        except (json.JSONDecodeError, TypeError):
            pass
        return content
    if isinstance(content, list):
        return " ".join(
            b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text"
        )
    return str(content)


def backfill_chiron(client: httpx.Client, dry_run: bool = False) -> int:
    """Extract conversations from sessions.db and send to memory."""
    if not SESSIONS_DB.exists():
        log.error(f"Sessions DB not found: {SESSIONS_DB}")
        return 0

    conn = sqlite3.connect(str(SESSIONS_DB))
    conn.row_factory = sqlite3.Row

    rows = conn.execute("""
        SELECT m.seq, m.role, m.content, m.created_at, s.nous_id
        FROM messages m
        JOIN sessions s ON m.session_id = s.id
        WHERE s.nous_id = 'chiron'
          AND m.role IN ('user', 'assistant')
          -- Include distilled messages too (summaries contain condensed facts)
        ORDER BY m.session_id, m.seq
    """).fetchall()
    conn.close()

    log.info(f"Found {len(rows)} user/assistant messages from Chiron sessions")

    # Group into conversational exchanges (user + following assistant response)
    chunks = []
    current_chunk = []
    current_len = 0

    for row in rows:
        text = extract_text_from_content(row["content"])
        if not text.strip():
            continue

        prefix = "User: " if row["role"] == "user" else "Assistant: "
        line = prefix + text.strip()

        if current_len + len(line) > MAX_CHUNK_CHARS and current_chunk:
            chunks.append("\n".join(current_chunk))
            current_chunk = []
            current_len = 0

        current_chunk.append(line)
        current_len += len(line)

    if current_chunk:
        chunks.append("\n".join(current_chunk))

    log.info(f"Grouped into {len(chunks)} chunks for processing")

    added = 0
    for i, chunk in enumerate(chunks):
        if dry_run:
            log.info(f"[DRY RUN] Chunk {i+1}/{len(chunks)}: {chunk[:100]}...")
            continue

        result = add_memory(client, chunk, "chiron-backfill")
        facts = result.get("result", {}).get("results", []) if result else []
        if facts:
            added += len(facts)
            fact_names = [f["memory"] for f in facts]
            log.info(f"Chunk {i+1}/{len(chunks)}: +{len(facts)} facts: {fact_names}")
        else:
            log.debug(f"Chunk {i+1}/{len(chunks)}: no new facts")

        # Small delay to avoid overwhelming Haiku
        time.sleep(0.5)

    return added


def backfill_claude_code(client: httpx.Client, dry_run: bool = False, max_files: int = 0) -> int:
    """Extract user messages from Claude Code JSONL transcripts."""
    if not CLAUDE_CODE_DIR.exists():
        log.error(f"Claude Code dir not found: {CLAUDE_CODE_DIR}")
        return 0

    transcripts = sorted(CLAUDE_CODE_DIR.glob("*.jsonl"), key=lambda p: p.stat().st_mtime)
    if max_files > 0:
        transcripts = transcripts[-max_files:]

    log.info(f"Processing {len(transcripts)} Claude Code transcripts")

    added = 0
    for path in transcripts:
        file_added = 0
        chunks = []
        current_chunk = []
        current_len = 0

        try:
            with open(path) as f:
                for line_num, line in enumerate(f):
                    try:
                        msg = json.loads(line)
                    except json.JSONDecodeError:
                        continue

                    # Extract user and assistant text messages
                    msg_type = msg.get("type", "")
                    message = msg.get("message", {})
                    role = message.get("role", msg_type)

                    if role in ("human", "user"):
                        content = message.get("content", "")
                        text = extract_text_from_content(content)
                        if text.strip() and len(text) > 20:
                            prefix = "User: "
                            entry = prefix + text.strip()

                            if current_len + len(entry) > MAX_CHUNK_CHARS and current_chunk:
                                chunks.append("\n".join(current_chunk))
                                current_chunk = []
                                current_len = 0

                            current_chunk.append(entry)
                            current_len += len(entry)

                    elif role == "assistant":
                        content = message.get("content", "")
                        text = extract_text_from_content(content)
                        if text.strip() and len(text) > 20:
                            prefix = "Assistant: "
                            entry = prefix + text.strip()[:1000]  # Cap assistant responses

                            if current_len + len(entry) > MAX_CHUNK_CHARS and current_chunk:
                                chunks.append("\n".join(current_chunk))
                                current_chunk = []
                                current_len = 0

                            current_chunk.append(entry)
                            current_len += len(entry)

        except Exception as e:
            log.warning(f"Error reading {path.name}: {e}")
            continue

        if current_chunk:
            chunks.append("\n".join(current_chunk))

        log.info(f"{path.name[:12]}...: {len(chunks)} chunks")

        for i, chunk in enumerate(chunks):
            if dry_run:
                log.info(f"  [DRY RUN] Chunk {i+1}/{len(chunks)}: {chunk[:100]}...")
                continue

            result = add_memory(client, chunk, f"claude-code:{path.stem[:12]}")
            facts = result.get("result", {}).get("results", []) if result else []
            if facts:
                file_added += len(facts)
                fact_names = [f["memory"] for f in facts]
                log.info(f"  Chunk {i+1}/{len(chunks)}: +{len(facts)} facts: {fact_names}")

            time.sleep(0.5)

        added += file_added

    return added


def main():
    parser = argparse.ArgumentParser(description="Backfill Chiron memory from conversations")
    parser.add_argument("--source", choices=["chiron", "claude-code", "all"], default="all")
    parser.add_argument("--dry-run", action="store_true", help="Preview without sending")
    parser.add_argument("--max-files", type=int, default=0, help="Limit Claude Code files (0=all)")
    args = parser.parse_args()

    # Verify sidecar is running
    try:
        r = httpx.get(f"{SIDECAR_URL}/health", timeout=5.0)
        health = r.json()
        if not health.get("ok"):
            log.error(f"Sidecar unhealthy: {health}")
            sys.exit(1)
        log.info("Sidecar healthy")
    except Exception as e:
        log.error(f"Cannot reach sidecar at {SIDECAR_URL}: {e}")
        sys.exit(1)

    client = httpx.Client()
    total = 0

    if args.source in ("chiron", "all"):
        log.info("=== Backfilling from Chiron sessions ===")
        total += backfill_chiron(client, dry_run=args.dry_run)

    if args.source in ("claude-code", "all"):
        log.info("=== Backfilling from Claude Code transcripts ===")
        total += backfill_claude_code(client, dry_run=args.dry_run, max_files=args.max_files)

    log.info(f"=== Done. Total new facts: {total} ===")
    client.close()


if __name__ == "__main__":
    main()
