#!/bin/bash
# archive-fetch: Fetch a URL and save to Wayback Machine + local archive
# Usage: archive-fetch <url> <local_name> [domain]

set -euo pipefail

URL="$1"
NAME="${2:-$(echo "$URL" | md5sum | cut -d' ' -f1)}"
DOMAIN="${3:-general}"
ARCHIVE_DIR="/mnt/ssd/aletheia/nous/syn/research/.archive/$DOMAIN"
mkdir -p "$ARCHIVE_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
OUTFILE="$ARCHIVE_DIR/${NAME}_${TIMESTAMP}.html"

# Save locally
curl -sL "$URL" -o "$OUTFILE" 2>/dev/null && echo "Local: $OUTFILE" || echo "WARN: Local fetch failed"

# Submit to Wayback Machine (non-blocking)
curl -s "https://web.archive.org/save/$URL" -o /dev/null 2>/dev/null &
echo "Wayback: submitted $URL"

# Log
echo "$TIMESTAMP|$URL|$OUTFILE|$DOMAIN" >> "$ARCHIVE_DIR/../archive-log.csv"
echo "Archived: $NAME ($DOMAIN)"
