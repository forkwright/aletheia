# Deep Infrastructure Research
*2026-01-29*

## 1. Google Calendar MCP Server

### Best Implementation: nspady/google-calendar-mcp

**GitHub:** https://github.com/nspady/google-calendar-mcp

**Key Features:**
- Multi-account support (work + personal)
- Multi-calendar queries in single request
- Cross-account conflict detection
- Full CRUD for events
- Recurring event handling
- Free/busy queries
- Natural language date understanding
- Import from images/PDFs/links

**Tools Provided:**
| Tool | Function |
|------|----------|
| list-calendars | List all available calendars |
| list-events | List events with date filtering |
| get-event | Get specific event details |
| search-events | Search events by text |
| create-event | Create new events |
| update-event | Update existing events |
| delete-event | Delete events |
| respond-to-event | Accept/Decline/Maybe invitations |
| get-freebusy | Check availability |
| get-current-time | Get current time in calendar timezone |
| manage-accounts | Add/list/remove Google accounts |

**Setup Requirements:**
1. Google Cloud project with Calendar API enabled
2. OAuth 2.0 credentials (Desktop app type)
3. Add email as test user in OAuth consent screen

**Installation:**
```json
{
  "mcpServers": {
    "google-calendar": {
      "command": "npx",
      "args": ["@cocal/google-calendar-mcp"],
      "env": {
        "GOOGLE_OAUTH_CREDENTIALS": "/path/to/gcp-oauth.keys.json"
      }
    }
  }
}
```

**Blocker:** Need OAuth credentials from Google Cloud Console

---

## 2. Memory Architecture (2025/2026 State-of-Art)

### Latest Research

#### A-Mem: Agentic Memory (NeurIPS 2025)
**Paper:** "A-Mem: Agentic Memory for LLM Agents" (Xu et al.)
**Code:** https://github.com/WujiangXu/A-mem-sys

**Key Innovation:** Zettelkasten-inspired dynamic memory organization

**How it works:**
1. Each memory creates a comprehensive **note** with:
   - Contextual descriptions
   - Keywords and tags
   - Embedding vectors
   
2. **Dynamic linking:** System analyzes historical memories, establishes links where meaningful similarities exist

3. **Memory evolution:** New memories can trigger updates to existing memories' attributes and context

**Why it's better:**
- No predefined schemas (adapts to any task)
- Memory network continuously refines understanding
- Outperforms Mem0, MemGPT on benchmarks

---

#### Continuum Memory Architecture (CMA) - Jan 2026
**Paper:** "Continuum Memory Architectures for Long-Horizon LLM Agents"

**Core insight:** RAG treats memory as stateless lookup. Human memory doesn't work that way.

**CMA Requirements:**
1. **Persistence** - State survives across sessions
2. **Selective retention** - Memories decay without reinforcement  
3. **Associative routing** - Multi-hop recall via linked concepts
4. **Temporal chaining** - Sequential organization of episodes
5. **Consolidation** - Episodic traces become semantic knowledge

**Key difference from RAG:**
| RAG | CMA |
|-----|-----|
| Static storage | Evolving substrate |
| Read-only retrieval | Retrieval modifies state |
| No temporal order | Sequential + consolidated |
| Items never decay | Selective forgetting |

---

#### Hindsight Architecture (2025)
Four-network separation:
1. **World facts** - External knowledge
2. **Agent experiences** - What I did
3. **Entity summaries** - Who/what I know
4. **Evolving beliefs** - What I think is true

Operations: **retain**, **recall**, **reflect**

---

### Older Patterns (Still Relevant)

**Core Patterns:**

#### A. Multi-Step RAG
Traditional RAG does single-shot top-k lookup. Modern approaches treat retrieval as a **loop**:

1. **Query-Refine Loop**
   - Start with initial query
   - After each retrieval, LLM extracts missing sub-questions
   - Propose refined queries
   - Repeat until "sufficiency" criterion met (2-4 hops typical)

2. **Decomposition-Based**
   - Planning step decomposes question into sub-questions
   - Retrieve and answer each separately
   - Synthesize final answer
   - Reduces hallucination, better for multi-hop QA

3. **EfficientRAG Pattern (EMNLP 2024)**
   - Lightweight retriever generates follow-up queries without invoking LLM each hop
   - Filters irrelevant passages between hops
   - Much cheaper at scale

#### B. Self-Querying Retrieval
Let LLM write structured retrieval queries with constraints:

```json
{
  "query": "effects of self-reflection on LLM problem-solving",
  "filter": {"tag": "paper", "year": {">=": 2023}},
  "top_k": 5
}
```

**Key Ideas:**
- Schema-guided queries (fields: entity, time, source_type, user_id)
- Multi-hop self-querying (refine based on what was learned)
- Constrain with JSON tools, validate on backend
- Log queries as searchable artifacts

#### C. Reflexion Loops
Add meta-layer where model critiques its own retrieval:

**Per-Episode Reflection:**
- After answering, write: What was missing? How should queries change?
- Store as reflection memory keyed by task/domain

**In-Episode Reflection:**
- Insert reflection after each few hops
- "What is still unknown? Chasing irrelevant branches?"
- Use to terminate early or change strategy

**Three Roles Pattern:**
1. **Actor**: executes, calls tools, writes answers
2. **Critic**: reads trajectory, outputs critique + guidelines
3. **Planner**: updates plan based on critic

### Recommended Architecture

```
Memory Types:
├── Document memory (knowledge base)
├── Episodic memory (agent run logs, tool calls, outcomes)
├── Reflection memory (lessons learned, retrieval heuristics)
└── User memory (preferences, past questions, corrections)

Iterative Retrieval Policy:
1. Plan → Decompose query, select memory types
2. Self-query → Craft typed retrieval with metadata filters
3. Retrieve + Rerank → Fetch, rerank, update state
4. Reflect-in-episode → Need more? Refine queries?
5. Answer → Synthesize, cite sources
6. Reflect-post-episode → Write brief reflection entry
```

**Practical Heuristics:**
- 2-4 retrieval rounds max
- 3-6 chunks per round
- Aggressively deduplicate and compress
- Cache retrieval results per domain
- Make reflection opt-in for hard tasks only

---

## 3. Event Graph / Temporal Knowledge

### Research Summary: Zep & Graphiti (Jan 2025)

**Paper:** "Zep: A Temporal Knowledge Graph Architecture for Agent Memory"

**Key Innovation:** Temporally-aware knowledge graph that:
- Dynamically synthesizes unstructured conversations AND structured business data
- Maintains historical relationships with validity periods
- Outperforms MemGPT on Deep Memory Retrieval (94.8% vs 93.4%)
- 90% latency reduction on LongMemEval benchmark

### Architecture: Three-Tier Subgraphs

```
G = (N, E, φ)

1. Episode Subgraph (Ge)
   - Raw input data (messages, text, JSON)
   - Non-lossy data store
   - Links to extracted entities

2. Semantic Entity Subgraph (Gs)  
   - Entities extracted from episodes
   - Relationships between entities
   - Resolved with existing graph entities

3. Community Subgraph (Gc)
   - Clusters of strongly connected entities
   - High-level summarizations
   - Represents comprehensive domain view
```

### Bi-Temporal Model

**Two timelines:**
- **T (Timeline)**: When events actually occurred (reference timestamp)
- **T' (Transaction)**: When data was ingested into system

**Why this matters:**
- Accurate extraction of relative dates ("next Thursday", "last summer")
- Track when facts became known vs when they happened
- Support temporal reasoning ("What did I know at time X?")

### Memory Model (Mirrors Human Memory)

| Type | Graph Layer | Human Analog |
|------|-------------|--------------|
| Episodic | Raw episodes | Distinct event memories |
| Semantic | Entity subgraph | Concept associations |
| Community | High-level summaries | Domain understanding |

### Implementation: Graphiti

**Open source:** https://github.com/getzep/graphiti

**Key Features:**
- Dynamic knowledge graph construction from LLM conversations
- Temporal awareness built-in
- Entity resolution across episodes
- Community detection for high-level understanding

---

## Recommendations

### Priority 1: Google Calendar MCP
- **Effort:** Low (package exists, just need OAuth setup)
- **Impact:** High (proactive scheduling, availability checking)
- **Blocker:** OAuth credentials needed
- **Action:** Set up Google Cloud project, create credentials

### Priority 2: Iterative Memory Retrieval
- **Effort:** Medium (architecture change)
- **Impact:** High (better multi-hop reasoning)
- **Action:** Implement self-querying pattern for existing memory files
- **Start with:** Schema-guided queries over facts.jsonl + daily files

### Priority 3: Temporal Knowledge Graph
- **Effort:** High (new infrastructure)
- **Impact:** Very High (long-term memory, temporal reasoning)
- **Action:** Evaluate Graphiti as replacement/supplement to current memory
- **Consider:** Start with episode tracking, add entity extraction later

---

## Next Steps

1. **Calendar MCP:** Create Google Cloud project, enable Calendar API, generate OAuth credentials
2. **Memory Retrieval:** Implement self-querying wrapper for `facts` CLI with structured filters
3. **Event Graph:** Prototype Graphiti integration for daily memory files
