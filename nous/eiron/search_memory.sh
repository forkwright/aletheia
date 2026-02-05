#!/bin/bash
# Eiron memory search - grep-based intelligence

QUERY="$1"
if [ -z "$QUERY" ]; then
    echo "Usage: $0 <search_term>"
    echo "Example: $0 'aaron|phase|capstone'"
    exit 1
fi

echo "=== Searching Eiron Memory ==="
echo "Query: $QUERY"
echo

# Search Eiron memory system
echo "🧠 Memory system:"
grep -ri "$QUERY" /mnt/ssd/aletheia/eiron/MEMORY.md /mnt/ssd/aletheia/eiron/memory/ 2>/dev/null | head -10

echo
echo "📁 Other Eiron files:"
grep -ri "$QUERY" /mnt/ssd/aletheia/eiron/*.md 2>/dev/null | grep -v MEMORY.md | head -5

echo
echo "📚 MBA Capstone files:"
grep -ri "$QUERY" /mnt/ssd/aletheia/clawd/mba/sp26/capstone/ 2>/dev/null | head -10

echo
echo "📝 Recent status files:"
find /mnt/ssd/aletheia/eiron/ -name "*.md" -mtime -7 -exec grep -l "$QUERY" {} \; 2>/dev/null