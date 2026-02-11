#!/usr/bin/env python3
# One-time Neo4j graph cleanup — normalize relationship types and merge duplicate entities

import sys
from collections import defaultdict

from neo4j import GraphDatabase

NEO4J_URI = "neo4j://localhost:7687"
NEO4J_USER = "neo4j"
NEO4J_PASSWORD = "aletheia-memory"

CONTROLLED_VOCAB = {
    "KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS",
    "STUDIES", "MANAGES", "MEMBER_OF", "INTERESTED_IN", "SKILLED_IN",
    "CREATED", "MAINTAINS", "DEPENDS_ON", "LOCATED_IN", "PART_OF",
    "SCHEDULED_FOR", "DIAGNOSED_WITH", "PRESCRIBED", "TREATS",
    "VEHICLE_IS", "INSTALLED_ON", "COMPATIBLE_WITH", "CONNECTED_TO",
    "COMMUNICATES_VIA", "CONFIGURED_WITH", "RUNS_ON", "SERVES",
    "RELATES_TO",
}

# Mapping from common non-vocab types to controlled vocab
TYPE_MAP = {
    "is": "RELATES_TO",
    "has": "OWNS",
    "is_a": "RELATES_TO",
    "is_part_of": "PART_OF",
    "part_of": "PART_OF",
    "works_at": "WORKS_AT",
    "works_on": "WORKS_AT",
    "lives_in": "LIVES_IN",
    "located_in": "LOCATED_IN",
    "located_at": "LOCATED_IN",
    "uses": "USES",
    "used_by": "USES",
    "used_for": "USES",
    "runs_on": "RUNS_ON",
    "runs": "RUNS_ON",
    "depends_on": "DEPENDS_ON",
    "requires": "DEPENDS_ON",
    "knows": "KNOWS",
    "knows_about": "KNOWS",
    "knows_of": "KNOWS",
    "prefers": "PREFERS",
    "likes": "PREFERS",
    "interested_in": "INTERESTED_IN",
    "studies": "STUDIES",
    "studying": "STUDIES",
    "created": "CREATED",
    "created_by": "CREATED",
    "built": "CREATED",
    "made": "CREATED",
    "maintains": "MAINTAINS",
    "managed_by": "MANAGES",
    "manages": "MANAGES",
    "member_of": "MEMBER_OF",
    "belongs_to": "MEMBER_OF",
    "skilled_in": "SKILLED_IN",
    "skilled_at": "SKILLED_IN",
    "owns": "OWNS",
    "has_a": "OWNS",
    "installed_on": "INSTALLED_ON",
    "installed": "INSTALLED_ON",
    "compatible_with": "COMPATIBLE_WITH",
    "connected_to": "CONNECTED_TO",
    "communicates_via": "COMMUNICATES_VIA",
    "configured_with": "CONFIGURED_WITH",
    "serves": "SERVES",
    "diagnosed_with": "DIAGNOSED_WITH",
    "prescribed": "PRESCRIBED",
    "treats": "TREATS",
    "scheduled_for": "SCHEDULED_FOR",
    "vehicle_is": "VEHICLE_IS",
    "relates_to": "RELATES_TO",
}


def run_query(session, cypher, **params):
    result = session.run(cypher, **params)
    return [dict(r) for r in result]


def normalize_type(rel_type):
    """Map a relationship type to controlled vocabulary."""
    if rel_type in CONTROLLED_VOCAB:
        return rel_type

    lower = rel_type.lower().strip()
    if lower in TYPE_MAP:
        return TYPE_MAP[lower]

    # Heuristic: check if the lowered, underscored version maps
    normalized = lower.replace(" ", "_").replace("-", "_")
    if normalized in TYPE_MAP:
        return TYPE_MAP[normalized]

    # Keywords in the type name
    keyword_map = {
        "know": "KNOWS",
        "live": "LIVES_IN",
        "work": "WORKS_AT",
        "own": "OWNS",
        "use": "USES",
        "prefer": "PREFERS",
        "stud": "STUDIES",
        "manag": "MANAGES",
        "member": "MEMBER_OF",
        "interest": "INTERESTED_IN",
        "skill": "SKILLED_IN",
        "creat": "CREATED",
        "maintain": "MAINTAINS",
        "depend": "DEPENDS_ON",
        "locat": "LOCATED_IN",
        "part": "PART_OF",
        "schedul": "SCHEDULED_FOR",
        "diagnos": "DIAGNOSED_WITH",
        "prescri": "PRESCRIBED",
        "treat": "TREATS",
        "vehicle": "VEHICLE_IS",
        "install": "INSTALLED_ON",
        "compat": "COMPATIBLE_WITH",
        "connect": "CONNECTED_TO",
        "communic": "COMMUNICATES_VIA",
        "config": "CONFIGURED_WITH",
        "run": "RUNS_ON",
        "serv": "SERVES",
    }
    for keyword, target in keyword_map.items():
        if keyword in normalized:
            return target

    return "RELATES_TO"


def main():
    dry_run = "--dry-run" in sys.argv
    driver = GraphDatabase.driver(NEO4J_URI, auth=(NEO4J_USER, NEO4J_PASSWORD))

    with driver.session() as session:
        # === Pre-cleanup stats ===
        nodes_before = run_query(session, "MATCH (n) RETURN count(n) AS c")[0]["c"]
        rels_before = run_query(session, "MATCH ()-[r]->() RETURN count(r) AS c")[0]["c"]
        types_before = run_query(session, "MATCH ()-[r]->() RETURN count(DISTINCT type(r)) AS c")[0]["c"]

        print(f"=== PRE-CLEANUP ===")
        print(f"Nodes: {nodes_before}")
        print(f"Relationships: {rels_before}")
        print(f"Relationship types: {types_before}")

        # === Step 1: Normalize relationship types ===
        print(f"\n=== STEP 1: NORMALIZE RELATIONSHIP TYPES ===")

        all_types = run_query(session, """
            MATCH ()-[r]->()
            RETURN type(r) AS t, count(*) AS c
            ORDER BY c DESC
        """)

        remap_plan = defaultdict(list)
        already_good = 0
        for row in all_types:
            rel_type = row["t"]
            count = row["c"]
            target = normalize_type(rel_type)
            if rel_type == target:
                already_good += count
            else:
                remap_plan[target].append((rel_type, count))

        print(f"Already in vocabulary: {already_good} relationships")
        print(f"Types to remap: {sum(len(v) for v in remap_plan.values())}")

        total_remapped = 0
        for target, sources in sorted(remap_plan.items()):
            source_count = sum(c for _, c in sources)
            print(f"\n  → {target} ({source_count} relationships from {len(sources)} types)")
            for src_type, count in sorted(sources, key=lambda x: -x[1])[:5]:
                print(f"    - {src_type}: {count}")
            if len(sources) > 5:
                print(f"    ... and {len(sources) - 5} more types")

            if not dry_run:
                for src_type, count in sources:
                    # Create new relationship with target type, copy properties, delete old
                    session.run(f"""
                        MATCH (a)-[r:`{src_type}`]->(b)
                        WITH a, b, r, properties(r) AS props
                        CREATE (a)-[r2:`{target}`]->(b)
                        SET r2 = props
                        DELETE r
                    """)
                    total_remapped += count

        if not dry_run:
            print(f"\nRemapped {total_remapped} relationships")
        else:
            print(f"\n[DRY RUN] Would remap {sum(sum(c for _, c in v) for v in remap_plan.values())} relationships")

        # === Step 2: Merge duplicate entity nodes ===
        print(f"\n=== STEP 2: MERGE DUPLICATE ENTITIES ===")

        # Find nodes with same name (case-insensitive)
        duplicates = run_query(session, """
            MATCH (n)
            WHERE n.name IS NOT NULL
            WITH toLower(trim(n.name)) AS normalized, collect(n) AS nodes
            WHERE size(nodes) > 1
            RETURN normalized, size(nodes) AS count,
                   [n IN nodes | {id: id(n), name: n.name, labels: labels(n)}] AS instances
            ORDER BY count DESC
        """)

        print(f"Duplicate entity groups: {len(duplicates)}")
        total_merged = 0

        for dup in duplicates:
            name = dup["normalized"]
            count = dup["count"]
            instances = dup["instances"]
            print(f"  {name}: {count} nodes")

            if not dry_run and count > 1:
                node_ids = [inst["id"] for inst in instances]

                # Find the node with most connections — that's our canonical
                counts = run_query(session, """
                    UNWIND $ids AS nid
                    MATCH (n) WHERE id(n) = nid
                    OPTIONAL MATCH (n)-[r]-()
                    RETURN id(n) AS nid, count(r) AS rels
                    ORDER BY rels DESC
                """, ids=node_ids)

                if len(counts) < 2:
                    continue

                primary_id = counts[0]["nid"]
                merge_ids = [c["nid"] for c in counts[1:]]

                for merge_id in merge_ids:
                    # Get relationship types to move (Cypher requires literal types)
                    incoming = run_query(session, """
                        MATCH (other)-[r]->(old) WHERE id(old) = $merge_id
                        MATCH (primary) WHERE id(primary) = $primary_id
                        WITH other, r, primary
                        WHERE other <> primary
                        RETURN DISTINCT type(r) AS rtype
                    """, merge_id=merge_id, primary_id=primary_id)

                    for row in incoming:
                        rtype = row["rtype"]
                        session.run(f"""
                            MATCH (other)-[r:`{rtype}`]->(old) WHERE id(old) = $merge_id
                            MATCH (primary) WHERE id(primary) = $primary_id
                            WITH other, r, primary
                            WHERE other <> primary
                            CREATE (other)-[:`{rtype}`]->(primary)
                            DELETE r
                        """, merge_id=merge_id, primary_id=primary_id)

                    outgoing = run_query(session, """
                        MATCH (old)-[r]->(other) WHERE id(old) = $merge_id
                        MATCH (primary) WHERE id(primary) = $primary_id
                        WITH other, r, primary
                        WHERE other <> primary
                        RETURN DISTINCT type(r) AS rtype
                    """, merge_id=merge_id, primary_id=primary_id)

                    for row in outgoing:
                        rtype = row["rtype"]
                        session.run(f"""
                            MATCH (old)-[r:`{rtype}`]->(other) WHERE id(old) = $merge_id
                            MATCH (primary) WHERE id(primary) = $primary_id
                            WITH other, r, primary
                            WHERE other <> primary
                            CREATE (primary)-[:`{rtype}`]->(other)
                            DELETE r
                        """, merge_id=merge_id, primary_id=primary_id)

                    session.run("MATCH (n) WHERE id(n) = $merge_id DETACH DELETE n",
                                merge_id=merge_id)
                    total_merged += 1

        if dry_run:
            print(f"[DRY RUN] Would merge {sum(d['count'] - 1 for d in duplicates)} duplicate nodes")
        else:
            print(f"Merged {total_merged} duplicate nodes")

        # === Step 3: Deduplicate relationships ===
        print(f"\n=== STEP 3: DEDUPLICATE RELATIONSHIPS ===")

        if not dry_run:
            result = session.run("""
                MATCH (a)-[r]->(b)
                WITH a, b, type(r) AS relType, collect(r) AS rels
                WHERE size(rels) > 1
                UNWIND rels[1..] AS duplicate
                DELETE duplicate
                RETURN count(duplicate) AS deleted
            """)
            deduped = result.single()["deleted"]
            print(f"Removed {deduped} duplicate relationships")
        else:
            dup_count = run_query(session, """
                MATCH (a)-[r]->(b)
                WITH a, b, type(r) AS relType, count(r) AS cnt
                WHERE cnt > 1
                RETURN sum(cnt - 1) AS total
            """)
            print(f"[DRY RUN] Would remove {dup_count[0]['total'] or 0} duplicate relationships")

        # === Step 4: Delete self-referencing ===
        print(f"\n=== STEP 4: SELF-REFERENCES ===")

        if not dry_run:
            result = session.run("MATCH (n)-[r]->(n) DELETE r RETURN count(r) AS deleted")
            deleted = result.single()["deleted"]
            print(f"Removed {deleted} self-referencing relationships")
        else:
            count = run_query(session, "MATCH (n)-[r]->(n) RETURN count(r) AS c")[0]["c"]
            print(f"[DRY RUN] Would remove {count} self-referencing relationships")

        # === Step 5: Delete orphaned nodes ===
        print(f"\n=== STEP 5: ORPHANED NODES ===")

        if not dry_run:
            result = session.run("MATCH (n) WHERE NOT (n)--() DELETE n RETURN count(n) AS deleted")
            deleted = result.single()["deleted"]
            print(f"Removed {deleted} orphaned nodes")
        else:
            count = run_query(session, "MATCH (n) WHERE NOT (n)--() RETURN count(n) AS c")[0]["c"]
            print(f"[DRY RUN] Would remove {count} orphaned nodes")

        # === Post-cleanup stats ===
        nodes_after = run_query(session, "MATCH (n) RETURN count(n) AS c")[0]["c"]
        rels_after = run_query(session, "MATCH ()-[r]->() RETURN count(r) AS c")[0]["c"]
        types_after = run_query(session, "MATCH ()-[r]->() RETURN count(DISTINCT type(r)) AS c")[0]["c"]

        print(f"\n=== POST-CLEANUP ===")
        print(f"Nodes: {nodes_before} → {nodes_after}")
        print(f"Relationships: {rels_before} → {rels_after}")
        print(f"Relationship types: {types_before} → {types_after}")

    driver.close()
    print("\nDone.")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--help":
        print("Usage: python neo4j-cleanup.py [--dry-run]")
        print("  --dry-run  Show what would be done without making changes")
        sys.exit(0)
    main()
