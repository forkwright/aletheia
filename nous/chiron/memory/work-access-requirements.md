# Work System Access Requirements

**Date:** 2026-01-29
**Critical Directive from Cody:** Work Claude Code (not Chiron directly) must handle all work system connections.

## Access Architecture

```
Chiron (orchestrator) 
   ↓ controls via tmux
Work Claude Code Session on Metis
   ↓ READ ONLY access to:
   • Work Slack 
   • GitHub
   • AWS
```

## Current Status - NEEDS SETUP

### Required READ ONLY Connections for Work Claude Code:
1. **Work Slack** - READ ONLY
   - Channel monitoring
   - Message history
   - Status awareness
   
2. **GitHub** - READ ONLY  
   - Repository access
   - Issue tracking
   - PR status
   
3. **AWS** - READ ONLY
   - Resource monitoring
   - Service status
   - Cost awareness

### Current MCP Configuration
Based on ~/.claude/mcp.json:
- ✅ Filesystem (summus directory)
- ✅ SQLite (tasks.db) 
- ✅ Git operations
- ❌ Slack MCP - NOT CONFIGURED
- ❌ GitHub MCP - NOT CONFIGURED  
- ❌ AWS MCP - NOT CONFIGURED

### Next Steps (APPROVED for READ ONLY access)
1. ⚠️ Fix work Claude Code session responsiveness (still hanging on commands)
2. ✅ Configure Slack MCP for READ ONLY access (APPROVED)
3. ✅ Configure GitHub MCP for READ ONLY access (APPROVED)
4. ✅ Configure AWS MCP for READ ONLY access (APPROVED)  
5. Test all connections through work Claude Code
6. Document access patterns for Chiron orchestration
7. Implement first weekly status report format

### Security Boundaries & Visibility Model
- **Chiron:** NO direct work system access
- **Work Claude Code:** READ ONLY work system access via LOCAL MCP only
- **Context Isolation:** Work Claude Code can ONLY access local Metis context
- **No Cross-System Reach:** Cannot access other systems unless locally configured
- **Company Visibility:** Claude Code usage IS visible to Anthropic/company
- **All write operations:** Require explicit human approval  
- **Proper isolation:** Personal and work contexts separated

### Security Clarification from Cody (2026-01-29):
- ✅ READ ONLY access to work systems is fine and allowed
- ✅ Work has prompt review turned off (less visibility concern)  
- ✅ Can proceed with Slack/GitHub/AWS MCP configuration for READ ONLY
- ✅ Local context on Metis + approved work system READ ONLY access