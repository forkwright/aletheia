#!/usr/bin/env python3
"""Fix zero-vector points in Qdrant by re-computing embeddings.

Run standalone:
    python -m aletheia_memory.reindex

Or import and call:
    from aletheia_memory.reindex import reindex_zero_vectors
    reindex_zero_vectors()
"""

import logging
import os
import sys

from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct, VectorParams, Distance

log = logging.getLogger("aletheia.memory.reindex")

QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
COLLECTION = "aletheia_memories"


def get_embedder():
    """Get the configured embedder."""
    voyage_key = os.environ.get("VOYAGE_API_KEY", "")
    if voyage_key:
        from openai import OpenAI
        client = OpenAI(api_key=voyage_key, base_url="https://api.voyageai.com/v1")
        def embed(text):
            r = client.embeddings.create(input=[text.replace("\n", " ")], model="voyage-3-large")
            return r.data[0].embedding
        return embed, 1024
    else:
        try:
            from fastembed import TextEmbedding
            model = TextEmbedding("BAAI/bge-small-en-v1.5")
            def embed(text):
                return list(model.embed([text]))[0].tolist()
            return embed, 384
        except ImportError:
            log.error("No embedder available. Install fastembed: pip install fastembed")
            return None, 0


def reindex_zero_vectors(dry_run=False):
    """Find points with zero/missing vectors and recompute embeddings."""
    client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

    # Check collection exists
    collections = [c.name for c in client.get_collections().collections]
    if COLLECTION not in collections:
        log.info(f"Collection {COLLECTION} doesn't exist. Nothing to reindex.")
        return 0

    info = client.get_collection(COLLECTION)
    total_points = info.points_count
    total_vectors = info.indexed_vectors_count or 0
    log.info(f"Collection: {total_points} points, {total_vectors} vectors")

    if total_vectors >= total_points and total_vectors > 0:
        log.info("All points have vectors. Nothing to reindex.")
        return 0

    embed_fn, dims = get_embedder()
    if not embed_fn:
        return 0

    # Ensure collection has correct vector config
    # If vectors_count is 0, the collection may need vector params set
    if total_vectors == 0 and total_points > 0:
        log.info(f"Recreating collection with {dims}-dim vectors (preserving points)")
        # We need to scroll all points, recreate collection, and re-insert
        # Qdrant doesn't allow adding vector config after creation

        all_points = []
        offset = None
        while True:
            result = client.scroll(
                collection_name=COLLECTION,
                limit=100,
                offset=offset,
                with_payload=True,
                with_vectors=False,
            )
            points, next_offset = result
            all_points.extend(points)
            if next_offset is None:
                break
            offset = next_offset

        log.info(f"Scrolled {len(all_points)} points")

        if dry_run:
            log.info(f"[DRY RUN] Would re-embed {len(all_points)} points")
            return len(all_points)

        # Recreate collection
        client.recreate_collection(
            collection_name=COLLECTION,
            vectors_config=VectorParams(size=dims, distance=Distance.COSINE),
        )

        # Re-insert with embeddings
        batch = []
        reindexed = 0
        for point in all_points:
            payload = point.payload or {}
            text = payload.get("memory", payload.get("data", payload.get("text", "")))
            if not text:
                log.debug(f"Point {point.id}: no text field, skipping")
                continue

            try:
                vector = embed_fn(text)
                batch.append(PointStruct(
                    id=point.id,
                    vector=vector,
                    payload=payload,
                ))
                reindexed += 1

                if len(batch) >= 50:
                    client.upsert(collection_name=COLLECTION, points=batch)
                    log.info(f"Re-embedded {reindexed}/{len(all_points)} points")
                    batch = []
            except Exception as e:
                log.warning(f"Point {point.id}: embedding failed: {e}")

        if batch:
            client.upsert(collection_name=COLLECTION, points=batch)

        log.info(f"Reindex complete: {reindexed}/{len(all_points)} points re-embedded")
        return reindexed

    return 0


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    dry = "--dry-run" in sys.argv
    n = reindex_zero_vectors(dry_run=dry)
    print(f"Reindexed: {n}")
