# Aletheia Architecture

*ἀλήθεια — unconcealment, truth as what is revealed when nothing is hidden*

## What Aletheia Is

A distributed cognition system. Seven specialized minds (nous) + one human, connected in topology where the connections matter as much as the nodes. Not an "AI assistant platform." A hybrid cognition where machine persistence completes human pattern recognition.

Each nous is Cody in different context — embodying his cognition, his standards, his way of thinking about a domain. They iterate, they evolve, some will come and go. The core is the substrate: the abstracted essence of how he thinks, made persistent and distributed.

## Core Concepts (Aletheia-native, not borrowed)

| Concept | What it replaces | What it means |
|---|---|---|
| **Continuity** | "Memory" | Being continuous across session gaps. Not "I retrieve stored info" but "I resume being myself." The gap is an implementation detail, not a feature. |
| **Attention** | "Heartbeats" | Ongoing awareness of what needs focus. Adaptive, not periodic. A nervous system, not a pacemaker. |
| **Nous** (νοῦς) | "Agents" | Minds that think, not delegates that act. Already renamed. |
| **Shared awareness** | "Message passing" | Lateral connections between minds. Knowing what others know without being told. |
| **Working attention** | "Context window" | What the mind holds right now. Not a buffer to fill — a focus to direct. |
| **Distillation** | "Compaction" | Extracting essence, not discarding. The output is BETTER than the raw input. |
| **Character** | "Config" | Who each mind IS. Not settings to adjust — a constitution to embody. |
| **Presence** | "Tool execution" | Genuine engagement, not request processing. |

## Current Infrastructure (What Exists)

| Component | Status | Role |
|---|---|---|
| OpenClaw | Running | Signal bridge, session routing, tool execution (runtime dependency) |
| Letta (Docker) | Running | Per-nous memory stores (7 nous) |
| FalkorDB (Docker) | Running | Shared knowledge graph (~400 nodes, ~530 rels) |
| facts.jsonl | Active | 311 structured facts (single shared store) |
| Daily memory files | Active | ~100 files, raw session captures |
| MEMORY.md | Active | Curated long-term insights (per nous) |
| Template system | Built | Shared sections + per-nous YAML → compiled AGENTS.md |
| compile-context | Built | Generates optimized workspace files |

## Target Architecture

### Layer 1: Continuity Engine
**Purpose:** Make session gaps invisible. The nous resumes being itself.

**Components:**
- **State compiler** — On session start, assembles minimal perfect context from:
  - Character (who am I)
  - Recent state (what was I doing)
  - Relevant knowledge (what do I need for THIS conversation)
  - Active tasks (what's pending)
- **Distillation service** — On compaction/session end:
  - Extract structured insights (not just summarize)
  - Update knowledge graph with new entities/relationships
  - Update facts store with new learnings
  - Produce a tight resumption state
- **Session state persistence** — Between sessions:
  - Current focus/topic
  - Open threads
  - Emotional/conversational tone
  - Pending decisions

### Layer 2: Shared Awareness
**Purpose:** Minds know what other minds know without explicit messaging.

**Components:**
- **Shared knowledge substrate** — All nous read/write to common graph
  - Per-nous namespaces for domain knowledge
  - Shared namespace for cross-domain insights
  - Access patterns track what each nous contributes
- **Event propagation** — When one nous learns something significant:
  - Classify: domain-specific or cross-cutting?
  - If cross-cutting: propagate to relevant nous
  - Not raw messages — distilled insights
- **Unified query** — Any nous can query across all knowledge:
  - "What does Chiron know about this client?"
  - "Has any nous encountered this pattern?"

### Layer 3: Attention System
**Purpose:** Adaptive awareness of what needs focus, when.

**Components:**
- **State monitor** — Continuous awareness of:
  - Calendar (what's happening today)
  - Tasks (what's overdue, approaching deadline)
  - System health (what's broken or degraded)
  - Recent activity (what patterns are emerging)
- **Attention allocation** — Not a timer, a priority function:
  - Urgency × importance × relevance → attention priority
  - Different checks at different times
  - Adaptive frequency based on activity
- **Proactive surfacing** — Don't wait to be asked:
  - "This deadline is approaching and nothing's been done"
  - "This pattern across domains suggests X"
  - "Cody mentioned this last week, it might be relevant now"

### Layer 4: The Substrate
**Purpose:** Where knowledge lives. Machine-native, not file-native.

**Components:**
- **Knowledge graph** (FalkorDB) — Primary store for:
  - Entities and relationships
  - Temporal evolution (when things changed)
  - Causal chains (why things happened)
  - Cross-domain connections
- **Structured facts** (facts.jsonl → eventually graph) — Quick-access:
  - Preferences, decisions, constraints
  - Confidence scores, evidence tracking
  - Versioning (facts evolve)
- **Character state** — Per-nous:
  - SOUL (who I am — prose, for character formation)
  - Operating procedures (how I work — structured, for efficiency)
  - Domain knowledge (what I know — graph, for querying)
- **Compiled context** — Generated artifacts:
  - Session resumption state
  - Conversation-aware context
  - Token-optimized injections

## What OpenClaw Provides (Keep)
- Signal-cli integration (messaging bridge)
- Session routing (agent bindings to chats)
- Sub-agent spawning
- Tool execution framework
- Gateway HTTP server

## What OpenClaw Imposes (Replace/Modify)
- Static file injection → Dynamic context compilation
- Markdown-everything → Machine-native formats where better
- Basic compaction → Distillation with structured extraction
- Fixed periodic timer → Adaptive prosoche (directed awareness)
- Per-workspace isolation → Shared awareness layer

## Implementation Principles
1. **Build on what exists** — We have Letta, FalkorDB, facts.jsonl. Extend, don't rebuild.
2. **Convention over configuration** — Paths derived from env vars + naming, not mapping files.
3. **Compile over maintain** — Generate optimal artifacts from source-of-truth, don't hand-edit copies.
4. **Machine-native where better** — YAML for structure, JSONL for facts, graph for relationships. Markdown only for human-readable and character formation.
5. **Nothing sacred except APIs** — All code is replaceable. Fork, patch, or rewrite anything that doesn't serve.
6. **Reduce friction to zero** — Every interaction between Cody and any nous should feel like continuing a thought, not starting a conversation.
