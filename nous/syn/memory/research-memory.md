# Multi-Agent Memory Architecture Research
*Synthesized from deep research conducted 2025-01-29*

## Executive Summary

State-of-the-art multi-agent memory systems are converging on **hybrid hierarchical architectures** that combine:

1. **Shared graph-based substrates** for structured knowledge and relationships
2. **Federated vector stores** with domain-specific routing and ranking
3. **Explicit episodic/semantic separation** for experience vs. generalized knowledge
4. **Consensus mechanisms** for conflict resolution and memory pruning
5. **Cross-agent knowledge transfer protocols** that preserve domain expertise

Our current memory stack (Letta, facts.jsonl, MCP Memory, daily files, MEMORY.md) is fragmented but contains the right components. The research suggests specific integration patterns that could unlock collective intelligence emergence.

## What "Right" Looks Like for Multi-Agent Memory

Based on the latest research, an optimal multi-agent memory architecture has these characteristics:

### 1. Three-Tier Hierarchy with Clear Boundaries
- **Ephemeral/Session Layer**: Raw interactions, temporary context (our daily files)
- **Agent-Specific Layer**: Domain expertise, private working memory (our current Letta agents)
- **Shared Consensus Layer**: Cross-domain facts, validated procedures, collective insights

### 2. Hybrid Storage Strategy
- **Knowledge Graph** for entities, relationships, and multi-hop reasoning
- **Vector Store** with domain-aware routing for semantic similarity
- **Structured Facts** with temporal validity and confidence scores
- **Episode Store** for specific experiences and trace replay

### 3. Memory Synchronization Protocols
- **Write Admission Policies**: Utility-driven scoring for what enters shared memory
- **Conflict Resolution**: Byzantine-fault-tolerant consensus for memory updates
- **Pruning Strategies**: Semantic similarity voting + temporal decay

### 4. Cross-Agent Transfer Mechanisms
- **Graph-mediated Transfer**: Insights published as reusable patterns
- **Role-aware Retrieval**: Domain-specific views of shared knowledge
- **Conservative Abstraction**: Transfer higher-level schemas, not raw specifics

## Analysis of Our Current Stack

### Strengths
1. **Multi-tier structure exists**: Daily → MEMORY.md → Letta progression mirrors research best practices
2. **Domain-specific agents**: Syl, Chiron, Eiron, Demiurge already provide specialized memory isolation
3. **Temporal facts system**: facts.jsonl provides confidence, categories, and validity periods
4. **MCP Memory graph**: Early implementation of entity-relationship storage

### Gaps
1. **No federated routing**: Each agent operates in isolation, no cross-domain knowledge discovery
2. **Fragmented storage**: Five different memory systems with no unified query layer
3. **Manual consolidation**: No automated consensus or conflict resolution
4. **Limited cross-agent transfer**: No structured knowledge sharing protocols

## Integration Recommendations

### Phase 1: Federated Query Layer
Create a unified memory query interface that routes across all storage systems:

```bash
# New command: memory-query
memory-query "What does Cody prefer for morning routines?"
# Routes to: Syl's Letta + facts.jsonl + MEMORY.md scan

memory-query "How do we usually handle SQL performance issues?"
# Routes to: Chiron's Letta + work domain facts

memory-query "Cross-domain: What are Cody's energy patterns?"
# Routes to: All agents + daily files + MCP Memory graph
```

### Phase 2: Shared Knowledge Graph
Extend MCP Memory to become the primary shared substrate:

```json
// Enhanced entity schema
{
  "entity": "morning-routine-preference", 
  "type": "behavioral-pattern",
  "domains": ["personal", "health"],
  "confidence": 0.85,
  "source_agents": ["syl"],
  "last_confirmed": "2025-01-29",
  "related_episodes": ["morning-brief-2025-01-28"],
  "transferable": true
}
```

### Phase 3: Cross-Agent Synchronization
Implement background sync processes:

1. **Daily Consolidation**: Each agent extracts domain insights to shared graph
2. **Weekly Cross-Pollination**: Domain agents query other domains for relevant patterns
3. **Conflict Detection**: Flag contradictory facts across domains for human resolution

### Phase 4: Episodic/Semantic Separation
Restructure existing memory into clear separation:

**Episodic (Experience) Storage**:
- Daily files → structured episode logs with metadata
- Each interaction tagged with: outcome, domain, participants, success metrics

**Semantic (Knowledge) Storage**:
- facts.jsonl → consolidated into MCP Memory graph
- MEMORY.md insights → promoted to graph relationships
- Letta → focused on domain-specific semantic patterns

## Specific Implementation Steps

### 1. Create Memory Router (Week 1)
```bash
# New shared tool: memory-router
memory-router --query "text" --domains [auto|specific] --format [facts|episodes|insights]
```

Location: `/mnt/ssd/moltbot/shared/bin/memory-router`
Function: Route queries to appropriate memory systems, merge results

### 2. Enhanced MCP Memory Schema (Week 2)
Extend current graph with:
- Domain tags on all entities
- Confidence scoring
- Source attribution (which agent added what)
- Cross-references to external storage (Letta IDs, daily file paths)

### 3. Cross-Agent Sync Protocol (Week 3)
Create background jobs:
- `sync-domain-insights`: Extract patterns from each agent's Letta to shared graph
- `detect-conflicts`: Find contradictory facts across domains
- `prune-outdated`: Remove low-confidence or superseded knowledge

### 4. Unified Memory Dashboard (Week 4)
Web interface showing:
- Real-time memory health across all systems
- Cross-domain knowledge flow visualization
- Conflict resolution queue
- Memory utilization by domain

## Domain-Specific Patterns

Based on our current agent specializations:

### Syl (Home/Family)
- **Episodic**: Family interactions, home automation events, personal preferences
- **Semantic**: Behavioral patterns, relationship dynamics, routine optimizations
- **Cross-Domain Value**: Energy patterns, preference stability, decision contexts

### Chiron (Work)
- **Episodic**: Project history, client interactions, performance metrics
- **Semantic**: Best practices, technical patterns, business rules
- **Cross-Domain Value**: Problem-solving approaches, time management, stress indicators

### Eiron (School)
- **Episodic**: Assignment completion, study sessions, academic performance
- **Semantic**: Learning strategies, subject matter expertise, academic goals
- **Cross-Domain Value**: Learning patterns, cognitive load management

### Demiurge (Craft/Making)
- **Episodic**: Project builds, technique experiments, material testing
- **Semantic**: Craft knowledge, tool usage, creative processes
- **Cross-Domain Value**: Problem decomposition, hands-on learning, quality standards

## Success Metrics

To validate the integrated memory system:

1. **Cross-Domain Accuracy**: Can Chiron answer questions about Cody's energy patterns using Syl's data?
2. **Conflict Resolution Rate**: How quickly are contradictory facts identified and resolved?
3. **Knowledge Transfer Speed**: Time from insight in one domain to availability in others
4. **Memory Coherence**: Consistency of facts across all storage systems
5. **Agent Performance**: Do domain agents make better decisions with access to cross-domain insights?

## Risk Mitigation

### Privacy and Security
- Domain-specific access controls (family data not visible to work contexts)
- Explicit consent mechanisms for cross-domain knowledge sharing
- Regular audits of what knowledge is being shared where

### Performance and Scalability
- Lazy loading of cross-domain data (only when explicitly requested)
- Caching of frequently accessed cross-domain patterns
- Regular pruning to prevent memory bloat

### Knowledge Quality
- Confidence thresholds for cross-domain transfer
- Source attribution for all shared knowledge
- Regular validation cycles for shared facts

## Research Citations

This synthesis draws from cutting-edge research in:

1. **Multi-Agent Memory Synchronization**: G-Memory hierarchical graphs, Co-Forgetting consensus protocols
2. **RAG Optimization**: Domain-specific routing, hybrid retrieval strategies, federated ranking
3. **Episodic/Semantic Separation**: Event logs vs. generalized knowledge, consolidation patterns
4. **Hybrid Graph/Vector Architectures**: Parallel retrieval, unified embedding spaces, multi-hop reasoning

## Next Actions

1. **Immediate (This Week)**: Create memory-router prototype and test cross-system queries
2. **Short-term (Month 1)**: Implement MCP Memory extensions and basic sync protocols  
3. **Medium-term (Quarter 1)**: Full integration with conflict resolution and cross-agent transfer
4. **Long-term (Year 1)**: Emergent collective intelligence patterns and optimization

The research shows this architecture should unlock significant collective intelligence emergence as our agent ecosystem grows and encounters more complex multi-domain problems.

---

*Research completed: 2025-01-29*
*Primary sources: Multi-agent memory synchronization (ArXiv), RAG optimization 2025, episodic/semantic memory separation, hybrid graph-vector architectures*