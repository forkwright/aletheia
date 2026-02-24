# Spec: Agent Portability — Export, Import & Time-Travel

**Status:** All 4 phases complete (PRs #100, #124, #128).
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-15, F-34; Letta Agent File + LangGraph checkpointing

---

## Problem

There's no way to backup, clone, or migrate a nous. Git-tracked workspaces capture files but not memory state (Qdrant vectors, Neo4j graph, session history). If the server dies, the agents' learned context dies with it.

There's also no way to branch a conversation — fork from a historical point to explore an alternative path without losing the original.

---

## Design

### Phase 1: Agent File Export (F-15)

`aletheia export <nous-id>` produces a portable JSON file containing everything needed to reconstruct an agent:

```typescript
interface AgentFile {
  version: 1;
  exportedAt: string;
  nous: {
    id: string;
    name: string;
    model: string;
    config: NousConfig;        // from config.yaml
  };
  workspace: {
    files: Record<string, string>;  // path → content (text files only)
    // Binary files listed but not included
    binaryFiles: string[];
  };
  memory: {
    vectors: Array<{
      id: string;
      text: string;
      metadata: Record<string, unknown>;
      embedding?: number[];    // optional — can re-embed on import
    }>;
    graph: {
      nodes: Array<{ id: string; labels: string[]; properties: Record<string, unknown> }>;
      edges: Array<{ source: string; target: string; type: string; properties: Record<string, unknown> }>;
    };
  };
  sessions: {
    primary: {
      id: string;
      messages: Array<{ role: string; content: string; metadata: Record<string, unknown> }>;
      workingState?: string;
      notes?: Array<{ content: string; category: string }>;
      threadSummary?: string;
    };
    // Recent background sessions (last 7 days)
    background: Array<{
      key: string;
      messageCount: number;
      lastActivity: string;
    }>;
  };
  distillation: {
    receipts: DistillationReceipt[];  // last 10
    reflections: ReflectionReceipt[]; // last 10 (if Spec 19 implemented)
  };
}
```

**Size management** — embeddings are optional (can re-embed on import). Session history is truncated to last distillation + tail. Binary workspace files are listed but not included.

### Phase 2: Agent File Import

`aletheia import <file.json>` creates a new nous from an export:

1. **Parse and validate** the agent file against schema
2. **ID remapping** — generate new IDs for sessions, messages, memories to avoid collisions
3. **Workspace restoration** — write files to new workspace directory
4. **Memory restoration** — insert vectors into Qdrant (re-embed if embeddings not included), create Neo4j nodes/edges
5. **Session restoration** — create primary session with message history, working state, notes
6. **Config update** — add nous definition to config.yaml (or merge with existing)

**Conflict handling:**
- If a nous with the same name exists: prompt for rename or overwrite
- Memory dedup: check vector similarity against existing memories, skip near-duplicates (0.92 threshold)

### Phase 3: Scheduled Backups

Cron job that runs `export` for all active nous on a schedule:

```yaml
backup:
  enabled: true
  schedule: "0 4 * * *"    # 4 AM daily
  destination: /mnt/nas/backups/aletheia/
  retention: 30             # keep 30 days
  compress: true            # gzip the JSON
```

### Phase 4: Checkpoint Time-Travel (F-34)

Branch a session from a historical point.

**Concept** — every distillation creates an implicit checkpoint. Time-travel lets you fork from any checkpoint:

```
aletheia fork <session-id> --at <distillation-id>
```

This creates a new session starting from the post-distillation state at that point:
- Messages from after the checkpoint are discarded
- Working state reverted to checkpoint state
- Thread summary reverted
- Memories created after checkpoint are NOT removed (they may be valid independently)

**UI integration** — session picker shows checkpoint history. Click a checkpoint to fork. New session appears as a branch with a visual indicator of its origin point.

**Use cases:**
- "That conversation went off the rails at turn 50 — let me fork from before that"
- "I want to try a different approach to the same problem"
- Debugging: reproduce an issue by forking from before it occurred

---

## Implementation Order

| Phase | What | Effort | Features |
|-------|------|--------|----------|
| **1** | Agent file export | Medium | F-15 |
| **2** | Agent file import | Medium | F-15 |
| **3** | Scheduled backups | Small | F-15 |
| **4** | Checkpoint time-travel | Medium | F-34 |

---

## Success Criteria

- Any nous can be exported to a single file and imported on a fresh Aletheia instance
- Export/import round-trip preserves identity, memories, and session continuity
- Automated daily backups with configurable retention
- Time-travel forks create clean branch points without corrupting the original session
