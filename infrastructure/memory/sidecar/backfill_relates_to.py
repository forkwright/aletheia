#!/usr/bin/env python3
# One-time migration: reclassify RELATES_TO edges via LLM-assisted type inference

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import sys
from pathlib import Path

log = logging.getLogger("backfill_relates_to")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

NEO4J_URL = os.environ.get("NEO4J_URL", "neo4j://localhost:7687")
NEO4J_USER = os.environ.get("NEO4J_USER", "neo4j")
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", os.environ.get("NEO4J_PASS", "chiron-memory"))

SIDECAR_DIR = Path(__file__).resolve().parent
CHECKPOINT_FILE = SIDECAR_DIR / "backfill_state.json"

VOCAB_LIST = sorted([
    "KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS",
    "STUDIES", "MANAGES", "MEMBER_OF", "INTERESTED_IN", "SKILLED_IN",
    "CREATED", "MAINTAINS", "DEPENDS_ON", "LOCATED_IN", "PART_OF",
    "SCHEDULED_FOR", "DIAGNOSED_WITH", "PRESCRIBED", "TREATS",
    "VEHICLE_IS", "INSTALLED_ON", "COMPATIBLE_WITH", "CONNECTED_TO",
    "COMMUNICATES_VIA", "CONFIGURED_WITH", "RUNS_ON", "SERVES",
])

RECLASSIFY_PROMPT = """\
You are a knowledge graph relationship classifier.

Given a source entity and target entity, classify the relationship between them.
Choose exactly ONE type from this controlled vocabulary:

{vocab}

If none of these types describes the relationship with confidence, respond with DELETE.

Source: {source}
Target: {target}

Respond with ONLY the relationship type (e.g. KNOWS) or DELETE. No explanation."""


def _neo4j_driver():
    from neo4j import GraphDatabase
    return GraphDatabase.driver(NEO4J_URL, auth=(NEO4J_USER, NEO4J_PASSWORD))


def get_stats(session) -> dict:
    """Return current RELATES_TO count, total edge count, and rate."""
    result = session.run(
        "MATCH ()-[r]->() WITH count(r) AS total "
        "OPTIONAL MATCH ()-[r2:RELATES_TO]->() "
        "RETURN count(r2) AS relates_to, total"
    )
    row = result.single()
    relates_to = row["relates_to"] or 0
    total = row["total"] or 0
    rate = relates_to / total if total > 0 else 0.0
    return {"relates_to": relates_to, "total": total, "rate": rate}


def print_stats(label: str, stats: dict) -> None:
    print(
        f"{label}: {stats['relates_to']} RELATES_TO / {stats['total']} total "
        f"({stats['rate']:.1%} rate)"
    )


def fetch_relates_to_edges(session) -> list[dict]:
    """Fetch all RELATES_TO edges as dicts with rid, source, target, props."""
    result = session.run(
        "MATCH (a)-[r:RELATES_TO]->(b) "
        "RETURN id(r) AS rid, a.name AS source, b.name AS target, properties(r) AS props"
    )
    return [
        {
            "rid": record["rid"],
            "source": record["source"] or "",
            "target": record["target"] or "",
            "props": dict(record["props"] or {}),
        }
        for record in result
    ]


def load_checkpoint() -> set[int]:
    """Load set of already-processed edge IDs from checkpoint file."""
    if not CHECKPOINT_FILE.exists():
        return set()
    try:
        data = json.loads(CHECKPOINT_FILE.read_text())
        return set(data.get("processed_ids", []))
    except Exception as exc:
        log.warning("Failed to load checkpoint: %s", exc)
        return set()


def save_checkpoint(processed_ids: set[int]) -> None:
    """Persist processed edge IDs to checkpoint file."""
    CHECKPOINT_FILE.write_text(json.dumps({"processed_ids": sorted(processed_ids)}))


def build_anthropic_client(backend: dict) -> object | None:
    """Build a raw anthropic.Anthropic client from the detected backend."""
    provider = backend.get("provider", "none")

    if provider == "anthropic-apikey":
        api_key = (backend.get("config") or {}).get("config", {}).get("api_key") or ""
        if not api_key:
            return None
        try:
            import anthropic as sdk
            return sdk.Anthropic(api_key=api_key)
        except Exception as exc:
            log.warning("Failed to build API-key client: %s", exc)
            return None

    if provider == "anthropic-oauth":
        token = backend.get("oauth_token")
        if not token:
            return None
        try:
            import anthropic as sdk
            return sdk.Anthropic(
                auth_token=token,
                default_headers={"anthropic-beta": "oauth-2025-04-20"},
            )
        except Exception as exc:
            log.warning("Failed to build OAuth client: %s", exc)
            return None

    return None


async def classify_edge(client, source: str, target: str, model: str) -> str:
    """Call LLM to classify a RELATES_TO edge. Returns a vocab type or 'DELETE'."""
    prompt = RECLASSIFY_PROMPT.format(
        vocab=", ".join(VOCAB_LIST),
        source=source or "(unnamed)",
        target=target or "(unnamed)",
    )
    try:
        response = await asyncio.to_thread(
            client.messages.create,
            model=model,
            max_tokens=20,
            messages=[{"role": "user", "content": prompt}],
        )
        raw = response.content[0].text.strip().upper().replace(".", "").replace(":", "")
        if raw in VOCAB_LIST:
            return raw
        if "DELETE" in raw:
            return "DELETE"
        log.warning("Unparseable LLM response for %s->%s: %r — deleting", source, target, raw)
        return "DELETE"
    except Exception as exc:
        log.warning("LLM call failed for %s->%s: %s — deleting", source, target, exc)
        return "DELETE"


def apply_reclassification(session, edge: dict, new_type: str) -> None:
    """Replace a RELATES_TO edge with a typed edge, preserving properties."""
    props = edge["props"]
    session.run(
        "MATCH ()-[r:RELATES_TO]->() WHERE id(r) = $rid "
        "MATCH (a)-[r2:RELATES_TO]->(b) WHERE id(r2) = $rid "
        f"CREATE (a)-[nr:{new_type} $props]->(b) "
        "DELETE r2",
        rid=edge["rid"],
        props=props,
    )


def delete_edge(session, rid: int) -> None:
    """Delete a RELATES_TO edge by internal ID."""
    session.run(
        "MATCH ()-[r:RELATES_TO]->() WHERE id(r) = $rid DELETE r",
        rid=rid,
    )


async def process_batch(
    client,
    model: str,
    batch: list[dict],
    dry_run: bool,
    session,
    processed_ids: set[int],
    rate_delay: float = 0.5,
) -> None:
    """Process one batch of RELATES_TO edges."""
    for edge in batch:
        new_type = await classify_edge(client, edge["source"], edge["target"], model)
        source = edge["source"] or "(unnamed)"
        target = edge["target"] or "(unnamed)"

        if new_type == "DELETE":
            action = "DELETE"
            log.info("  %s -> %s: DELETED", source, target)
        else:
            action = f"RELATES_TO -> {new_type}"
            log.info("  %s -> %s: %s", source, target, action)

        if not dry_run:
            if new_type == "DELETE":
                delete_edge(session, edge["rid"])
            else:
                apply_reclassification(session, edge, new_type)
            processed_ids.add(edge["rid"])
            save_checkpoint(processed_ids)

        await asyncio.sleep(rate_delay)


async def run_backfill(
    dry_run: bool = True,
    batch_size: int = 50,
    resume: bool = True,
) -> None:
    """Main backfill coroutine."""
    sys.path.insert(0, str(SIDECAR_DIR))
    try:
        from aletheia_memory.llm_backend import detect_backend
    except ImportError:
        log.error("Cannot import aletheia_memory — run from sidecar directory with the venv active")
        sys.exit(1)

    backend = detect_backend()
    if backend["provider"] == "none":
        log.error("No LLM backend available (Tier 3). Cannot reclassify edges.")
        sys.exit(1)

    if backend["provider"] == "ollama":
        log.error("Ollama backend not supported for reclassification — requires Anthropic.")
        sys.exit(1)

    client = build_anthropic_client(backend)
    if client is None:
        log.error("Failed to build Anthropic client from backend: %s", backend["provider"])
        sys.exit(1)

    model = backend.get("model", "claude-haiku-4-5-20251001")
    log.info("Using model: %s (provider: %s)", model, backend["provider"])

    driver = _neo4j_driver()
    try:
        with driver.session() as session:
            before_stats = get_stats(session)
            print_stats("BEFORE", before_stats)
            print(f"  Detected {before_stats['relates_to']} RELATES_TO edges to process")

            if before_stats["relates_to"] == 0:
                print("No RELATES_TO edges found. Nothing to do.")
                return

            edges = fetch_relates_to_edges(session)
            log.info("Fetched %d RELATES_TO edges", len(edges))

        processed_ids: set[int] = set()
        if resume:
            processed_ids = load_checkpoint()
            if processed_ids:
                log.info("Resuming from checkpoint: %d edges already processed", len(processed_ids))

        pending = [e for e in edges if e["rid"] not in processed_ids]
        log.info("%d edges pending processing", len(pending))

        if dry_run:
            print("\n[DRY RUN] No changes will be made. Sample reclassifications:")

        total_batches = (len(pending) + batch_size - 1) // batch_size if pending else 0

        with driver.session() as session:
            for batch_idx in range(0, len(pending), batch_size):
                batch = pending[batch_idx : batch_idx + batch_size]
                batch_num = batch_idx // batch_size + 1
                print(f"\nBatch {batch_num}/{total_batches} ({len(batch)} edges)")

                await process_batch(
                    client=client,
                    model=model,
                    batch=batch,
                    dry_run=dry_run,
                    session=session,
                    processed_ids=processed_ids,
                )

        if not dry_run:
            with driver.session() as session:
                after_stats = get_stats(session)
            print_stats("AFTER ", after_stats)
            print(f"\nRELATES_TO rate change: {before_stats['rate']:.1%} -> {after_stats['rate']:.1%}")
            if after_stats["rate"] < 0.30:
                print("SUCCESS: RELATES_TO rate is below 30% target.")
            else:
                print(f"WARNING: RELATES_TO rate ({after_stats['rate']:.1%}) is still above 30%.")

            if CHECKPOINT_FILE.exists():
                CHECKPOINT_FILE.unlink()
                log.info("Checkpoint file removed after successful completion")
        else:
            print(f"\n[DRY RUN] Would process {len(pending)} RELATES_TO edges in {total_batches} batches.")
            print("Run with --execute to apply changes.")

    finally:
        driver.close()


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Reclassify RELATES_TO edges via LLM-assisted type inference"
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        default=True,
        help="Print what would be done without making changes (default)",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Actually perform the reclassification",
    )
    parser.add_argument(
        "--batch-size",
        type=int,
        default=50,
        help="Number of edges per LLM batch (default: 50)",
    )
    parser.add_argument(
        "--resume",
        action="store_true",
        default=True,
        help="Resume from checkpoint file if present (default: enabled)",
    )
    parser.add_argument(
        "--no-resume",
        action="store_true",
        help="Ignore checkpoint and reprocess all edges",
    )
    args = parser.parse_args()

    dry_run = not args.execute
    resume = not args.no_resume

    if args.execute:
        print("EXECUTE mode: changes WILL be written to Neo4j.")
    else:
        print("DRY RUN mode: no changes will be made (use --execute to apply).")

    asyncio.run(run_backfill(dry_run=dry_run, batch_size=args.batch_size, resume=resume))


if __name__ == "__main__":
    main()
