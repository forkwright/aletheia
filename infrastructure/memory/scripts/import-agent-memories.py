#!/usr/bin/env python3
# Import agent workspace memory files into Mem0

import json
import sys
import time
from pathlib import Path

import httpx

SIDECAR_URL = "http://127.0.0.1:8230"
USER_ID = "default"
NOUS_DIR = Path("/mnt/ssd/aletheia/nous")
AGENTS = ["syn", "chiron", "eiron", "demiurge", "syl", "arbor", "akron"]
TIMEOUT = 120.0

client = httpx.Client(timeout=TIMEOUT)


def import_file(agent_id: str, file_path: Path) -> bool:
    content = file_path.read_text(encoding="utf-8", errors="replace")
    if len(content.strip()) < 50:
        return False

    try:
        resp = client.post(
            f"{SIDECAR_URL}/add",
            json={
                "text": content[:50000],
                "user_id": USER_ID,
                "agent_id": agent_id,
                "metadata": {
                    "source": "workspace_memory",
                    "file": str(file_path.name),
                    "agent": agent_id,
                },
            },
        )
        resp.raise_for_status()
        return True
    except Exception as e:
        print(f"  ERROR: {file_path.name}: {e}", file=sys.stderr)
        return False


def import_mcp_memory() -> int:
    mcp_path = Path("/mnt/ssd/aletheia/shared/memory/mcp-memory.json")
    if not mcp_path.exists():
        print("mcp-memory.json not found, skipping")
        return 0

    data = json.loads(mcp_path.read_text())
    entities = data.get("entities", [])
    relations = data.get("relations", [])

    imported = 0
    for entity in entities:
        name = entity.get("name", "")
        entity_type = entity.get("entityType", "")
        observations = entity.get("observations", [])
        if not name:
            continue

        text = f"{name} ({entity_type}): {'; '.join(observations)}"
        try:
            resp = client.post(
                f"{SIDECAR_URL}/add",
                json={
                    "text": text,
                    "user_id": USER_ID,
                    "metadata": {"source": "mcp-memory", "entity_type": entity_type},
                },
            )
            resp.raise_for_status()
            imported += 1
        except Exception as e:
            print(f"  ERROR entity {name}: {e}", file=sys.stderr)

    for rel in relations:
        from_entity = rel.get("from", "")
        to_entity = rel.get("to", "")
        rel_type = rel.get("relationType", "")
        if not from_entity or not to_entity:
            continue

        text = f"{from_entity} {rel_type} {to_entity}"
        try:
            resp = client.post(
                f"{SIDECAR_URL}/add",
                json={
                    "text": text,
                    "user_id": USER_ID,
                    "metadata": {"source": "mcp-memory", "relation_type": rel_type},
                },
            )
            resp.raise_for_status()
            imported += 1
        except Exception as e:
            print(f"  ERROR relation {from_entity}->{to_entity}: {e}", file=sys.stderr)

    return imported


def main():
    total = 0
    errors = 0

    # Import agent workspace memories
    for agent in AGENTS:
        mem_dir = NOUS_DIR / agent / "memory"
        if not mem_dir.exists():
            print(f"{agent}: no memory directory")
            continue

        files = sorted(mem_dir.glob("*.md"))
        print(f"{agent}: {len(files)} files")

        for f in files:
            ok = import_file(agent, f)
            if ok:
                total += 1
                print(f"  + {f.name}")
            else:
                errors += 1
            time.sleep(0.5)

    # Import mcp-memory.json
    print("\n--- mcp-memory.json ---")
    mcp_count = import_mcp_memory()
    total += mcp_count
    print(f"mcp-memory: {mcp_count} entities/relations imported")

    print(f"\nDone: {total} imported, {errors} skipped/errors")


if __name__ == "__main__":
    main()
