#!/usr/bin/env python3
# Import facts.jsonl into Mem0 with batching and rate limit handling

import json
import sys
import time
from pathlib import Path

import httpx

SIDECAR_URL = "http://127.0.0.1:8230"
USER_ID = "default"
FACTS_FILE = Path("/mnt/ssd/aletheia/shared/memory/facts.jsonl")
TIMEOUT = 300.0
DELAY = 3.0
MAX_RETRIES = 3
RETRY_DELAY = 15.0

client = httpx.Client(timeout=TIMEOUT)


def import_fact(text: str, metadata: dict, idx: int) -> bool:
    for attempt in range(MAX_RETRIES):
        try:
            resp = client.post(
                f"{SIDECAR_URL}/add",
                json={
                    "text": text,
                    "user_id": USER_ID,
                    "metadata": metadata,
                },
            )
            resp.raise_for_status()
            return True
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                print(f"  RETRY fact {idx} (attempt {attempt + 1}): {e}")
                time.sleep(RETRY_DELAY)
            else:
                print(f"  ERROR fact {idx}: {e}", file=sys.stderr)
                return False
    return False


def main():
    if not FACTS_FILE.exists():
        print(f"File not found: {FACTS_FILE}")
        sys.exit(1)

    facts = []
    with open(FACTS_FILE) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                facts.append(json.loads(line))
            except json.JSONDecodeError:
                continue

    print(f"Loaded {len(facts)} facts from {FACTS_FILE}")

    imported = 0
    errors = 0

    for i, fact in enumerate(facts):
        subject = fact.get("subject", "")
        predicate = fact.get("predicate", "")
        obj = fact.get("object", "")
        text = f"{subject} {predicate} {obj}".strip()
        if not text:
            continue

        metadata = {
            "source": "facts.jsonl",
            "confidence": fact.get("confidence", 1.0),
        }
        if fact.get("domain"):
            metadata["domain"] = fact["domain"]
        if fact.get("agent"):
            metadata["original_agent"] = fact["agent"]

        ok = import_fact(text, metadata, i)
        if ok:
            imported += 1
            if imported % 10 == 0:
                print(f"  Progress: {imported}/{len(facts)} imported")
        else:
            errors += 1

        time.sleep(DELAY)

    print(f"\nDone: {imported} facts imported, {errors} errors")


if __name__ == "__main__":
    main()
