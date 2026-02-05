# Agent Health Monitoring Implementation
*Implemented: 2026-01-30*
*By: Subagent for main/Syn*

## Overview

Built a complete agent health monitoring infrastructure for the 5-agent ecosystem (Syn, Syl, Chiron, Eiron, Demiurge). Simple, shell-based approach with no external dependencies as requested.

## Components Built

### 1. `/mnt/ssd/moltbot/shared/bin/agent-health` - Core CLI Tool

**Purpose:** Monitor health status of all agents

**Features:**
- ✅ Check last activity for each agent
- ✅ Count session files per agent
- ✅ Token usage estimation (24h rolling window)
- ✅ Error rate analysis from session logs
- ✅ Agent status from status files
- ✅ JSON and table output formats
- ✅ Blackboard blocked task monitoring

**Usage:**
```bash
agent-health                    # Quick overview
agent-health --agent syn        # Check specific agent
agent-health --json --full      # Complete data in JSON
```

**Data Sources:**
- Session transcripts: `~/.clawdbot/agents/*/sessions/*.md`
- Agent status files: `/mnt/ssd/moltbot/clawd/agent-status/*.md`
- Blackboard: via `bb status` command
- File modification times for activity tracking

### 2. `/mnt/ssd/moltbot/shared/bin/generate-dashboard` - Dashboard Generator

**Purpose:** Auto-generate markdown status dashboard

**Features:**
- ✅ Comprehensive agent status table with emojis
- ✅ System health (memory, disk, load, services)
- ✅ Recent activity from heartbeat tracking
- ✅ Blackboard task status
- ✅ Alert detection (stale agents, high error rates)
- ✅ Gateway status monitoring

**Output:** `/mnt/ssd/moltbot/clawd/agent-status/dashboard.md`

**Status Indicators:**
- 🟢 Active (updated <1h ago)
- 🟡 Idle (updated 1-24h ago)  
- 🔴 Stale (updated >24h ago)
- ⚪ Unknown (no status file)

### 3. `/mnt/ssd/moltbot/shared/bin/heartbeat-tracker` - Heartbeat System

**Purpose:** Track periodic checks and update heartbeat state

**Features:**
- ✅ Configurable check intervals per service
- ✅ JSON state persistence in `memory/heartbeat-state.json`
- ✅ Agent health monitoring
- ✅ System resource monitoring
- ✅ Blackboard monitoring
- ✅ Extensible framework for email/calendar/weather checks

**Check Types:**
- `agents` - Agent health (30min interval)
- `system` - Memory/disk/services (10min interval)
- `blackboard` - Blocked tasks (15min interval)
- `email` - Email check (1hr interval) *placeholder*
- `calendar` - Calendar check (30min interval) *placeholder*
- `weather` - Weather check (2hr interval) *placeholder*

### 4. Enhanced Heartbeat State (`memory/heartbeat-state.json`)

**Structure:**
```json
{
  "lastHeartbeat": "2026-01-30T18:09:37-06:00",
  "agents": "green",
  "infra": "green",
  "lastChecks": {
    "email": null,
    "calendar": null,
    "weather": null,
    "agents": 1769818177,
    "blackboard": null,
    "system": 1769818183
  },
  "alerts": {
    "active": [],
    "history": []
  },
  "metrics": {
    "totalSessions": 0,
    "avgResponseTime": 0,
    "errorCount": 0,
    "lastAgentActivity": {
      "syn": 0,
      "syl": 0,
      "chiron": 0,
      "eiron": 0,
      "demiurge": 0
    }
  }
}
```

## Technical Implementation Details

### Data Collection Strategy

**Agent Activity Detection:**
- Scans `~/.clawdbot/agents/*/sessions/` for recent `.md` files
- Uses file modification times to determine last activity
- Counts total session files as activity indicator

**Error Rate Analysis:**
- Searches session logs for error patterns: "error", "failed", "exception", "timeout"
- Calculates percentage over 24h rolling window
- Uses `bc` for decimal precision in calculations

**Token Usage Estimation:**
- Counts characters in recent session files
- Rough estimate: 4 characters per token (industry standard)
- 24h rolling window for current usage trends

**System Health Monitoring:**
- Memory usage via `free` command
- Disk usage via `df /mnt/ssd/moltbot`
- Service status via `systemctl is-active`
- Load average from `uptime`

### Output Formats

**JSON Format:**
- Machine-readable for integration with other tools
- Timestamp included for freshness tracking
- All numeric values preserved for calculations

**Table Format:**
- Human-readable terminal output
- Formatted columns with headers
- Summary information and tips

### Alert Thresholds

**System Alerts:**
- Memory >90% = Red, >80% = Yellow
- Disk >90% = Red, >80% = Yellow
- Gateway down = Red
- >5 blocked tasks = Red, >3 = Yellow

**Agent Alerts:**
- No activity >24h = Stale
- Error rate >5% = High error rate
- Agent status file missing = Unknown

## Integration Points

### With Existing Systems

**Taskwarrior Integration:**
- Can be extended to create tasks for issues
- Blocked task monitoring via blackboard

**Memory System Integration:**
- Heartbeat state persisted in `memory/` directory
- Can write alerts to daily memory files

**Agent Status Integration:**
- Reads existing agent status files
- Can trigger status updates

### With Shared Infrastructure

**Blackboard Integration:**
- Uses `bb list` command for task monitoring
- Detects blocked, pending, and active tasks

**Service Integration:**
- Monitors clawdbot, ollama, docker services
- Gateway health through systemctl

## Usage Examples

### Quick Health Check
```bash
agent-health
# Shows table format with all agents
```

### Generate Dashboard
```bash
generate-dashboard > /path/to/dashboard.md
# Creates comprehensive status page
```

### Run Periodic Checks
```bash
heartbeat-tracker all
# Runs all configured health checks
```

### Check Specific Components
```bash
heartbeat-tracker agents    # Just agent health
heartbeat-tracker system    # Just system health
heartbeat-tracker summary   # Show current state
```

## Performance Considerations

**Efficient File Scanning:**
- Uses `find` with time filters to minimize I/O
- Only scans recent files for error analysis
- Caches session counts to avoid repeated scans

**Rate Limiting:**
- Heartbeat tracker respects check intervals
- Prevents excessive API calls and disk I/O
- Uses `--force` flag to override intervals when needed

**Resource Usage:**
- All shell-based, minimal memory footprint
- No persistent processes or daemons
- Fast execution times (<5 seconds for full checks)

## Future Enhancements

### Phase 1 Extensions
1. **Email Integration** - Connect to Gmail API for inbox monitoring
2. **Calendar Integration** - Use existing `gcal` tool for event monitoring
3. **Weather Integration** - Add weather API for location-based alerts

### Phase 2 Features
1. **Trend Analysis** - Track metrics over time
2. **Predictive Alerts** - Detect degradation patterns
3. **Auto-Recovery** - Automated healing for common issues
4. **Performance Metrics** - Response time tracking

### Phase 3 Advanced Features
1. **Cross-Agent Correlation** - Detect ecosystem-wide issues
2. **Machine Learning** - Anomaly detection for complex patterns
3. **Integration Monitoring** - External service health (OpenAI, etc.)
4. **Load Balancing** - Dynamic task distribution based on agent health

## Testing & Validation

### Verified Functions
- ✅ Agent activity detection
- ✅ Session file counting
- ✅ Error rate calculation
- ✅ Token usage estimation
- ✅ System resource monitoring
- ✅ JSON output formatting
- ✅ Dashboard generation
- ✅ Heartbeat state persistence

### Edge Cases Handled
- ✅ Missing session directories
- ✅ Empty session files
- ✅ Division by zero in error calculations
- ✅ Missing blackboard tool
- ✅ Service availability checks
- ✅ File permission issues

### Known Limitations
- Token estimation is approximate (actual tokenization varies)
- Error detection uses simple keyword matching
- Requires shell environment with standard Unix tools
- Blackboard integration requires `bb` tool availability

## Maintenance

### Regular Tasks
- Monitor log file growth in session directories
- Validate heartbeat state JSON integrity
- Check dashboard generation frequency
- Update alert thresholds based on usage patterns

### Troubleshooting
- Check file permissions on session directories
- Verify `jq`, `bc`, and other tool availability
- Monitor dashboard generation for stale data
- Validate JSON output formatting

---

**Status:** ✅ Implementation Complete
**Next Steps:** Begin Phase 1 enhancements (email, calendar, weather integration)
**Integration:** Ready for production use with existing agent ecosystem