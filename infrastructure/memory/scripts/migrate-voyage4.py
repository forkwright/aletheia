#!/usr/bin/env python3
"""Migrate Qdrant embeddings from voyage-3-large to voyage-4-large.

Scrolls every point in aletheia_memories, re-embeds the text payload
with voyage-4-large, and upserts in-place. Payloads preserved exactly.

Usage:
    # From sidecar venv (has qdrant-client + openai):
    cd ~/.aletheia/infrastructure/memory/sidecar
    .venv/bin/python ../scripts/migrate-voyage4.py --dry-run
    .venv/bin/python ../scripts/migrate-voyage4.py

    # With explicit model override:
    VOYAGE_MODEL=voyage-4-large .venv/bin/python ../scripts/migrate-voyage4.py

Environment:
    VOYAGE_API_KEY  — required
    QDRANT_HOST     — default localhost
    QDRANT_PORT     — default 6333
    VOYAGE_MODEL    — default voyage-4-large

Notes:
    - Dimensions stay at 1024 (both models default to 1024)
    - Voyage-4 series has shared embedding space, so future queries
      with voyage-4 or voyage-4-lite are compatible
    - ~121K tokens for 2,425 memories ≈ free tier
    - Rate limit: Voyage allows 120K tokens/min for voyage-4-large
"""

import argparse
import logging
import os
import sys
import time
from datetime import datetime, timezone

from openai import OpenAI
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger("migrate-voyage4")

QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
COLLECTION = "aletheia_memories"
TARGET_MODEL = os.environ.get("VOYAGE_MODEL", "voyage-4-large")
BATCH_SIZE = 50  # Voyage API accepts up to 128, but 50 keeps responses manageable
UPSERT_BATCH = 100


def get_text(payload: dict) -> str:
    """Extract embeddable text from a point's payload."""
    # Mem0 stores in 'memory' (short) and 'data' (full)
    return (
        payload.get("data")
        or payload.get("memory")
        or payload.get("text")
        or ""
    ).strip()


def embed_batch(client: OpenAI, texts: list[str], model: str) -> list[list[float]]:
    """Embed a batch of texts via Voyage's OpenAI-compatible endpoint."""
    # Replace newlines per Voyage best practice
    cleaned = [t.replace("\n", " ")[:8000] for t in texts]  # 8K char safety limit
    response = client.embeddings.create(
        input=cleaned,
        model=model,
    )
    return [d.embedding for d in response.data]


def migrate(dry_run: bool = False, verify: bool = True):
    """Re-embed all points with the target model."""
    voyage_key = os.environ.get("VOYAGE_API_KEY")
    if not voyage_key:
        log.error("VOYAGE_API_KEY not set")
        sys.exit(1)

    voyage = OpenAI(api_key=voyage_key, base_url="https://api.voyageai.com/v1")
    qdrant = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

    # Check collection
    collections = [c.name for c in qdrant.get_collections().collections]
    if COLLECTION not in collections:
        log.error(f"Collection {COLLECTION} not found")
        sys.exit(1)

    info = qdrant.get_collection(COLLECTION)
    total = info.points_count
    indexed = info.indexed_vectors_count or 0
    log.info(f"Collection: {total} points, {indexed} indexed vectors")

    if dry_run:
        # Sample 3 points to show what would happen
        sample, _ = qdrant.scroll(COLLECTION, limit=3, with_payload=True, with_vectors=False)
        for p in sample:
            text = get_text(p.payload or {})
            log.info(f"  Point {p.id}: {len(text)} chars — {text[:80]}...")
        log.info(f"[DRY RUN] Would re-embed {total} points with {TARGET_MODEL}")
        return

    # Scroll all points
    all_points = []
    offset = None
    while True:
        points, next_offset = qdrant.scroll(
            COLLECTION,
            limit=100,
            offset=offset,
            with_payload=True,
            with_vectors=False,
        )
        all_points.extend(points)
        if next_offset is None:
            break
        offset = next_offset

    log.info(f"Scrolled {len(all_points)} points")

    # Filter to embeddable
    embeddable = [(p, get_text(p.payload or {})) for p in all_points]
    embeddable = [(p, t) for p, t in embeddable if t]
    skipped = len(all_points) - len(embeddable)
    if skipped:
        log.warning(f"Skipping {skipped} points with no text payload")

    log.info(f"Re-embedding {len(embeddable)} points with {TARGET_MODEL}...")

    # Process in batches
    total_tokens = 0
    upsert_buffer: list[PointStruct] = []
    upserted = 0
    errors = 0

    for i in range(0, len(embeddable), BATCH_SIZE):
        batch = embeddable[i : i + BATCH_SIZE]
        texts = [t for _, t in batch]

        try:
            vectors = embed_batch(voyage, texts, TARGET_MODEL)

            for (point, _text), vector in zip(batch, vectors):
                upsert_buffer.append(PointStruct(
                    id=point.id,
                    vector=vector,
                    payload=point.payload,
                ))

            # Flush upsert buffer
            if len(upsert_buffer) >= UPSERT_BATCH:
                qdrant.upsert(collection_name=COLLECTION, points=upsert_buffer)
                upserted += len(upsert_buffer)
                upsert_buffer = []

            # Estimate tokens (rough: 1 token ≈ 4 chars)
            batch_chars = sum(len(t) for t in texts)
            total_tokens += batch_chars // 4

            log.info(
                f"  Embedded {min(i + BATCH_SIZE, len(embeddable))}/{len(embeddable)} "
                f"(~{total_tokens:,} tokens)"
            )

        except Exception as e:
            log.error(f"  Batch {i}-{i + BATCH_SIZE} failed: {e}")
            errors += len(batch)
            # Brief pause on error (rate limit)
            time.sleep(2)

    # Flush remaining
    if upsert_buffer:
        qdrant.upsert(collection_name=COLLECTION, points=upsert_buffer)
        upserted += len(upsert_buffer)

    log.info(f"Migration complete: {upserted} upserted, {errors} errors, ~{total_tokens:,} tokens")

    # Verify: spot-check a few embeddings
    if verify and upserted > 0:
        log.info("Verifying migration...")
        sample, _ = qdrant.scroll(COLLECTION, limit=3, with_payload=True, with_vectors=True)
        for p in sample:
            vec = p.vector
            if isinstance(vec, list):
                dim = len(vec)
                norm = sum(v * v for v in vec) ** 0.5
                log.info(f"  Point {p.id}: {dim}d, norm={norm:.4f}")
                if dim != 1024:
                    log.error(f"  DIMENSION MISMATCH: expected 1024, got {dim}")
            else:
                log.warning(f"  Point {p.id}: unexpected vector type {type(vec)}")

        # Test retrieval quality
        test_text = "What material does Acme use?"
        test_vec = embed_batch(voyage, [test_text], TARGET_MODEL)[0]
        results = qdrant.query_points(
            collection_name=COLLECTION,
            query=test_vec,
            limit=3,
            with_payload=True,
        )
        log.info(f"  Test query: '{test_text}'")
        for r in results.points:
            score = r.score
            text = get_text(r.payload or {})[:100]
            log.info(f"    score={score:.4f}: {text}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Migrate embeddings to voyage-4-large")
    parser.add_argument("--dry-run", action="store_true", help="Show plan without executing")
    parser.add_argument("--no-verify", action="store_true", help="Skip post-migration verification")
    args = parser.parse_args()

    migrate(dry_run=args.dry_run, verify=not args.no_verify)
