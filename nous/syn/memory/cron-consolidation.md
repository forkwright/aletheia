# Automated Memory Consolidation Setup

**Created:** 2026-01-30  
**Status:** ✅ Deployed and Active  
**Cron Job ID:** `d59b00b8-cbaf-405d-a333-c08d88f8a40f`

## Overview

The automated memory consolidation system runs daily at 23:00 CST to extract, consolidate, and preserve insights from all agent workspaces. This ensures important decisions, preferences, and learnings are captured systematically without manual intervention.

## System Architecture

### Components

1. **`memory-consolidation` script** (`/mnt/ssd/moltbot/shared/bin/memory-consolidation`)
   - Main orchestration script
   - Processes all agent workspaces automatically
   - Generates consolidated daily reports

2. **`pre-compact` tool** (existing)
   - Enhanced context preservation before compaction
   - Extracts insights from conversation files
   - Creates structured summaries with importance weighting

3. **`extract-insights` tool** (existing)
   - Identifies decision points, preferences, and novel insights
   - Hierarchical summarization with confidence scoring
   - Outputs structured JSON for further processing

4. **`daily-facts` tool** (existing)
   - Automatic fact extraction from memory files
   - Updates facts.jsonl with new learnings
   - Pattern-based detection of key-value pairs

### Agent Workspaces Processed

| Agent | Workspace | Domain |
|-------|-----------|--------|
| **Syn** | `/mnt/ssd/moltbot/clawd` | Meta/orchestrator |
| **Chiron** | `/mnt/ssd/moltbot/chiron` | Work/technical |
| **Demiurge** | `/mnt/ssd/moltbot/demiurge` | Craft/research |
| **Eiron** | `/mnt/ssd/moltbot/eiron` | School/education |
| **Syl** | `/mnt/ssd/moltbot/syl` | Home/family |

## Cron Job Configuration

```bash
# Schedule: Daily at 23:00 CST
Schedule: "cron 0 23 * * * @ America/Chicago"
Target: main session
Agent: main
Payload: system event "Run automated memory consolidation: memory-consolidation"
```

### Timing Rationale

- **23:00 CST:** End of day when most activity has concluded
- **After active conversations:** Minimizes interruption risk
- **Before midnight:** Ensures proper date attribution
- **Same time as existing fact extraction:** Coordination with other daily processes

## Process Flow

```
23:00 CST Daily Trigger
    ↓
For each agent workspace:
    ↓
1. Check for conversation file
    ↓
2. Run pre-compact analysis
    ↓ 
3. Extract insights using extract-insights
    ↓
4. Generate workspace summary
    ↓
5. Update running statistics
    ↓
Next workspace...
    ↓
6. Run daily-facts extraction
    ↓
7. Generate consolidated report
    ↓
8. Save to memory/consolidation-YYYY-MM-DD.md
```

## Output Structure

Each consolidation generates:

### Primary Output
- **`memory/consolidation-YYYY-MM-DD.md`** - Master consolidation report
  - Overview and statistics
  - Per-agent insights and activity
  - Facts database updates
  - Files generated summary

### Per-Agent Outputs
- **`memory/compaction-YYYY-MM-DD_HH-MM.md`** - Individual workspace summaries
- **`memory/insights-YYYY-MM-DD_HH-MM.json`** - Structured insight extractions

### Logging
- **`/tmp/memory-consolidation-YYYY-MM-DD.log`** - Process execution log

## Key Features

### 🔍 Comprehensive Analysis
- Extracts decisions, preferences, and insights from conversation history
- Hierarchical summarization with importance weighting
- Cross-workspace pattern detection

### 📊 Quantified Tracking
- Conversation turn counts
- Decision point identification  
- Preference capture statistics
- New fact generation metrics

### 🤖 Fully Automated
- No manual intervention required
- Lightweight execution (no heavy agent spawning)
- Error-tolerant (continues if individual workspace fails)

### 🔗 Integration with Existing Systems
- Updates shared facts.jsonl database
- Coordinates with existing daily-fact-extraction
- Compatible with three-tier memory architecture

## Error Handling

The system is designed to be robust:

- **Missing conversation files:** Creates basic summary template
- **Tool availability:** Graceful degradation if tools missing
- **Workspace errors:** Continues processing other workspaces  
- **Insight extraction failures:** Falls back to pattern detection
- **Facts database issues:** Reports error but continues consolidation

## Monitoring

### Status Verification
```bash
# Check cron job status
clawdbot cron list | grep memory-consolidation

# View latest consolidation
ls -la memory/consolidation-*.md | tail -1

# Check execution logs
ls /tmp/memory-consolidation-*.log | tail -1
```

### Expected Metrics
- **Workspaces processed:** 5/5 (all agents)
- **Typical insights:** 5-15 per active workspace
- **Decision capture:** 1-5 per active workspace
- **New facts:** 2-10 per day depending on activity

## Maintenance

### Regular Tasks
- **Weekly:** Review consolidation quality and adjust extraction patterns
- **Monthly:** Clean up old log files from /tmp
- **Quarterly:** Evaluate consolidation effectiveness and update tools

### Tool Updates
When updating core tools (`pre-compact`, `extract-insights`), the cron job automatically uses the latest versions since they're referenced by name in the shared bin directory.

## Comparison with Manual Process

| Aspect | Manual (Previous) | Automated (Current) |
|--------|-------------------|---------------------|
| **Frequency** | As needed/remembered | Daily at 23:00 |
| **Coverage** | Single workspace | All 5 agent workspaces |
| **Consistency** | Variable quality | Standardized format |
| **Fact Updates** | Manual fact entry | Automatic extraction + updates |
| **Time Cost** | 15-30 minutes/session | Fully automated |
| **Human Error** | Possible missed insights | Pattern-based detection |

## Integration Points

### Three-Tier Memory System
- **Raw:** Daily files → enhanced by consolidation insights
- **Curated:** MEMORY.md → updated with consolidation findings  
- **Searchable:** Letta → populated via facts.jsonl updates

### Cross-Agent Coordination
- **Status monitoring:** Consolidation reports feed into agent status system
- **Insight sharing:** Cross-workspace patterns identified and documented
- **Decision tracking:** Important choices preserved across all domains

## Future Enhancements

### Planned Improvements
1. **Semantic clustering:** Group related insights across workspaces
2. **Trend detection:** Identify patterns over multiple days/weeks
3. **Priority scoring:** Weight consolidations by impact and urgency
4. **Integration with MCP Memory:** Feed insights into knowledge graph

### Advanced Features Under Consideration
- **Real-time insight capture:** Supplement daily batch with continuous monitoring
- **Cross-agent insight propagation:** Auto-share relevant insights between domains
- **Predictive consolidation:** Identify sessions likely to need consolidation
- **Quality scoring:** Rate consolidation completeness and accuracy

---

## Commands Reference

```bash
# Manual execution (for testing)
memory-consolidation

# Check cron status  
clawdbot cron list | grep memory-consolidation

# View latest output
cat memory/consolidation-$(date +%Y-%m-%d).md

# Check logs
tail /tmp/memory-consolidation-$(date +%Y-%m-%d).log

# Disable/enable cron
clawdbot cron disable memory-consolidation  
clawdbot cron enable memory-consolidation
```

## Validation Checklist

- [✅] Script created and executable: `/mnt/ssd/moltbot/shared/bin/memory-consolidation`
- [✅] Cron job scheduled: Daily 23:00 CST 
- [✅] All agent workspaces configured: clawd, chiron, demiurge, eiron, syl
- [✅] Integration with existing tools: pre-compact, extract-insights, daily-facts
- [✅] Error handling implemented: Graceful degradation on failures
- [✅] Output documentation: Structured consolidation reports
- [✅] Logging configured: Process execution tracking

**Status: Fully deployed and operational** ✅