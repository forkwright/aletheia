#!/usr/bin/env python3
# QA audit: find and remove near-duplicate memories in Qdrant

import sys
from collections import defaultdict

import httpx
import numpy as np

QDRANT_URL = "http://localhost:6333"
SIDECAR_URL = "http://127.0.0.1:8230"
COLLECTION = "aletheia_memories"
SIMILARITY_THRESHOLD = 0.95
BATCH_SIZE = 100


def get_all_points_with_vectors():
    points = []
    offset = None
    while True:
        body = {"limit": BATCH_SIZE, "with_payload": True, "with_vector": True}
        if offset:
            body["offset"] = offset
        resp = httpx.post(
            f"{QDRANT_URL}/collections/{COLLECTION}/points/scroll",
            json=body,
            timeout=60.0,
        )
        resp.raise_for_status()
        data = resp.json()["result"]
        batch = data.get("points", [])
        if not batch:
            break
        points.extend(batch)
        offset = data.get("next_page_offset")
        if not offset:
            break
        print(f"  Loaded {len(points)} points...", flush=True)
    return points


def cosine_similarity(a, b):
    a = np.array(a, dtype=np.float32)
    b = np.array(b, dtype=np.float32)
    dot = np.dot(a, b)
    norm = np.linalg.norm(a) * np.linalg.norm(b)
    if norm == 0:
        return 0.0
    return float(dot / norm)


def find_duplicates(points):
    n = len(points)
    duplicates = []
    seen = set()

    vectors = []
    for p in points:
        vec = p.get("vector", [])
        if isinstance(vec, dict):
            vec = vec.get("default", vec.get("", []))
        vectors.append(np.array(vec, dtype=np.float32) if vec else None)

    norms = []
    for v in vectors:
        if v is not None and len(v) > 0:
            norms.append(np.linalg.norm(v))
        else:
            norms.append(0.0)

    print(f"Computing pairwise similarities for {n} points...", flush=True)
    print("(This may take a while for large collections)", flush=True)

    # Process in blocks to show progress
    checked = 0
    total_pairs = n * (n - 1) // 2
    for i in range(n):
        if vectors[i] is None or norms[i] == 0:
            continue
        for j in range(i + 1, n):
            if vectors[j] is None or norms[j] == 0:
                continue
            checked += 1
            if checked % 500000 == 0:
                print(f"  Checked {checked}/{total_pairs} pairs, found {len(duplicates)} dupes so far", flush=True)

            sim = np.dot(vectors[i], vectors[j]) / (norms[i] * norms[j])
            if sim >= SIMILARITY_THRESHOLD:
                duplicates.append((i, j, float(sim)))

    return duplicates


def main():
    print("Loading all memories with vectors from Qdrant...", flush=True)
    points = get_all_points_with_vectors()
    print(f"Total: {len(points)} memories\n", flush=True)

    if len(points) > 10000:
        print("WARNING: >10K points, pairwise comparison will be slow.")
        print("Consider using approximate nearest neighbor search instead.\n")

    duplicates = find_duplicates(points)
    print(f"\nFound {len(duplicates)} duplicate pairs (similarity >= {SIMILARITY_THRESHOLD})\n")

    if not duplicates:
        print("No duplicates found. Memory store is clean.")
        return

    # Group duplicates into clusters
    clusters = defaultdict(set)
    for i, j, sim in duplicates:
        # Find existing cluster or create new one
        found = None
        for cid, members in clusters.items():
            if i in members or j in members:
                found = cid
                break
        if found is not None:
            clusters[found].add(i)
            clusters[found].add(j)
        else:
            clusters[len(clusters)].update([i, j])

    print(f"Duplicate clusters: {len(clusters)}")
    to_delete = []

    for cid, members in sorted(clusters.items()):
        members = sorted(members)
        keep = members[0]
        remove = members[1:]
        to_delete.extend(remove)

        keep_payload = points[keep].get("payload", {})
        keep_text = keep_payload.get("memory", keep_payload.get("data", ""))[:100]
        print(f"\n  Cluster {cid} ({len(members)} memories, keeping oldest):")
        print(f"    KEEP: [{points[keep]['id']}] {keep_text}...")
        for idx in remove[:3]:
            rm_payload = points[idx].get("payload", {})
            rm_text = rm_payload.get("memory", rm_payload.get("data", ""))[:100]
            sim = next((s for i, j, s in duplicates if (i == keep and j == idx) or (j == keep and i == idx)), 0)
            print(f"    DEL:  [{points[idx]['id']}] (sim={sim:.3f}) {rm_text}...")
        if len(remove) > 3:
            print(f"    ... and {len(remove) - 3} more")

    print(f"\nTotal to delete: {len(to_delete)} duplicate memories")
    print(f"Would reduce collection from {len(points)} to {len(points) - len(to_delete)}")

    if "--delete" in sys.argv:
        print(f"\nDeleting {len(to_delete)} duplicates...")
        client = httpx.Client(timeout=10.0)
        deleted = 0
        for idx in to_delete:
            pid = points[idx]["id"]
            try:
                resp = client.delete(f"{SIDECAR_URL}/memories/{pid}")
                if resp.status_code == 200:
                    deleted += 1
            except Exception as e:
                print(f"  Failed to delete {pid}: {e}")
        client.close()
        print(f"Deleted {deleted}/{len(to_delete)}")
    else:
        print("\nRun with --delete to remove duplicates")


if __name__ == "__main__":
    main()
