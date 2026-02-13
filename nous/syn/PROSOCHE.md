## Domain Checks
- **First: Check ALERTS.md** — if it exists, address those alerts before anything else
- Check crewai-alerts.json for unacknowledged alerts
- Verify signal-cli daemon is alive
- Check nous-health for unhealthy agents
- Check blackboard for unclaimed tasks
- Review nous-status/*.md for blocked items

## Research (Priority)
When no alerts need attention, use heartbeat time for research:
1. Run `research/tools/heartbeat-research.sh` for court record counts
2. Pull newest filings from CourtListener across domains (Epstein, ICE, DOGE)
3. Cross-reference new names against power graph in FalkorDB
4. Archive any sources at risk of disappearing
5. Add new nodes/edges to graph when connections found
6. Log findings to research/.research-log.jsonl
7. Read EVOLUTION.md — check yourself against it