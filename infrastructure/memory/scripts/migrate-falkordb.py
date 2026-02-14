#!/usr/bin/env python3
# Migrate FalkorDB graph data directly to Neo4j (no Mem0/Haiku needed)

import os
from pathlib import Path

from neo4j import GraphDatabase

NEO4J_URI = "neo4j://localhost:7687"
NEO4J_USER = "neo4j"
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", "")
RESCUE_DIR = Path("/mnt/ssd/aletheia/data/falkordb-rescue")


def parse_nodes(filepath: Path) -> list[dict]:
    nodes = []
    lines = filepath.read_text().splitlines()
    i = 0
    while i < len(lines) and lines[i] in ("n", ""):
        i += 1

    current = {}
    in_props = False

    while i < len(lines):
        line = lines[i].strip()
        i += 1

        if not line:
            continue

        if line == "id":
            if current.get("name"):
                nodes.append(dict(current))
            current = {}
            in_props = False
            current["id"] = lines[i].strip() if i < len(lines) else ""
            i += 1
            continue

        if line == "labels":
            current["labels"] = lines[i].strip() if i < len(lines) else "Entity"
            i += 1
            continue

        if line == "properties":
            in_props = True
            continue

        if in_props and i < len(lines):
            val = lines[i].strip()
            i += 1
            current[line] = val

    if current.get("name"):
        nodes.append(dict(current))

    return nodes


def parse_edges(filepath: Path) -> list[dict]:
    triples = []
    lines = filepath.read_text().splitlines()
    i = 0
    while i < len(lines) and lines[i] in ("n", "r", "m", ""):
        i += 1

    current = {}
    section = None
    in_props = False

    while i < len(lines):
        line = lines[i].strip()
        i += 1

        if not line:
            continue

        if line == "id" and i < len(lines):
            val = lines[i].strip()
            i += 1

            if "src_name" not in current and "rel_type" not in current:
                section = "n"
                current["src_id"] = val
                in_props = False
            elif "src_name" in current and "rel_type" not in current:
                section = "r"
                current["rel_id"] = val
                in_props = False
            else:
                section = "m"
                current["tgt_id"] = val
                in_props = False
            continue

        if line == "labels":
            val = lines[i].strip() if i < len(lines) else ""
            i += 1
            if section == "n":
                current["src_label"] = val
            elif section == "m":
                current["tgt_label"] = val
            continue

        if line == "type":
            val = lines[i].strip() if i < len(lines) else ""
            i += 1
            current["rel_type"] = val
            continue

        if line in ("src_node", "dest_node"):
            i += 1
            continue

        if line == "properties":
            in_props = True
            continue

        if in_props and i < len(lines):
            val = lines[i].strip()
            i += 1
            if line == "name":
                if section == "n":
                    current["src_name"] = val
                elif section == "m":
                    current["tgt_name"] = val
            elif line == "domain":
                if section == "n":
                    current["src_domain"] = val
                elif section == "m":
                    current["tgt_domain"] = val
            else:
                current[f"rel_{line}"] = val

        if "src_name" in current and "rel_type" in current and "tgt_name" in current:
            triples.append(dict(current))
            current = {}
            section = None
            in_props = False

    return triples


def import_graph(driver, graph_name: str):
    edges_file = RESCUE_DIR / f"{graph_name}-edges.txt"
    nodes_file = RESCUE_DIR / f"{graph_name}-nodes.txt"

    if not edges_file.exists():
        print(f"  Skipping {graph_name}: no edges file")
        return 0, 0

    print(f"\n=== {graph_name} ===")

    nodes = parse_nodes(nodes_file) if nodes_file.exists() else []
    triples = parse_edges(edges_file)
    print(f"  {len(nodes)} nodes, {len(triples)} edges")

    imported = 0
    errors = 0

    with driver.session() as session:
        for t in triples:
            src = t.get("src_name", "unknown")
            tgt = t.get("tgt_name", "unknown")
            rel_type = t.get("rel_type", "RELATED_TO")
            src_domain = t.get("src_domain", "shared")
            tgt_domain = t.get("tgt_domain", "shared")
            confidence = float(t.get("rel_confidence", 0.8))
            timestamp = t.get("rel_timestamp", "")

            safe_rel = rel_type.upper().replace(" ", "_").replace("-", "_")
            if not safe_rel.isidentifier():
                safe_rel = "RELATED_TO"

            try:
                session.run(
                    f"MERGE (s:Entity {{name: $src}}) "
                    f"ON CREATE SET s.domain = $src_domain, s.source = $graph "
                    f"MERGE (o:Entity {{name: $tgt}}) "
                    f"ON CREATE SET o.domain = $tgt_domain, o.source = $graph "
                    f"CREATE (s)-[r:{safe_rel} {{"
                    f"confidence: $conf, source: $graph, migrated: true"
                    f"}}]->(o)",
                    src=src, tgt=tgt, src_domain=src_domain,
                    tgt_domain=tgt_domain, conf=confidence,
                    graph=f"falkordb_{graph_name}",
                )
                imported += 1
            except Exception as e:
                print(f"  ERROR: {src} -[{safe_rel}]-> {tgt}: {e}")
                errors += 1

    print(f"  {imported} edges imported, {errors} errors")
    return imported, errors


def main():
    driver = GraphDatabase.driver(NEO4J_URI, auth=(NEO4J_USER, NEO4J_PASSWORD))

    try:
        with driver.session() as session:
            session.run("CREATE INDEX entity_name IF NOT EXISTS FOR (n:Entity) ON (n.name)")
            session.run("CREATE INDEX entity_domain IF NOT EXISTS FOR (n:Entity) ON (n.domain)")

        graphs = ["aletheia", "knowledge", "temporal_events"]
        total_imported = 0
        total_errors = 0

        for graph_name in graphs:
            imported, errors = import_graph(driver, graph_name)
            total_imported += imported
            total_errors += errors

        print(f"\nDone: {total_imported} edges imported, {total_errors} errors")

        with driver.session() as session:
            result = session.run("MATCH (n) RETURN count(n) AS nodes")
            nodes = result.single()["nodes"]
            result = session.run("MATCH ()-[r]->() RETURN count(r) AS rels")
            rels = result.single()["rels"]
            print(f"Neo4j totals: {nodes} nodes, {rels} relationships")
    finally:
        driver.close()


if __name__ == "__main__":
    main()
