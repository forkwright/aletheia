#!/bin/bash
# sec-lookup: Search SEC EDGAR for financial filings
# Usage: sec-lookup <company_or_person> [filing_type]

set -euo pipefail

QUERY="$1"
TYPE="${2:-}"  # 10-K, DEF 14A, SC 13D, etc.

ENCODED=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$QUERY'))")

if [ -n "$TYPE" ]; then
    URL="https://efts.sec.gov/LATEST/search-index?q=${ENCODED}&dateRange=custom&startdt=2020-01-01&forms=${TYPE}"
else
    URL="https://efts.sec.gov/LATEST/search-index?q=${ENCODED}&dateRange=custom&startdt=2020-01-01"
fi

echo "Searching SEC EDGAR: $QUERY (type: ${TYPE:-all})"
curl -sL "https://efts.sec.gov/LATEST/search-index?q=${ENCODED}" \
    -H "User-Agent: Aletheia Research research@localhost" | python3 -m json.tool 2>/dev/null | head -50

echo ""
echo "Full text search: https://efts.sec.gov/LATEST/search-index?q=${ENCODED}"
echo "EDGAR company search: https://www.sec.gov/cgi-bin/browse-edgar?company=${ENCODED}&CIK=&type=&dateb=&owner=include&count=40&search_text=&action=getcompany"
