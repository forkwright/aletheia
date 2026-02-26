#!/usr/bin/env python3
# Identify and optionally remove Qdrant points missing required metadata fields

import argparse
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

from qdrant_client import QdrantClient
from qdrant_client.models import PointIdsList

REQUIRED_FIELDS = ("session_id", "source", "agent_id")
BATCH_SIZE = 200


def scroll_all_points(client: QdrantClient, collection: str) -> list:
    points = []
    offset = None
    while True:
        result, next_offset = client.scroll(
            collection_name=collection,
            limit=BATCH_SIZE,
            offset=offset,
            with_payload=True,
            with_vectors=False,
        )
        points.extend(result)
        if next_offset is None:
            break
        offset = next_offset
        print(f"  Scrolled {len(points)} points...", flush=True)
    return points


def is_orphan(point) -> bool:
    payload = point.payload or {}
    for field in REQUIRED_FIELDS:
        value = payload.get(field)
        if not value:
            return True
    return False


def main():
    parser = argparse.ArgumentParser(
        description="Detect and optionally purge Qdrant points missing required metadata."
    )
    parser.add_argument("--host", default="localhost", help="Qdrant host (default: localhost)")
    parser.add_argument("--port", type=int, default=6333, help="Qdrant port (default: 6333)")
    parser.add_argument(
        "--collection",
        default="aletheia_memories",
        help="Qdrant collection name (default: aletheia_memories)",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Delete orphans (default is dry-run — no deletions)",
    )
    args = parser.parse_args()

    client = QdrantClient(host=args.host, port=args.port)

    print(f"Connecting to Qdrant at {args.host}:{args.port}")
    print(f"Collection: {args.collection}")
    print(f"Mode: {'EXECUTE (will delete)' if args.execute else 'DRY-RUN (no deletions)'}")
    print()

    print("Scrolling all points...")
    all_points = scroll_all_points(client, args.collection)
    total_scanned = len(all_points)
    print(f"Total points scanned: {total_scanned}")
    print()

    orphans = [p for p in all_points if is_orphan(p)]
    orphan_count = len(orphans)

    source_dist: Counter = Counter()
    for p in orphans:
        payload = p.payload or {}
        source = payload.get("source") or "<missing>"
        source_dist[source] += 1

    print(f"Orphans found: {orphan_count} / {total_scanned}")
    print()

    print("Source value distribution among orphans:")
    if source_dist:
        for source, count in source_dist.most_common():
            print(f"  {source!r}: {count}")
    else:
        print("  (none)")
    print()

    print("Sample orphan IDs (up to 5):")
    for p in orphans[:5]:
        payload = p.payload or {}
        missing = [f for f in REQUIRED_FIELDS if not (payload.get(f))]
        print(f"  {p.id}  (missing: {', '.join(missing)})")
    print()

    if not args.execute:
        print("DRY-RUN complete. Use --execute to delete orphans.")
        return

    if orphan_count == 0:
        print("No orphans to delete.")
        return

    # Log orphan IDs to file before deletion
    log_path = Path(__file__).parent / f"purged-orphans-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}.log"
    orphan_ids = [str(p.id) for p in orphans]
    with open(log_path, "w") as f:
        json.dump(
            {
                "collection": args.collection,
                "timestamp": datetime.now(timezone.utc).isoformat(),
                "orphan_count": orphan_count,
                "orphan_ids": orphan_ids,
            },
            f,
            indent=2,
        )
    print(f"Orphan IDs logged to: {log_path}")

    # Delete
    client.delete(
        collection_name=args.collection,
        points_selector=PointIdsList(points=orphan_ids),
    )
    print(f"Deleted {orphan_count} orphan points.")
    print()
    print(f"Summary: scanned={total_scanned}, orphans_found={orphan_count}, orphans_deleted={orphan_count}")


if __name__ == "__main__":
    main()
