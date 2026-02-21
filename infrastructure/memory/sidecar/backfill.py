#!/usr/bin/env python3
"""Direct Qdrant backfill for curated knowledge.

Bypasses Mem0's /add endpoint (slow, buggy for bulk) and writes directly
to Qdrant with proper metadata so sidecar search still works.

Usage:
    python backfill.py                    # Backfill all agent workspaces
    python backfill.py --agent syn        # Backfill only syn's workspace
    python backfill.py --dry-run          # Show what would be embedded
    python backfill.py --source docs      # Backfill docs/specs only

Requires VOYAGE_API_KEY in environment (or pass --voyage-key).
"""

import argparse
import hashlib
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

try:
    import voyageai
    from qdrant_client import QdrantClient
    from qdrant_client.models import PointStruct, Filter, FieldCondition, MatchValue
except ImportError as e:
    print(f"Missing dependency: {e}")
    print("Run from the sidecar venv: .venv/bin/python backfill.py")
    sys.exit(1)


QDRANT_HOST = os.environ.get("QDRANT_HOST", "localhost")
QDRANT_PORT = int(os.environ.get("QDRANT_PORT", "6333"))
COLLECTION = "aletheia_memories"
EMBED_MODEL = "voyage-3-large"  # 1024-dim, must match existing collection
CHUNK_SIZE = 512  # tokens ~= words * 1.3, keep chunks focused
CHUNK_OVERLAP = 50  # overlap for context continuity
BATCH_SIZE = 64  # Voyage batch limit
USER_ID = os.environ.get("ALETHEIA_MEMORY_USER", "ck")

# Directories
ALETHEIA_ROOT = Path(__file__).resolve().parent.parent.parent.parent
NOUS_DIR = ALETHEIA_ROOT / "nous"
DOCS_DIR = ALETHEIA_ROOT / "docs"


def chunk_text(text: str, source_path: str, max_words: int = CHUNK_SIZE, overlap: int = CHUNK_OVERLAP) -> list[dict]:
    """Split text into overlapping chunks with metadata."""
    lines = text.strip().split("\n")
    chunks = []
    current_words: list[str] = []
    current_section = ""

    for line in lines:
        # Track markdown headers for section context
        if line.startswith("# "):
            current_section = line.strip("# ").strip()
        elif line.startswith("## "):
            current_section = line.strip("# ").strip()

        words = line.split()
        current_words.extend(words)

        if len(current_words) >= max_words:
            chunk_text = " ".join(current_words)
            chunks.append({
                "text": chunk_text,
                "section": current_section,
                "source": source_path,
            })
            # Keep overlap
            current_words = current_words[-overlap:]

    # Remaining words
    if current_words:
        chunk_text = " ".join(current_words)
        if len(chunk_text.strip()) > 20:  # Skip tiny remnants
            chunks.append({
                "text": chunk_text,
                "section": current_section,
                "source": source_path,
            })

    return chunks


def collect_agent_files(agent: str | None = None) -> list[tuple[str, str, str]]:
    """Collect markdown files from agent workspaces.
    
    Returns: list of (file_path, agent_id, source_type)
    """
    files = []
    
    if agent:
        agents = [agent]
    else:
        agents = [d.name for d in NOUS_DIR.iterdir() if d.is_dir() and not d.name.startswith(".")]

    for agent_name in sorted(agents):
        agent_dir = NOUS_DIR / agent_name
        if not agent_dir.exists():
            print(f"  [skip] Agent directory not found: {agent_dir}")
            continue

        # MEMORY.md
        mem_file = agent_dir / "MEMORY.md"
        if mem_file.exists():
            files.append((str(mem_file), agent_name, "memory"))

        # SOUL.md
        soul_file = agent_dir / "SOUL.md"
        if soul_file.exists():
            files.append((str(soul_file), agent_name, "identity"))

        # memory/*.md (daily logs + refs)
        mem_dir = agent_dir / "memory"
        if mem_dir.exists():
            for f in sorted(mem_dir.glob("*.md")):
                files.append((str(f), agent_name, "memory"))

    return files


def collect_doc_files() -> list[tuple[str, str, str]]:
    """Collect spec and doc files."""
    files = []
    if not DOCS_DIR.exists():
        return files

    for f in sorted(DOCS_DIR.rglob("*.md")):
        files.append((str(f), "system", "docs"))

    return files


def compute_content_hash(text: str) -> str:
    """Hash for dedup — same content won't be re-embedded."""
    return hashlib.md5(text.encode()).hexdigest()


def get_existing_hashes(client: QdrantClient) -> set[str]:
    """Get all backfill content hashes already in Qdrant."""
    hashes = set()
    offset = None
    while True:
        results, offset = client.scroll(
            collection_name=COLLECTION,
            scroll_filter=Filter(
                must=[FieldCondition(key="source", match=MatchValue(value="backfill"))]
            ),
            limit=100,
            offset=offset,
            with_payload=["hash"],
            with_vectors=False,
        )
        for point in results:
            h = point.payload.get("hash")
            if h:
                hashes.add(h)
        if offset is None:
            break
    return hashes


def backfill(
    files: list[tuple[str, str, str]],
    dry_run: bool = False,
    voyage_key: str | None = None,
) -> dict:
    """Embed and upsert curated knowledge into Qdrant."""
    
    key = voyage_key or os.environ.get("VOYAGE_API_KEY")
    if not key and not dry_run:
        print("ERROR: VOYAGE_API_KEY required (set in env or --voyage-key)")
        sys.exit(1)

    # Collect all chunks
    all_chunks: list[dict] = []
    for file_path, agent_id, source_type in files:
        try:
            text = Path(file_path).read_text()
        except Exception as e:
            print(f"  [error] {file_path}: {e}")
            continue

        rel_path = str(Path(file_path).relative_to(ALETHEIA_ROOT))
        chunks = chunk_text(text, rel_path)
        for chunk in chunks:
            chunk["agent_id"] = agent_id
            chunk["source_type"] = source_type
            chunk["hash"] = compute_content_hash(chunk["text"])
        all_chunks.extend(chunks)

    print(f"\nCollected {len(all_chunks)} chunks from {len(files)} files")

    if dry_run:
        for c in all_chunks[:10]:
            preview = c["text"][:100].replace("\n", " ")
            print(f"  [{c['agent_id']}:{c['source_type']}] {c['source']}")
            print(f"    {preview}...")
        if len(all_chunks) > 10:
            print(f"  ... and {len(all_chunks) - 10} more")
        return {"chunks": len(all_chunks), "files": len(files), "embedded": 0, "skipped": 0}

    # Connect
    client = QdrantClient(host=QDRANT_HOST, port=QDRANT_PORT)
    vo = voyageai.Client(api_key=key)

    # Dedup against existing backfill content
    existing_hashes = get_existing_hashes(client)
    new_chunks = [c for c in all_chunks if c["hash"] not in existing_hashes]
    skipped = len(all_chunks) - len(new_chunks)
    
    if skipped:
        print(f"Skipping {skipped} already-embedded chunks")
    
    if not new_chunks:
        print("Nothing new to embed.")
        return {"chunks": len(all_chunks), "files": len(files), "embedded": 0, "skipped": skipped}

    print(f"Embedding {len(new_chunks)} new chunks...")

    # Batch embed
    texts = [c["text"] for c in new_chunks]
    all_vectors = []
    for i in range(0, len(texts), BATCH_SIZE):
        batch = texts[i : i + BATCH_SIZE]
        result = vo.embed(batch, model=EMBED_MODEL)
        all_vectors.extend(result.embeddings)
        print(f"  Embedded batch {i // BATCH_SIZE + 1}/{(len(texts) + BATCH_SIZE - 1) // BATCH_SIZE}")

    # Build points
    now = datetime.now(timezone.utc).isoformat()
    points = []
    for chunk, vector in zip(new_chunks, all_vectors):
        memory_text = chunk["text"][:500]  # Mem0 convention: truncated for display
        section = f" [{chunk['section']}]" if chunk.get("section") else ""
        
        points.append(PointStruct(
            id=str(uuid4()),
            vector=vector,
            payload={
                "memory": memory_text,
                "data": chunk["text"],
                "source": "backfill",
                "source_file": chunk["source"],
                "source_type": chunk["source_type"],
                "section": chunk.get("section", ""),
                "hash": chunk["hash"],
                "user_id": USER_ID,
                "agent_id": chunk["agent_id"],
                "created_at": now,
            },
        ))

    # Upsert in batches
    UPSERT_BATCH = 100
    for i in range(0, len(points), UPSERT_BATCH):
        batch = points[i : i + UPSERT_BATCH]
        client.upsert(collection_name=COLLECTION, points=batch)
        print(f"  Upserted {min(i + UPSERT_BATCH, len(points))}/{len(points)}")

    # Verify
    info = client.get_collection(COLLECTION)
    print(f"\nDone. Collection now has {info.points_count} points.")
    
    return {
        "chunks": len(all_chunks),
        "files": len(files),
        "embedded": len(new_chunks),
        "skipped": skipped,
        "total_points": info.points_count,
    }


def main():
    parser = argparse.ArgumentParser(description="Backfill curated knowledge into Qdrant")
    parser.add_argument("--agent", help="Backfill only this agent's workspace")
    parser.add_argument("--source", choices=["agents", "docs", "all"], default="all",
                        help="Which files to backfill (default: all)")
    parser.add_argument("--dry-run", action="store_true", help="Show what would be embedded")
    parser.add_argument("--voyage-key", help="Voyage API key (or set VOYAGE_API_KEY)")
    args = parser.parse_args()

    print(f"Aletheia Memory Backfill")
    print(f"Root: {ALETHEIA_ROOT}")
    print(f"Collection: {COLLECTION} @ {QDRANT_HOST}:{QDRANT_PORT}")

    files = []
    if args.source in ("agents", "all"):
        files.extend(collect_agent_files(args.agent))
    if args.source in ("docs", "all") and not args.agent:
        files.extend(collect_doc_files())

    if not files:
        print("No files found to backfill.")
        return

    print(f"Found {len(files)} files to process")

    start = time.time()
    result = backfill(files, dry_run=args.dry_run, voyage_key=args.voyage_key)
    elapsed = time.time() - start

    print(f"\nSummary: {result['embedded']} embedded, {result['skipped']} skipped, "
          f"{result['chunks']} total chunks from {result['files']} files ({elapsed:.1f}s)")


if __name__ == "__main__":
    main()
