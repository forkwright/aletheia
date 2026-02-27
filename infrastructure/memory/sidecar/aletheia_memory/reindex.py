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
from collections.abc import Callable
from typing import Any

from qdrant_client import QdrantClient
from qdrant_client.models import Distance, PointStruct, Record, VectorParams

from .config import VOYAGE_DIMS, VOYAGE_MODEL

log = logging.getLogger("aletheia.memory.reindex")

QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
COLLECTION = "aletheia_memories"

EmbedFn = Callable[[str], list[float]]


def get_embedder() -> tuple[EmbedFn | None, int]:
    """Get the configured embedder."""
    voyage_key = os.environ.get("VOYAGE_API_KEY", "")
    if voyage_key:
        from openai import OpenAI
        client = OpenAI(api_key=voyage_key, base_url="https://api.voyageai.com/v1")
        def embed(text: str) -> list[float]:
            r = client.embeddings.create(input=[text.replace("\n", " ")], model=VOYAGE_MODEL)
            return r.data[0].embedding
        return embed, VOYAGE_DIMS
    else:
        try:
            fastembed: Any = __import__("fastembed")
            model: Any = fastembed.TextEmbedding("BAAI/bge-small-en-v1.5")
            def embed(text: str) -> list[float]:
                result: list[float] = list(next(iter(model.embed([text]))))
                return result
            return embed, 384
        except ImportError:
            log.error("No embedder available. Install fastembed: pip install fastembed")
            return None, 0


def reindex_zero_vectors(dry_run: bool = False) -> int:
    """Find points with zero/missing vectors and recompute embeddings."""
    client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)

    collections: list[str] = [c.name for c in client.get_collections().collections]
    if COLLECTION not in collections:
        log.info("Collection %s doesn't exist. Nothing to reindex.", COLLECTION)
        return 0

    info = client.get_collection(COLLECTION)
    total_points: int | None = info.points_count
    total_vectors: int = info.indexed_vectors_count or 0
    log.info("Collection: %s points, %d vectors", total_points, total_vectors)

    if total_points is not None and total_vectors >= total_points and total_vectors > 0:
        log.info("All points have vectors. Nothing to reindex.")
        return 0

    embed_fn, dims = get_embedder()
    if not embed_fn:
        return 0

    if total_vectors == 0 and total_points is not None and total_points > 0:
        log.info("Recreating collection with %d-dim vectors (preserving points)", dims)

        all_points: list[Record] = []
        offset: Any = None
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

        log.info("Scrolled %d points", len(all_points))

        if dry_run:
            log.info("[DRY RUN] Would re-embed %d points", len(all_points))
            return len(all_points)

        client.recreate_collection(
            collection_name=COLLECTION,
            vectors_config=VectorParams(size=dims, distance=Distance.COSINE),
        )

        batch: list[PointStruct] = []
        reindexed = 0
        for point in all_points:
            payload: dict[str, Any] = point.payload or {}
            text: str = str(payload.get("memory", payload.get("data", payload.get("text", ""))))
            if not text:
                log.debug("Point %s: no text field, skipping", point.id)
                continue

            try:
                vector: list[float] = embed_fn(text)
                batch.append(PointStruct(
                    id=point.id,
                    vector=vector,
                    payload=payload,
                ))
                reindexed += 1

                if len(batch) >= 50:
                    client.upsert(collection_name=COLLECTION, points=batch)
                    log.info("Re-embedded %d/%d points", reindexed, len(all_points))
                    batch = []
            except Exception as e:
                log.warning("Point %s: embedding failed: %s", point.id, e)

        if batch:
            client.upsert(collection_name=COLLECTION, points=batch)

        log.info("Reindex complete: %d/%d points re-embedded", reindexed, len(all_points))
        return reindexed

    return 0


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    dry = "--dry-run" in sys.argv
    n = reindex_zero_vectors(dry_run=dry)
    print(f"Reindexed: {n}")
