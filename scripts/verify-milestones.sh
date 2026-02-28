#!/usr/bin/env bash
# Verify all 18 milestone hashes (15 milestones, some with range endpoints) resolve in the repo

HASHES=(
  71d282fb 183a73ba d07e3e4f c10b7bc0 99bbf534 5fac8ca5 c1a92caa
  97d7e3fa 5a43dad9 7dcf8225 68b5f941 2bda0468 be86c09b
  93f0e442 5ab72514 b8746254 2c844d27 76fc62fb
)

TOTAL=${#HASHES[@]}
VERIFIED=0
MISSING=0

for hash in "${HASHES[@]}"; do
  result=$(git log --oneline -1 "$hash" 2>/dev/null)
  if [[ -n "$result" ]]; then
    echo "OK: $hash $result"
    ((VERIFIED++))
  else
    echo "MISSING: $hash"
    ((MISSING++))
  fi
done

echo ""
echo "$VERIFIED/$TOTAL milestones verified"

[[ $MISSING -eq 0 ]] && exit 0 || exit 1
