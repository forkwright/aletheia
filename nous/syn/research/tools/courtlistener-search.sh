#!/bin/bash
# courtlistener-search: Search federal court records via CourtListener API
# Usage: courtlistener-search <query> [court]
# Free API, no key needed for basic search

set -euo pipefail

QUERY="$1"
COURT="${2:-}"  # e.g., "flsd" for Southern District of Florida

ENCODED=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$QUERY'))")

if [ -n "$COURT" ]; then
    URL="https://www.courtlistener.com/api/rest/v4/search/?q=${ENCODED}&court=${COURT}&type=r&order_by=score+desc"
else
    URL="https://www.courtlistener.com/api/rest/v4/search/?q=${ENCODED}&type=r&order_by=score+desc"
fi

echo "Searching CourtListener: $QUERY"
curl -sL "$URL" -H "User-Agent: Aletheia Research" | python3 -c "
import json, sys
data = json.load(sys.stdin)
for r in data.get('results', [])[:10]:
    print(f\"  {r.get('dateFiled', 'n/d')} | {r.get('caseName', 'Unknown')} | {r.get('court', '')}\")
    print(f\"    {r.get('snippet', '')[:200]}\")
    print()
" 2>/dev/null || echo "Parse error — raw URL: $URL"
