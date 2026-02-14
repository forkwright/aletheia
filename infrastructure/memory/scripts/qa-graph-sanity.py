#!/usr/bin/env python3
# QA audit: Neo4j graph sanity check

import os
import sys

from neo4j import GraphDatabase

NEO4J_URI = "neo4j://localhost:7687"
NEO4J_USER = "neo4j"
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", "")


def run_query(session, cypher, **params):
    result = session.run(cypher, **params)
    return [dict(r) for r in result]


def main():
    driver = GraphDatabase.driver(NEO4J_URI, auth=(NEO4J_USER, NEO4J_PASSWORD))

    with driver.session() as session:
        # 1. Overall stats
        print("=== GRAPH STATS ===", flush=True)
        nodes = run_query(session, "MATCH (n) RETURN count(n) AS count")[0]["count"]
        rels = run_query(session, "MATCH ()-[r]->() RETURN count(r) AS count")[0]["count"]
        print(f"Total nodes: {nodes}")
        print(f"Total relationships: {rels}")

        # Node labels
        labels = run_query(session, "MATCH (n) RETURN DISTINCT labels(n) AS labels, count(n) AS count ORDER BY count DESC")
        print("\nNode labels:")
        for r in labels:
            print(f"  {r['labels']}: {r['count']}")

        # Relationship types
        rel_types = run_query(session, "MATCH ()-[r]->() RETURN type(r) AS type, count(r) AS count ORDER BY count DESC LIMIT 20")
        print("\nTop relationship types:")
        for r in rel_types:
            print(f"  {r['type']}: {r['count']}")

        # 2. Orphaned nodes (no relationships)
        print("\n=== ORPHANED NODES ===", flush=True)
        orphans = run_query(session, """
            MATCH (n)
            WHERE NOT (n)--()
            RETURN n.name AS name, labels(n) AS labels, n.source AS source
            ORDER BY n.name
            LIMIT 50
        """)
        print(f"Orphaned nodes (no relationships): {len(orphans)}")
        if orphans:
            for o in orphans[:20]:
                print(f"  [{','.join(o['labels'] or [])}] {o['name']} (source: {o.get('source', '?')})")
            if len(orphans) > 20:
                print(f"  ... and {len(orphans) - 20} more")

        orphan_count = run_query(session, "MATCH (n) WHERE NOT (n)--() RETURN count(n) AS count")[0]["count"]
        if orphan_count > len(orphans):
            print(f"  (showing 50 of {orphan_count} total orphans)")

        # 3. Suspicious entity names (code artifacts)
        print("\n=== SUSPICIOUS ENTITIES ===", flush=True)
        suspicious_patterns = [
            ("File paths", "MATCH (n) WHERE n.name CONTAINS '/' AND size(n.name) > 20 RETURN n.name AS name LIMIT 15"),
            ("Code-like", "MATCH (n) WHERE n.name CONTAINS '(' OR n.name CONTAINS '{' OR n.name CONTAINS '=' RETURN n.name AS name LIMIT 15"),
            ("Very short names", "MATCH (n) WHERE size(n.name) <= 2 RETURN n.name AS name LIMIT 15"),
            ("Very long names", "MATCH (n) WHERE size(n.name) > 100 RETURN n.name AS name, size(n.name) AS len LIMIT 15"),
            ("URLs", "MATCH (n) WHERE n.name STARTS WITH 'http' RETURN n.name AS name LIMIT 15"),
        ]

        suspect_names = set()
        for label, query in suspicious_patterns:
            results = run_query(session, query)
            if results:
                print(f"\n  {label} ({len(results)} found):")
                for r in results[:5]:
                    name = r["name"]
                    suspect_names.add(name)
                    display = name[:80] + "..." if len(name) > 80 else name
                    print(f"    - {display}")
                if len(results) > 5:
                    print(f"    ... and {len(results) - 5} more")

        # 4. Relationship quality
        print("\n=== RELATIONSHIP QUALITY ===", flush=True)

        # Self-referencing
        self_refs = run_query(session, "MATCH (n)-[r]->(n) RETURN n.name AS name, type(r) AS rel LIMIT 10")
        print(f"Self-referencing nodes: {len(self_refs)}")
        for r in self_refs[:5]:
            print(f"  {r['name']} --[{r['rel']}]--> (self)")

        # Duplicate relationships (same type between same nodes)
        dup_rels = run_query(session, """
            MATCH (a)-[r]->(b)
            WITH a, b, type(r) AS relType, count(r) AS cnt
            WHERE cnt > 1
            RETURN a.name AS src, b.name AS dst, relType, cnt
            ORDER BY cnt DESC
            LIMIT 15
        """)
        print(f"\nDuplicate relationships: {len(dup_rels)}")
        for r in dup_rels[:10]:
            print(f"  {r['src']} --[{r['relType']}]--> {r['dst']} (x{r['cnt']})")

        # 5. Source distribution
        print("\n=== SOURCE DISTRIBUTION ===", flush=True)
        sources = run_query(session, """
            MATCH (n)
            RETURN COALESCE(n.source, 'unknown') AS source, count(n) AS count
            ORDER BY count DESC
        """)
        for s in sources:
            print(f"  {s['source']}: {s['count']}")

        # 6. Summary and recommendations
        print("\n=== SUMMARY ===", flush=True)
        issues = []
        if orphan_count > nodes * 0.2:
            issues.append(f"High orphan rate: {orphan_count}/{nodes} ({orphan_count/nodes*100:.0f}%) nodes have no relationships")
        if len(suspect_names) > 20:
            issues.append(f"{len(suspect_names)} suspicious entity names detected (code/path artifacts)")
        if len(self_refs) > 0:
            issues.append(f"{len(self_refs)} self-referencing nodes")
        if len(dup_rels) > 0:
            total_dup_count = sum(r["cnt"] - 1 for r in dup_rels)
            issues.append(f"{total_dup_count} duplicate relationships across {len(dup_rels)} pairs")

        if issues:
            print("Issues found:")
            for issue in issues:
                print(f"  - {issue}")
        else:
            print("Graph looks clean!")

        if "--delete-orphans" in sys.argv and orphan_count > 0:
            print(f"\nDeleting {orphan_count} orphaned nodes...")
            result = session.run("MATCH (n) WHERE NOT (n)--() DELETE n RETURN count(n) AS deleted")
            deleted = result.single()["deleted"]
            print(f"Deleted {deleted} orphaned nodes")

        if "--delete-self-refs" in sys.argv and self_refs:
            print(f"\nDeleting self-referencing relationships...")
            result = session.run("MATCH (n)-[r]->(n) DELETE r RETURN count(r) AS deleted")
            deleted = result.single()["deleted"]
            print(f"Deleted {deleted} self-referencing relationships")

        if "--dedup-rels" in sys.argv and dup_rels:
            print(f"\nDeduplicating relationships...")
            result = session.run("""
                MATCH (a)-[r]->(b)
                WITH a, b, type(r) AS relType, collect(r) AS rels
                WHERE size(rels) > 1
                UNWIND rels[1..] AS duplicate
                DELETE duplicate
                RETURN count(duplicate) AS deleted
            """)
            deleted = result.single()["deleted"]
            print(f"Deleted {deleted} duplicate relationships")

    driver.close()
    print("\nDone.", flush=True)


if __name__ == "__main__":
    main()
