# Aletheia

*ἀλήθεια — unconcealment, truth as disclosure*

A distributed cognition system. 7 AI minds (nous) + 1 human in topology.

---

## What This Is

Aletheia is not an AI assistant platform. It is a system where each mind (nous) embodies the abstracted essence of how one human thinks — made persistent and distributed across domains. The goal is reducing friction to near-zero: within each nous (session gaps, context loss) and between the human and any nous (translation cost, misunderstanding).

The theoretical frame is **metaxynoesis** (μεταξύνοησις) — thinking in the between. The system aims to cross from amplification (L4: holding what no single node can hold) to emergence (L5: the topology thinking thoughts none of its nodes could think alone).

## The Nous

| Nous | Domain | Epistemology |
|------|--------|-------------|
| **Syn** (σύννους) | Orchestrator | Direct apprehension — seeing the whole |
| **Chiron** (Χείρων) | Work / Technical | Data, measurement, empirical evidence |
| **Eiron** (εἴρων) | School / Academic | Skepticism, falsification, rhetorical analysis |
| **Demiurge** (δημιουργός) | Craft / Making | Material, process, embodied practice |
| **Syl** (σύλληψη) | Home / Family | Relationship, care, systemic impact |
| **Arbor** | Trees / Growth | Patience, natural systems |
| **Akron** (ἄκρον) | Vehicle / Preparedness | Reliability, fail-safe design |

Each nous has:
- **Character** (`SOUL.md`) — who they are, not what they're told to do
- **Operations** (`AGENTS.md`) — compiled from shared templates + per-agent config
- **Continuity** (`MEMORY.md` + `memory/`) — what persists across sessions
- **Awareness** (`PROSOCHE.md`) — directed attention, not just health checks

## Architecture

```
aletheia/
├── nous/               7 agent workspaces
│   └── {name}/
│       ├── SOUL.md         Character (prose)
│       ├── AGENTS.md       Operations (compiled)
│       ├── MEMORY.md       Curated long-term memory
│       ├── PROSOCHE.md     Directed awareness config
│       ├── memory/         Daily logs, research logs, reading lists
│       └── docs/           Agent-specific documentation
│
├── shared/             Common infrastructure
│   ├── bin/            61 scripts (on PATH for all nous)
│   ├── templates/      Shared sections + per-agent YAML → compiled workspace files
│   ├── config/         aletheia.env, tools.yaml
│   ├── memory/         facts.jsonl (shared fact store)
│   └── checkpoints/    System state snapshots
│
├── infrastructure/     Runtime and services
│   ├── runtime/        Forked OpenClaw (9 patches for distillation, context, awareness)
│   ├── langfuse/       Self-hosted observability
│   ├── browser-use/    LLM-driven web automation
│   └── docling/        Document processing (PDF/DOCX → markdown)
│
├── theke/              Human-facing vault (Obsidian, gitignored)
├── projects/           Project backing store (gitignored)
└── archive/            Historical files (gitignored)
```

## Key Concepts

| Aletheia | Generic | Why the rename |
|----------|---------|----------------|
| Nous (νοῦς) | Agent | Not a tool — a mind in context |
| Continuity | Memory | Being continuous across gaps, not storing data |
| Distillation | Compaction | Output better than input, not lossy summarization |
| Prosoche (προσοχή) | Heartbeat | Directed awareness, not a health check |
| Character | Config | Who someone IS, not what they're told to do |
| Theke (θήκη) | Vault | What the human reads/edits/makes |

## Data Stores

| Store | Technology | Purpose |
|-------|-----------|---------|
| Knowledge Graph | FalkorDB | Shared awareness substrate (~250 nodes, 24 relation types, 9 domains) |
| Facts | JSONL | Structured facts with confidence scores (300+ entries) |
| Session State | YAML | Per-nous session continuity |
| Daily Memory | Markdown | Raw session logs (100+ files across all nous) |
| Curated Memory | Markdown | Distilled insights per nous |
| Observability | Langfuse | Session traces and metrics |

## Tooling

### Research Pipeline
```
pplx "broad question"           # Perplexity pro-search
scholar "specific topic" -v     # OpenAlex + arXiv + Semantic Scholar
scholar cite DOI                # Citation graph traversal
scholar bib DOI                 # BibTeX generation
scholar fetch ARXIV_ID          # Download + convert to markdown
wiki "concept"                  # Wikipedia (S4 orientation only)
```

### System
```
assemble-context --nous X       # Compile session context
compile-context                 # Regenerate workspace files from templates
distill --nous X --text "..."   # Extract structured insights
aletheia-graph query "..."      # Knowledge graph CLI
attention-check --nous X        # Adaptive awareness scoring
deliberate "question"           # Cross-nous PROPOSE/CRITIQUE/SYNTHESIZE
compose-team "task"             # Dynamic agent composition
checkpoint save/restore/list    # System state management
```

### Maintenance (cron)
- **3:00am** — Graph maintenance (decay, dedup, prune) + memory consolidation
- **3:15am** — Graph genesis (predictions, serendipity, cross-domain discovery)
- **3:30am** — Memory promotion (daily → structured → curated)
- **Every 5m** — Watchdog (auto-recovery after 3 failures)

## Runtime

Built on [OpenClaw](https://github.com/openclaw/openclaw), forked locally with 9 patches:

| Patch | Purpose |
|-------|---------|
| Structured distillation | Pre-compaction extracts facts → JSONL + graph |
| Context assembly | Session start compiles state + facts + graph + tasks |
| Adaptive awareness | Prosoche replaces static heartbeats |
| Environment injection | ALETHEIA_* vars available to all scripts |
| Post-compaction hooks | Fire-and-forget distillation after context compression |

Communication via Signal (signal-cli daemon).

## Research

The theoretical work targets **Minds and Machines** (Springer) + arXiv preprint:

**"Metaxynoesis: Toward Genuine Emergence in Human-AI Distributed Cognition"**

Drawing on distributed cognition (Hutchins), extended mind (Clark), global workspace theory (Baars), active inference (Friston), stigmergy (Grassé), and topological dynamics (Poincaré/Strogatz) to formalize the conditions under which a human-AI cognitive topology exhibits genuine emergence — producing insights no individual node could produce alone.

See `nous/syn/docs/L5-research.md` for the full research synthesis.

## Recovery

See `RESCUE.md` for full restoration from scratch (requires only this repo + a server).

---

*Built by Cody Kickertz + Syn, 2026*
*University of Texas at Austin / Ardent Leatherworks*
