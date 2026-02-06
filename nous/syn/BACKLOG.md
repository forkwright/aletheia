# BACKLOG.md — Pending Setups & Ideas

## Architecture (High Priority)

| Issue | Problem | Status |
|-------|---------|--------|
| Context loading | Full chat loaded every prompt | ✅ Compaction tuned (50k reserve) |
| Self-recovery | Service breaks, lose access | ✅ Watchdog, auto-restart, Tailscale webchat fallback |
| Tailscale + Mullvad | Coexistence on Metis | ✅ Applied via nftables |
| **Agent architecture** | Domain separation | ✅ 5 agents: Syn, Syl, Chiron (Opus), Eiron, Demiurge |
| **Work Slack access** | Read-only context | ✅ Chiron via tmux → Claude Code on Metis |

## Infrastructure (To Set Up)

| Item | Priority | Status |
|------|----------|--------|
| Proton email via himalaya | Medium | ⏳ hydroxide ready, awaiting creds |
| ~~Google Calendar~~ | ✅ Done | `gcal` wrapper with read/write |
| ~~Brave Search API~~ | Low | Have Perplexity instead |
| ~~Perplexity API~~ | ✅ Done | `pplx "query"` + `research` wrapper |
| ~~coding-agent skill~~ | ✅ Done | `coding-agent send/status/output` |
| blogwatcher skill | Low | RSS monitoring |
| ~~summarize skill~~ | ✅ Done | Research, podcasts |

## Ecosystem Improvements (2026-01-29 Retrospective)

### Completed ✅
| Item | Description |
|------|-------------|
| Shared bin/ | All agents symlink to `shared/bin/` |
| 30-min heartbeats | Reduced from 15min, less noise |
| Auto-status | `bin/auto-status` generates from filesystem activity |
| Pre-compact | `bin/pre-compact` dumps context before compaction |
| Shared insights/ | `shared/insights/` for cross-agent learning |
| Smarter HEARTBEAT.md | Productive checklist, not just acks |

### Pending
| Item | Priority | Description |
|------|----------|-------------|
| Adaptive heartbeat frequency | Low | Check more when active, less when quiet |
| ~~PATH fix for shared bin~~ | ✅ Done | Ensure `bin/` in PATH for all shells |

## Research-Driven Ideas (2026-01-29)

### Multi-Agent Architecture
| Idea | Priority | Description | Source |
|------|----------|-------------|--------|
| ~~Blackboard architecture~~ | ✅ Done | Shared knowledge space where agents post observations, plans, partial results. Others pick them up. 13-57% better task success than direct messaging. | Perplexity research |
| Structured task decomposition | Medium | Store plans as data (JSON/graph) not just prompts. Inspectable, modifiable, approvable. | Perplexity research |
| Hierarchical orchestration | Low | Mid-tier coordinators for domain groups if we scale further | Perplexity research |
| ~~Contract-like agent interfaces~~ | ✅ Done | Define inputs/outputs/SLAs for each agent formally | Perplexity research |

### Memory Systems
| Idea | Priority | Description | Source |
|------|----------|-------------|--------|
| ~~Atomic fact extraction~~ | ✅ Done | Auto-extract facts after sessions (not just summaries). `[fact] Cody prefers X` becomes queryable. | Perplexity research |
| Iterative retrieval | Medium | Multi-step memory queries: structured → semantic → refine → re-rank | Perplexity research |
| ~~Event graphs~~ | ✅ Done | Track events as linked graph, not linear timestamped notes. Better "when did X" and "what before Y" queries. | Perplexity research |
| ~~Memory consolidation automation~~ | ✅ Done | Periodic job to promote significant daily items to MEMORY.md, extract facts to Letta | Perplexity research |
| Typed memory entries | Low | Schema for entries: Observation, Plan, ToolResult, etc. with source/confidence/timestamp | Perplexity research |

### Claude Tooling / MCP
| Idea | Priority | Description | Source |
|------|----------|-------------|--------|
| ~~Explore MCP ecosystem~~ | ✅ Done | 17,000+ servers exist. Could replace custom tools with standard protocol. | Perplexity research |
| Gmail MCP server | Medium | Standard email integration vs custom | mcpservers.org |
| Notion/Obsidian MCP | Medium | Knowledge base integration | mcpservers.org |
| Slack MCP server | Medium | Work chat integration for Chiron | mcpservers.org |
| Custom Memory MCP server | Low | Expose our hybrid memory via MCP protocol | Perplexity research |
| Claude Agent SDK patterns | Medium | More structured sub-agent coordination | Anthropic docs |

### Operational
| Idea | Priority | Description | Source |
|------|----------|-------------|--------|
| Cross-agent task handoff protocol | Medium | Formal way to pass tasks between agents with context | Day 2 observation |
| Agent collaboration patterns | Low | Document how agents should interact (when to @mention, when to share insights) | Day 2 observation |
| Compaction summary richness | Low | Better pre-compaction dumps with structured sections | Day 2 observation |

## Patterns from Dianoia Inbox

**Natural observation capture:**
- Listen for: "I was thinking...", "wild idea...", "wouldn't it be cool if..."
- Listen for: "this is annoying...", friction indicators
- Listen for: "we should...", future ideas
- Capture repeated corrections (implicit friction)
- Surface observations at session end

**Ideas to track (from 2026-01-15):**
- dianoia-template repo for GitHub portfolio
- Claude Code fork for fish default (low priority)
- General idea tracking — frames have value even without plans

## Routines to Build

- [x] morning-brief — Weather, calendar, tasks, agents (sub-agent building)
- [ ] pr-review.md — Checklist for reviewing agent PRs
- [x] inbox-triage — Route dianoia inbox to domain agents (sub-agent building)
- [x] weekly-review — Memory consolidation, state sync (done)

## Career Context (Reference)

From career audit 2026-01-17:
- Title: Data Scientist & AI Systems (undersells capability)
- Staff-level scope: Medical taxonomy = organizational IP
- AI fluency differentiator: 141M tokens/day
- Leadership: USMC (157 Marines) underleverage
- GitHub needs polish for portfolio

## Health Awareness

**Moved to Letta memory** (2026-01-28) — query via `letta ask "health"`

---

*Updated: 2026-01-29*
