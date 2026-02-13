#!/bin/bash
# epstein-fetch: Fetch and index documents from the DOJ Epstein portal
# Usage: epstein-fetch <dataset_number> [search_term]

set -euo pipefail

DATASET="${1:-}"
SEARCH="${2:-}"
BASE_URL="https://www.justice.gov/epstein"
INDEX_DIR="/mnt/ssd/aletheia/nous/syn/research/epstein/raw"
mkdir -p "$INDEX_DIR"

if [ -z "$DATASET" ]; then
    echo "Fetching DOJ Epstein portal index..."
    curl -sL "$BASE_URL" | grep -oP 'href="[^"]*epstein/files[^"]*"' | sort -u > "$INDEX_DIR/portal-links.txt"
    echo "Found $(wc -l < "$INDEX_DIR/portal-links.txt") document links"
    cat "$INDEX_DIR/portal-links.txt" | head -20
else
    echo "Fetching Dataset $DATASET index..."
    curl -sL "${BASE_URL}/files" | grep -oP "DataSet%20${DATASET}[^\"]*" | sort -u > "$INDEX_DIR/dataset-${DATASET}-links.txt"
    COUNT=$(wc -l < "$INDEX_DIR/dataset-${DATASET}-links.txt")
    echo "Found $COUNT files in Dataset $DATASET"
    
    if [ -n "$SEARCH" ]; then
        echo "Searching for '$SEARCH' in Dataset $DATASET..."
        grep -i "$SEARCH" "$INDEX_DIR/dataset-${DATASET}-links.txt" || echo "No matches"
    fi
fi
