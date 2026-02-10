#!/bin/bash
# graph-add: Add a node or relationship to the power graph
# Usage: 
#   graph-add node <type> <name> <role>
#   graph-add edge <from_name> <rel_type> <to_name> [properties_json]

set -euo pipefail

ACTION="$1"

if [ "$ACTION" = "node" ]; then
    TYPE="$2"  # Person, Institution, Event
    NAME="$3"
    ROLE="${4:-}"
    docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
        "MERGE (n:${TYPE} {name: '${NAME}'}) SET n.role = '${ROLE}', n.domain = 'power' RETURN n.name"
    echo "Node: $TYPE '$NAME' ($ROLE)"

elif [ "$ACTION" = "edge" ]; then
    FROM="$2"
    REL="$3"
    TO="$4"
    PROPS="${5:-}"
    if [ -n "$PROPS" ]; then
        docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
            "MATCH (a {name: '${FROM}'}), (b {name: '${TO}'}) CREATE (a)-[:${REL} ${PROPS}]->(b) RETURN a.name, type(r), b.name"
    else
        docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
            "MATCH (a {name: '${FROM}'}), (b {name: '${TO}'}) CREATE (a)-[:${REL}]->(b)"
    fi
    echo "Edge: $FROM -[$REL]-> $TO"

elif [ "$ACTION" = "query" ]; then
    QUERY="$2"
    docker exec -i falkordb redis-cli GRAPH.QUERY aletheia "$QUERY"

elif [ "$ACTION" = "paths" ]; then
    # Find all paths between two people up to N hops
    FROM="$2"
    TO="$3"
    HOPS="${4:-3}"
    docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
        "MATCH path = (a {name: '${FROM}'})-[*1..${HOPS}]-(b {name: '${TO}'}) RETURN [n in nodes(path) | n.name] as names, [r in relationships(path) | type(r)] as rels"

elif [ "$ACTION" = "stats" ]; then
    echo "=== Power Graph Stats ==="
    docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
        "MATCH (n) WHERE n.domain = 'power' RETURN labels(n)[0] as type, count(n) as count ORDER BY count DESC"
    echo ""
    docker exec -i falkordb redis-cli GRAPH.QUERY aletheia \
        "MATCH ()-[r]->() WHERE startNode(r).domain = 'power' OR endNode(r).domain = 'power' RETURN type(r) as rel, count(r) as count ORDER BY count DESC"
fi
