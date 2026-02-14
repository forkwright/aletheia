#!/usr/bin/env python3
# Import mcp-memory.json entities and observations into Mem0

import json
import sys
import time

import httpx

SIDECAR_URL = "http://127.0.0.1:8230"
USER_ID = "ck"
MCP_FILE = "/mnt/ssd/aletheia/shared/memory/mcp-memory.json"
TIMEOUT = 300.0
DELAY = 3.0
MAX_RETRIES = 3

client = httpx.Client(timeout=TIMEOUT)


def send(text: str, metadata: dict, idx: int) -> bool:
    for attempt in range(MAX_RETRIES):
        try:
            resp = client.post(
                f"{SIDECAR_URL}/add",
                json={"text": text, "user_id": USER_ID, "metadata": metadata},
            )
            resp.raise_for_status()
            return True
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                print(f"  RETRY {idx}: {e}")
                time.sleep(10)
            else:
                print(f"  ERROR {idx}: {e}", file=sys.stderr)
                return False
    return False


def main():
    with open(MCP_FILE) as f:
        lines = [json.loads(line) for line in f if line.strip()]

    entities = [l for l in lines if l.get("type") == "entity"]
    relations = [l for l in lines if l.get("type") == "relation"]

    print(f"Loaded {len(entities)} entities, {len(relations)} relations")

    imported = 0
    errors = 0

    for i, entity in enumerate(entities):
        name = entity.get("name", "")
        etype = entity.get("entityType", "")
        observations = entity.get("observations", [])
        if not name or not observations:
            continue

        text = f"{name} ({etype}): " + ". ".join(observations)
        ok = send(text, {"source": "mcp-memory", "entity": name, "type": etype}, i)
        if ok:
            imported += 1
        else:
            errors += 1
        time.sleep(DELAY)

    for i, rel in enumerate(relations):
        frm = rel.get("from", "")
        to = rel.get("to", "")
        rtype = rel.get("relationType", "")
        if not frm or not to:
            continue

        text = f"{frm} {rtype} {to}"
        ok = send(text, {"source": "mcp-memory", "relation": rtype}, len(entities) + i)
        if ok:
            imported += 1
        else:
            errors += 1
        time.sleep(DELAY)

    print(f"\nDone: {imported} imported, {errors} errors")


if __name__ == "__main__":
    main()
