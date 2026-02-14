#!/usr/bin/env python3
# Nightly memory consolidation — merge near-duplicates, log stats

import json
import logging
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

import httpx

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
logger = logging.getLogger("consolidation")

SIDECAR_URL = os.environ.get("ALETHEIA_MEMORY_URL", "http://127.0.0.1:8230")
AUTH_TOKEN = os.environ.get("ALETHEIA_MEMORY_TOKEN", "")
STATS_DIR = Path(os.environ.get("ALETHEIA_HOME", "/mnt/ssd/aletheia")) / "shared" / "memory"


def headers() -> dict[str, str]:
    h = {"Content-Type": "application/json"}
    if AUTH_TOKEN:
        h["Authorization"] = f"Bearer {AUTH_TOKEN}"
    return h


def run_consolidation(dry_run: bool = False, threshold: float = 0.90) -> dict:
    """Run consolidation via sidecar endpoint."""
    logger.info(f"Starting consolidation (dry_run={dry_run}, threshold={threshold})")

    with httpx.Client(timeout=120.0) as client:
        resp = client.post(
            f"{SIDECAR_URL}/consolidate",
            headers=headers(),
            json={"dry_run": dry_run, "threshold": threshold, "limit": 200},
        )
        resp.raise_for_status()
        result = resp.json()

    logger.info(f"Consolidation: {result.get('candidates', 0)} candidates, {result.get('merged', 0)} merged")
    return result


def get_stats() -> dict:
    """Fetch current memory statistics."""
    with httpx.Client(timeout=30.0) as client:
        resp = client.get(f"{SIDECAR_URL}/fact_stats", headers=headers())
        resp.raise_for_status()
        return resp.json()


def get_graph_stats() -> dict:
    """Fetch graph statistics."""
    with httpx.Client(timeout=30.0) as client:
        resp = client.get(f"{SIDECAR_URL}/graph_stats", headers=headers())
        resp.raise_for_status()
        return resp.json()


def run_foresight_decay() -> dict:
    """Decay expired foresight signals."""
    with httpx.Client(timeout=30.0) as client:
        resp = client.post(f"{SIDECAR_URL}/foresight/decay", headers=headers())
        if resp.status_code == 404:
            logger.info("Foresight decay endpoint not available — skipping")
            return {"decayed": 0}
        resp.raise_for_status()
        return resp.json()


def run_graph_analytics(store_scores: bool = True) -> dict:
    """Run graph analytics (PageRank, community detection, dedup candidates)."""
    with httpx.Client(timeout=120.0) as client:
        resp = client.post(
            f"{SIDECAR_URL}/graph/analyze",
            headers=headers(),
            json={"store_scores": store_scores, "similarity_threshold": 0.5},
        )
        if resp.status_code == 404:
            logger.info("Graph analytics endpoint not available — skipping")
            return {}
        resp.raise_for_status()
        return resp.json()


def log_stats(
    stats: dict,
    graph_stats: dict,
    consolidation: dict,
    foresight: dict | None = None,
    analytics: dict | None = None,
) -> None:
    """Append daily stats to JSONL."""
    STATS_DIR.mkdir(parents=True, exist_ok=True)
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "memories": stats.get("total", 0),
        "avg_length": stats.get("avg_length", 0),
        "by_agent": stats.get("by_agent", {}),
        "graph_nodes": graph_stats.get("nodes", 0),
        "graph_relationships": graph_stats.get("relationships", 0),
        "consolidated": consolidation.get("merged", 0),
        "duplicate_candidates": consolidation.get("candidates", 0),
        "foresight_decayed": (foresight or {}).get("decayed", 0),
        "graph_communities": (analytics or {}).get("communities", 0),
        "graph_dedup_candidates": len((analytics or {}).get("dedup_candidates", [])),
    }
    stats_file = STATS_DIR / "consolidation-log.jsonl"
    with open(stats_file, "a") as f:
        f.write(json.dumps(entry) + "\n")
    logger.info(f"Stats logged to {stats_file}")


def main() -> None:
    dry_run = "--dry-run" in sys.argv
    threshold = 0.90

    for arg in sys.argv[1:]:
        if arg.startswith("--threshold="):
            threshold = float(arg.split("=", 1)[1])

    try:
        consolidation = run_consolidation(dry_run=dry_run, threshold=threshold)
        stats = get_stats()
        graph_stats = get_graph_stats()

        foresight = run_foresight_decay()
        logger.info(f"Foresight decay: {foresight.get('decayed', 0)} signals expired")

        analytics = run_graph_analytics(store_scores=not dry_run)
        if analytics:
            logger.info(
                f"Graph analytics: {analytics.get('communities', 0)} communities, "
                f"{len(analytics.get('dedup_candidates', []))} dedup candidates"
            )

        log_stats(stats, graph_stats, consolidation, foresight, analytics)

        print(f"Memories: {stats.get('total', '?')}")
        print(f"Graph: {graph_stats.get('nodes', '?')} nodes, {graph_stats.get('relationships', '?')} rels")
        print(f"Consolidated: {consolidation.get('merged', 0)} (candidates: {consolidation.get('candidates', 0)})")
        print(f"Foresight decayed: {foresight.get('decayed', 0)}")
        if analytics:
            print(f"Communities: {analytics.get('communities', 0)}")

        if dry_run:
            pairs = consolidation.get("pairs", [])
            if pairs:
                print(f"\nDry-run candidates ({len(pairs)}):")
                for p in pairs[:10]:
                    src = p.get("source", {}).get("text", "?")[:60]
                    dup = p.get("duplicate", {}).get("text", "?")[:60]
                    score = p.get("score", 0)
                    print(f"  [{score:.3f}] {src}... ≈ {dup}...")

    except httpx.HTTPError as e:
        logger.error(f"HTTP error: {e}")
        sys.exit(1)
    except Exception as e:
        logger.exception(f"Consolidation failed: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
