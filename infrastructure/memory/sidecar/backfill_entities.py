#!/usr/bin/env python3
"""Backfill entity resolution on existing Qdrant corpus.

Scans all points in aletheia_memories, extracts entities from the text,
resolves them via the entity resolver (alias table + fuzzy matching),
and updates each point's payload with resolved entities.

Also runs graph dedup (merge_duplicates + cleanup_orphans) at the end.

Usage:
    python backfill_entities.py              # Full run
    python backfill_entities.py --dry-run    # Show stats without writing
    python backfill_entities.py --batch 200  # Custom batch size
"""

import argparse
import logging
import re
import sys
import time
from collections import Counter

# Must run from sidecar venv
try:
    from qdrant_client import QdrantClient
    from qdrant_client.models import PointStruct, SetPayloadOperation, SetPayload, PointIdsList
except ImportError:
    print("Run from sidecar venv: .venv/bin/python backfill_entities.py")
    sys.exit(1)

sys.path.insert(0, str(__import__("pathlib").Path(__file__).parent))
from aletheia_memory.entity_resolver import (
    resolve_entity,
    is_valid_entity,
    get_canonical_entities,
    merge_duplicate_entities,
    cleanup_orphan_entities,
)

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("backfill_entities")

QDRANT_HOST = "localhost"
QDRANT_PORT = 6333
COLLECTION = "aletheia_memories"
SCROLL_BATCH = 100


def extract_entities(text: str) -> list[str]:
    """Same heuristic as routes.py _extract_entities."""
    entities = []
    for match in re.finditer(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b", text):
        entities.append(match.group())
    for match in re.finditer(r"\b[a-z]+[-_][a-z]+(?:[-_][a-z]+)*\b", text):
        entities.append(match.group())
    for match in re.finditer(r'"([^"]+)"', text):
        entities.append(match.group(1))
    return list(set(entities))[:10]


def run(dry_run: bool = False, batch_size: int = SCROLL_BATCH):
    client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

    # Get collection info
    info = client.get_collection(COLLECTION)
    total = info.points_count
    log.info(f"Collection has {total} points")

    # Fetch canonical entities for resolution
    canonical_list = get_canonical_entities()
    log.info(f"Canonical entities in registry: {len(canonical_list)}")

    offset = None
    processed = 0
    updated = 0
    already_has = 0
    no_entities = 0
    entity_counts = Counter()
    all_resolved = Counter()

    while True:
        kwargs = {
            "collection_name": COLLECTION,
            "limit": batch_size,
            "with_payload": True,
            "with_vectors": False,
        }
        if offset:
            kwargs["offset"] = offset

        points, next_offset = client.scroll(**kwargs)

        if not points:
            break

        batch_updates = []  # (point_id, entities_payload)

        for point in points:
            processed += 1
            payload = point.payload or {}

            # Skip if already has entities
            if payload.get("entities"):
                already_has += 1
                continue

            # Get text content
            text = payload.get("data") or payload.get("memory") or ""
            if not text or len(text) < 20:
                no_entities += 1
                continue

            # Extract raw entities
            raw_entities = extract_entities(text)
            if not raw_entities:
                no_entities += 1
                continue

            # Resolve through entity resolver
            resolved = []
            for name in raw_entities:
                canonical = resolve_entity(name, canonical_list)
                if canonical:
                    resolved.append(canonical)
                    all_resolved[canonical] += 1

            resolved = list(set(resolved))[:10]
            if not resolved:
                no_entities += 1
                continue

            entity_counts[len(resolved)] += 1
            batch_updates.append((point.id, resolved))

        # Apply updates
        if batch_updates and not dry_run:
            for point_id, entities in batch_updates:
                client.set_payload(
                    collection_name=COLLECTION,
                    payload={"entities": entities},
                    points=[point_id],
                )
            updated += len(batch_updates)
        elif batch_updates:
            updated += len(batch_updates)

        if not next_offset:
            break
        offset = next_offset

        if processed % 500 == 0:
            log.info(f"  ... {processed}/{total} processed, {updated} to update")

    # Summary
    log.info(f"\n{'DRY RUN — ' if dry_run else ''}Backfill complete:")
    log.info(f"  Total points:    {processed}")
    log.info(f"  Already tagged:  {already_has}")
    log.info(f"  Updated:         {updated}")
    log.info(f"  No entities:     {no_entities}")
    log.info(f"\n  Entity count distribution:")
    for count in sorted(entity_counts.keys()):
        log.info(f"    {count} entities: {entity_counts[count]} points")
    log.info(f"\n  Top 25 resolved entities:")
    for name, count in all_resolved.most_common(25):
        log.info(f"    {name}: {count}")

    # Graph dedup
    if not dry_run:
        log.info("\nRunning graph deduplication...")
        try:
            merge_result = merge_duplicate_entities(dry_run=False)
            log.info(f"  Merged {merge_result.get('duplicates_merged', 0)} duplicate groups")
        except Exception as e:
            log.warning(f"  Merge failed (non-fatal): {e}")

        try:
            orphan_result = cleanup_orphan_entities(dry_run=False)
            log.info(f"  Cleaned {orphan_result.get('orphans_deleted', 0)} orphan entities")
        except Exception as e:
            log.warning(f"  Orphan cleanup failed (non-fatal): {e}")
    else:
        log.info("\nSkipping graph dedup (dry run)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Backfill entity resolution on Qdrant corpus")
    parser.add_argument("--dry-run", action="store_true", help="Preview without writing")
    parser.add_argument("--batch", type=int, default=SCROLL_BATCH, help="Scroll batch size")
    args = parser.parse_args()
    run(dry_run=args.dry_run, batch_size=args.batch)
