#!/usr/bin/env python3
# Import agent session JSONL transcripts into Mem0 (Tier 2 optimized)

import json
import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import httpx

SIDECAR_URL = "http://127.0.0.1:8230"
USER_ID = "default"
SESSIONS_ROOT = Path(os.environ.get("SESSIONS_ROOT", "/var/lib/aletheia/agents"))
CHUNK_SIZE = 12000
TIMEOUT = 300.0
WORKERS = 12
DELAY_BETWEEN_SUBMITS = 0.1
MAX_RETRIES = 3
RETRY_DELAY = 10.0

AGENTS_MAP = {
    "main": "syn",
    "syn": "syn",
    "chiron": "chiron",
    "eiron": "eiron",
    "demiurge": "demiurge",
    "syl": "syl",
    "arbor": "arbor",
    "akron": "akron",
}


def extract_messages(session_path: Path) -> list[dict]:
    messages = []
    with open(session_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue

            entry_type = entry.get("type", "")
            if entry_type == "compaction":
                summary = entry.get("summary", "")
                if summary and len(summary) > 50:
                    messages.append({
                        "role": "compaction",
                        "content": summary,
                        "ts": entry.get("timestamp", ""),
                    })
                continue

            if entry_type != "message":
                continue

            msg = entry.get("message", {})
            role = msg.get("role", "")
            if role not in ("user", "assistant"):
                continue

            content_raw = msg.get("content", "")
            if isinstance(content_raw, str):
                text = content_raw
            elif isinstance(content_raw, list):
                parts = []
                for block in content_raw:
                    if block.get("type") == "text":
                        parts.append(block.get("text", ""))
                text = "\n".join(parts)
            else:
                continue

            if not text or len(text.strip()) < 10:
                continue

            messages.append({
                "role": role,
                "content": text.strip(),
                "ts": entry.get("timestamp", ""),
            })

    return messages


def chunk_messages(messages: list[dict], chunk_size: int = CHUNK_SIZE) -> list[str]:
    chunks = []
    current = []
    current_len = 0

    for msg in messages:
        line = f"{msg['role']}: {msg['content']}"
        if current_len + len(line) > chunk_size and current:
            chunks.append("\n\n".join(current))
            current = []
            current_len = 0
        current.append(line)
        current_len += len(line)

    if current:
        chunks.append("\n\n".join(current))

    return chunks


def import_chunk(agent_id: str, text: str, session_id: str, chunk_idx: int) -> tuple[bool, int]:
    client = httpx.Client(timeout=TIMEOUT)
    try:
        for attempt in range(MAX_RETRIES):
            try:
                resp = client.post(
                    f"{SIDECAR_URL}/add",
                    json={
                        "text": text[:50000],
                        "user_id": USER_ID,
                        "agent_id": agent_id,
                        "metadata": {
                            "source": "session_import",
                            "session_id": session_id,
                            "chunk": chunk_idx,
                            "agent": agent_id,
                        },
                    },
                )
                resp.raise_for_status()
                return True, chunk_idx
            except Exception as e:
                err_str = str(e)
                if "429" in err_str or "rate" in err_str.lower():
                    wait = RETRY_DELAY * (attempt + 1)
                    print(f"  RATE LIMITED chunk {chunk_idx}, waiting {wait}s", flush=True)
                    time.sleep(wait)
                elif attempt < MAX_RETRIES - 1:
                    print(f"  RETRY chunk {chunk_idx} (attempt {attempt + 1}): {e}", flush=True)
                    time.sleep(RETRY_DELAY)
                else:
                    print(f"  ERROR chunk {chunk_idx}: {e}", file=sys.stderr, flush=True)
                    return False, chunk_idx
    finally:
        client.close()
    return False, chunk_idx


def main():
    total_chunks = 0
    total_errors = 0

    if not SESSIONS_ROOT.exists():
        print(f"Sessions root not found: {SESSIONS_ROOT}")
        sys.exit(1)

    for agent_dir in sorted(SESSIONS_ROOT.iterdir()):
        if not agent_dir.is_dir():
            continue

        agent_name = agent_dir.name
        agent_id = AGENTS_MAP.get(agent_name)
        if not agent_id:
            print(f"Skipping unknown agent: {agent_name}", flush=True)
            continue

        sessions_dir = agent_dir / "sessions"
        if not sessions_dir.exists():
            continue

        jsonl_files = sorted(sessions_dir.glob("*.jsonl"))
        if not jsonl_files:
            continue

        print(f"\n=== {agent_name} → agent_id:{agent_id} ({len(jsonl_files)} sessions) ===", flush=True)

        for session_file in jsonl_files:
            session_id = session_file.stem
            messages = extract_messages(session_file)
            if len(messages) < 3:
                continue

            chunks = chunk_messages(messages)
            print(f"  {session_id}: {len(messages)} msgs → {len(chunks)} chunks", flush=True)

            session_ok = 0
            session_err = 0
            t0 = time.monotonic()

            with ThreadPoolExecutor(max_workers=WORKERS) as pool:
                futures = []
                for i, chunk in enumerate(chunks):
                    futures.append(pool.submit(import_chunk, agent_id, chunk, session_id, i))
                    time.sleep(DELAY_BETWEEN_SUBMITS)

                for future in as_completed(futures):
                    ok, idx = future.result()
                    if ok:
                        session_ok += 1
                    else:
                        session_err += 1

            elapsed = time.monotonic() - t0
            rate = session_ok / elapsed if elapsed > 0 else 0
            total_chunks += session_ok
            total_errors += session_err
            print(f"    → {session_ok} OK, {session_err} errors ({elapsed:.1f}s, {rate:.1f} chunks/s)", flush=True)

    print(f"\nDone: {total_chunks} chunks imported, {total_errors} errors", flush=True)


if __name__ == "__main__":
    main()
