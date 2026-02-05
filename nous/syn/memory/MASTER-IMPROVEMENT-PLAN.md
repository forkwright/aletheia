# Master System Improvement Plan
*Generated: 2026-01-30 from 6 parallel audits*

---

## 🔴 CRITICAL (Fix This Week)

| Issue | Domain | Effort | Impact |
|-------|--------|--------|--------|
| **Kendall can't DM Syl** | UX | 30 min | High |
| **Email completely broken** | Integrations | 1-2 hr | High |
| **Security: ~/.clawdbot permissions** | Infrastructure | 5 min | Critical |
| **75% of Clawdbot skills missing** | Infrastructure | 2-4 hr | Medium |

### Quick Wins (< 1 hour each)
1. `chmod 700 ~/.clawdbot` — security fix
2. Add Kendall's Signal UUID binding for Syl DMs
3. Start Proton Bridge or reconfigure email

---

## 🟡 HIGH PRIORITY (Next 2 Weeks)

### Coordination Gaps
- **CrewAI routing miscalibration** — "SQL" routes to Syn not Chiron
- **Blackboard underutilized** — infrastructure exists, agents don't use it
- **Status files go stale** — no automated monitoring
- **Cross-agent awareness** — agents don't know what others are working on

### Memory Silos
- **Knowledge trapped in domains** — Demi's Ardent knowledge invisible to Syn
- **Letta unevenly populated** — Demiurge rich, others sparse
- **MCP Memory unused** — only 5 entities despite full setup
- **No automated compaction extraction** — insights lost to context limits

### Workflow Gaps
- **Taskwarrior config fragmentation** — using Syl's config globally
- **Weekly reviews not automated** — documented but manual
- **No agent health alerts** — blocked agents not escalated

---

## 📋 IMPROVEMENT PHASES

### Phase 1: Critical Fixes (Week 1)
- [x] Security permissions (2026-02-03)
- [ ] Kendall/Syl binding
- [ ] Email restoration
- [x] CrewAI routing calibration (2026-01-31)

### Phase 2: Coordination (Week 2-3)
- [x] Activate blackboard usage in agent protocols (2026-02-03)
- [x] Add status freshness monitoring to heartbeats (2026-02-03)
- [x] Implement cross-agent memory query tool → memory-router v2 (2026-02-03)
- [x] Fix taskwarrior global vs domain config (2026-02-03)

### Phase 3: Memory (Week 3-4)
- [x] Auto-extract facts from compaction → consolidate-memory cron (2026-02-03)
- [x] Cross-agent Letta query tool (2026-02-03)
- [x] Systematic Letta population protocol (2026-02-03)
- [x] MCP Memory integration pipeline → populate-mcp tool (2026-02-03)

### Phase 2.5: Infrastructure (Added 2026-02-03)
- [x] Temporal event graph for temporal reasoning
- [x] Agent contracts for all 7 agents
- [x] Iterative memory retrieval with synonym expansion

### Phase 4: Integrations (Month 2)
- [ ] Social media APIs (Twitter, Discord)
- [ ] Productivity tools (Notion, Todoist)
- [ ] Missing Clawdbot skill dependencies

---

## 🔬 RESEARCH PROMPTS (Ready for Perplexity)

### Multi-Agent Coordination
> "What are the most effective patterns for automated task handoff between specialized AI agents? Focus on: formal protocols for task delegation, state preservation during handoffs, conflict resolution when agents need the same resources, and SLA monitoring for inter-agent dependencies."

### Memory Architecture
> "Research the latest approaches to synchronized memory systems in multi-agent AI architectures. Focus on: knowledge graph unification strategies, conflict resolution in distributed memory, cross-agent knowledge transfer mechanisms, and maintaining consistency across specialized agents."

### Context Preservation
> "Investigate state-of-the-art techniques for preserving semantic context during AI conversation compaction and summarization. Focus on: hierarchical summarization with importance weighting, semantic embedding preservation through compression, automated extraction of reasoning chains."

### Neurodivergent UX
> "What are the best practices for designing multi-agent AI systems that minimize cognitive overhead for neurodivergent users? Focus on agent handoffs, context preservation, and routing transparency for ADHD users with high context-switching costs."

### Email/Integrations
> "What are the best practices for AI assistant email integration in 2026? Compare Proton Bridge, direct IMAP/SMTP, Gmail API, and Outlook Graph API for reliability, security, and automation capabilities."

### Agent Scaling
> "How do organizations scale from 5-10 specialized AI agents to larger ecosystems without losing coordination effectiveness? Cover: hierarchical vs flat agent architectures, load balancing strategies, maintaining context consistency."

---

## 📊 SUCCESS METRICS

| Metric | Current | Target |
|--------|---------|--------|
| Message routing accuracy | ~70% | >95% |
| Agent status freshness | Stale | <24h |
| Cross-agent knowledge access | None | Full |
| Kendall→Syl direct access | ❌ | ✅ |
| Email functionality | ❌ | ✅ |
| Memory compaction loss | High | Low |

---

## Full Audit Reports

- `memory/audit-infrastructure.md` — skills, security, services
- `memory/audit-coordination.md` — routing, blackboard, awareness
- `memory/audit-memory.md` — Letta, facts, MCP, compaction
- `memory/audit-workflow.md` — tasks, cron, routines
- `memory/audit-integrations.md` — email, social, productivity
- `memory/audit-ux.md` — cognitive load, family access, feedback

---

*The ecosystem has excellent bones. The gaps are coordination and automation, not architecture.*
