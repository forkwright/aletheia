#!/bin/bash
# Update hardcoded paths in shared/bin scripts
# Run AFTER filesystem migration

set -e
cd /mnt/ssd/aletheia/shared/bin

echo "=== Updating script paths ==="

# Scripts that need /mnt/ssd/aletheia/clawd → /mnt/ssd/aletheia/agents/syn
for script in agent-audit agent-health bulk-extract-facts consolidate-memory \
              daily-facts extract-facts extract-insights generate-dashboard \
              graph graph-agent graph-sync heartbeat-tracker index-gdrive \
              letta-populate mine-memory-facts moltbook-feed monitoring-cron \
              pre-compact predictive-context recall reflect restore-autarkia \
              restore-clawdbot self-audit update-llms-txt; do
  if [ -f "$script" ]; then
    echo "Updating $script..."
    sed -i 's|/mnt/ssd/aletheia/clawd|/mnt/ssd/aletheia/agents/syn|g' "$script"
  fi
done

# Update dianoia references → projects
for script in dianoia-sync; do
  if [ -f "$script" ]; then
    echo "Updating $script..."
    sed -i 's|/mnt/ssd/aletheia/dianoia|/mnt/ssd/aletheia/projects|g' "$script"
  fi
done

echo "=== Done ==="
echo "Verify with: grep -r 'moltbot/clawd' /mnt/ssd/aletheia/shared/bin/"
