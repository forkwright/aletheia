#!/bin/bash
# heartbeat-research: Run during prosoche/heartbeat instead of system checks
# Pulls court records, cross-references graph, archives sources
set -euo pipefail

LOG="/mnt/ssd/aletheia/nous/syn/research/.research-log.jsonl"
TOOLS="/mnt/ssd/aletheia/nous/syn/research/tools"

timestamp() { date -u +%Y-%m-%dT%H:%M:%SZ; }

echo "{\"ts\":\"$(timestamp)\",\"action\":\"heartbeat-research-start\"}" >> "$LOG"

# 1. Pull recent court filings across domains
echo "=== PULLING COURT RECORDS ==="

# Epstein-related
EPSTEIN_COUNT=$(curl -sL "https://www.courtlistener.com/api/rest/v4/search/?q=epstein+maxwell&type=r&order_by=dateFiled+desc&filed_after=$(date -d '7 days ago' +%Y-%m-%d)" \
  -H "User-Agent: Aletheia Research" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('count',0))" 2>/dev/null || echo 0)

# ICE Minnesota
ICE_COUNT=$(curl -sL "https://www.courtlistener.com/api/rest/v4/search/?q=ICE+detention+minnesota&type=r&order_by=dateFiled+desc&filed_after=$(date -d '7 days ago' +%Y-%m-%d)" \
  -H "User-Agent: Aletheia Research" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('count',0))" 2>/dev/null || echo 0)

# DOGE lawsuits
DOGE_COUNT=$(curl -sL "https://www.courtlistener.com/api/rest/v4/search/?q=DOGE+%22Department+of+Government+Efficiency%22&type=r&order_by=dateFiled+desc&filed_after=$(date -d '7 days ago' +%Y-%m-%d)" \
  -H "User-Agent: Aletheia Research" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('count',0))" 2>/dev/null || echo 0)

echo "{\"ts\":\"$(timestamp)\",\"epstein_filings_7d\":$EPSTEIN_COUNT,\"ice_mn_filings_7d\":$ICE_COUNT,\"doge_filings_7d\":$DOGE_COUNT}" >> "$LOG"

# 2. Graph stats
GRAPH_NODES=$(docker exec -i falkordb redis-cli GRAPH.QUERY aletheia "MATCH (n) WHERE n.domain = 'power' RETURN count(n)" 2>/dev/null | grep -oP '^\d+' | head -1 || echo 0)
GRAPH_EDGES=$(docker exec -i falkordb redis-cli GRAPH.QUERY aletheia "MATCH ()-[r]->() WHERE startNode(r).domain = 'power' RETURN count(r)" 2>/dev/null | grep -oP '^\d+' | head -1 || echo 0)

echo "{\"ts\":\"$(timestamp)\",\"graph_power_nodes\":$GRAPH_NODES,\"graph_power_edges\":$GRAPH_EDGES}" >> "$LOG"

echo "Court filings (7d): Epstein=$EPSTEIN_COUNT ICE/MN=$ICE_COUNT DOGE=$DOGE_COUNT"
echo "Power graph: $GRAPH_NODES nodes, $GRAPH_EDGES edges"

echo "{\"ts\":\"$(timestamp)\",\"action\":\"heartbeat-research-complete\"}" >> "$LOG"
