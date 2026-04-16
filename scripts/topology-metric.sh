#!/usr/bin/env bash
set -euo pipefail

# Count workspace crates (nodes) and inter-crate dependencies (edges)
# from cargo metadata.

metadata=$(cargo metadata --format-version=1 --no-deps)

crates=$(echo "$metadata" | jq '[.packages[]] | length')

edges=$(echo "$metadata" | jq '
  .packages
  | map(.dependencies // [] | map(select(.path != null)) | length)
  | add
')

printf "crates=%d, edges=%d, ratio=%.2f\n" "$crates" "$edges" "$(echo "scale=2; $edges / $crates" | bc)"
