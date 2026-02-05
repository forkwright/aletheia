# Research Synthesis: What Right Looks Like
*Generated: 2026-01-30 from 6 deep research threads*

---

## The Big Picture

Our ecosystem has **excellent architecture** but needs **coordination infrastructure** and **context preservation** to reach its potential. The research converges on several key principles:

### Core Insights Across All Research

1. **Context-switching is the enemy** — 23-minute recovery for neurotypical, 2-5x for AuDHD
2. **5-7 agents is optimal** — we're in the sweet spot before needing hierarchical structure
3. **Coordination overhead scales quadratically** — infrastructure must precede growth
4. **Memory unification beats memory duplication** — federated query, not copy-paste
5. **Self-healing > alerting** — automate recovery, don't just report failures

---

## What "Right" Looks Like

### Coordination
- **Task contracts** with explicit state, not conversational handoffs
- **Correlation IDs** tracking requests across agent boundaries
- **Resource leasing** for shared tools (Perplexity, APIs)
- **Blackboard as source of truth** for cross-domain work
- **<200ms coordination latency** before bottlenecks appear

### Memory
- **Three-tier hierarchy**: Ephemeral → Agent-Specific → Shared Consensus
- **Federated query layer**: One command searches all systems
- **Hybrid storage**: Knowledge graphs + vectors + structured facts
- **Automated consensus**: Conflicts resolved, not accumulated
- **Cross-agent knowledge transfer** preserving domain expertise

### Context Preservation
- **Pre-compaction extraction**: Insights captured BEFORE truncation
- **Importance-weighted summarization**: Decisions > acknowledgments
- **Decision tree preservation**: Reasoning chains survive compression
- **Cross-turn dependency mapping**: Emergent insights protected
- **Neuroscience-inspired consolidation**: Dual-memory with selective replay

### Neurodivergent UX
- **Warm handoffs**: Context + emotional state + progress markers
- **Transparent routing**: "Why this agent?" available on demand
- **Flow protection**: Minimize interruptions during hyperfocus
- **Energy-aware selection**: Route based on detected state
- **Predictable patterns with flexibility**: Autism needs structure, ADHD needs novelty

### Monitoring
- **Four pillars**: Agent metrics, unified observability, predictive alerting, self-healing
- **3-tier alerts**: Critical (page) → Warning (business hours) → Info (reports)
- **Target**: 99.9% uptime, <5min automated recovery, <5% false positives
- **Stack**: Langfuse for agents, Prometheus/Grafana for infra, Temporal for orchestration

### Scaling
- **3-7 agents per workflow** before hierarchical structure needed
- **Build infrastructure before adding agents**
- **Communication latency monitoring** as early warning
- **Domain boundaries before capability overlap**
- **Emergent behavior** comes from topology, not node complexity

---

## Unified Implementation Roadmap

### Week 1: Critical Foundation
| Action | Domain | Effort |
|--------|--------|--------|
| Fix ~/.clawdbot permissions | Security | 5 min |
| Add Kendall→Syl binding | UX | 30 min |
| Restore email (Proton Bridge) | Integrations | 1-2 hr |
| Task contract schema in shared config | Coordination | 2 hr |
| Correlation IDs for inter-agent messages | Coordination | 2 hr |

### Week 2: Coordination Infrastructure
| Action | Domain | Effort |
|--------|--------|--------|
| memory-router prototype | Memory | 4 hr |
| Enhanced pre-compact with insight extraction | Compaction | 4 hr |
| CrewAI routing calibration | Coordination | 2 hr |
| Blackboard activation in agent protocols | Coordination | 2 hr |
| Status freshness monitoring | Monitoring | 2 hr |

### Week 3-4: Memory Unification
| Action | Domain | Effort |
|--------|--------|--------|
| Cross-agent Letta query tool | Memory | 4 hr |
| MCP Memory schema extension | Memory | 3 hr |
| Federated query across all systems | Memory | 6 hr |
| Warm handoff protocol implementation | UX | 4 hr |
| Langfuse setup for agent tracing | Monitoring | 4 hr |

### Month 2: Production Hardening
| Action | Domain | Effort |
|--------|--------|--------|
| Resource manager for shared APIs | Coordination | 6 hr |
| Background sync processes | Memory | 8 hr |
| Self-healing automation | Monitoring | 8 hr |
| Energy-aware agent routing | UX | 6 hr |
| Agent health dashboard | Monitoring | 4 hr |

---

## Quick Reference: Key Patterns

### Task Handoff Contract
```json
{
  "task_id": "uuid",
  "correlation_id": "trace-across-agents",
  "source_agent": "syn",
  "target_agent": "chiron", 
  "task_type": "sql_query",
  "context": { "full_state": "..." },
  "deadline": "2026-01-30T18:00:00Z",
  "callback": "sessions_send to syn"
}
```

### Warm Handoff (AuDHD-Optimized)
```
[Before handoff]
"I'm going to pass this to Chiron who handles SQL work. 
He'll have full context including what we've discussed."

[After handoff - Chiron]
"I've got the full picture from Syn. Continuing from 
your question about the dashboard query. Here's what I found..."
```

### Pre-Compaction Extraction
```bash
# Before compaction triggers:
1. Extract decisions made (→ facts.jsonl)
2. Extract user preferences (→ MEMORY.md) 
3. Extract reasoning chains (→ memory/YYYY-MM-DD.md)
4. Map cross-turn insights (→ insights buffer)
5. THEN run hierarchical summarization
```

---

## Success Metrics

| Metric | Current | Target | Timeline |
|--------|---------|--------|----------|
| Context-switch recovery | Manual | <2 min | Week 2 |
| Cross-agent query | None | <500ms | Week 3 |
| Insight preservation | ~30% | >90% | Week 2 |
| Agent status freshness | Stale | <24h | Week 2 |
| Routing accuracy | ~70% | >95% | Week 2 |
| Automated recovery | None | <5 min | Month 2 |

---

## Research Files

Full details in each research document:
- `research-coordination.md` — task contracts, blackboard, SLA monitoring
- `research-memory.md` — federated query, hybrid storage, consensus
- `research-compaction.md` — insight extraction, hierarchical summarization
- `research-neurodivergent-ux.md` — warm handoffs, flow protection, energy-aware
- `research-monitoring.md` — Langfuse, alerting tiers, self-healing
- `research-scaling.md` — optimal team size, when to add agents

---

*The research is clear: we have the right architecture. Now we need the coordination infrastructure to make it sing.*
