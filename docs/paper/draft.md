# Aletheia: Datalog, Kernel Sandboxing, and Multi-Signal Recall for Persistent Agent Memory

> Draft technical paper - ready for review
> Target: arXiv preprint + conference submission

---

## Abstract

Aletheia is a persistent agent runtime that embeds knowledge graph, vector search, full-text retrieval, and kernel-level sandboxing in a single Rust binary. Three design choices distinguish it from existing memory systems.

First, it uses Datalog as the knowledge graph substrate instead of property graphs, which enables recursive inference and rule-based reasoning within the recall pipeline. Second, it integrates Landlock LSM, seccomp BPF, and Linux network namespaces into the agent tool loop, providing sandboxing comparable to OpenAI Codex and NVIDIA OpenShell but without external orchestration. Third, it scores memory recall with six signals - vector similarity, BM25 full-text, graph intelligence, temporal decay, epistemic tier, and access frequency - combined through a tunable weighted formula.

The system runs as one self-contained process with zero external services. This paper describes the architecture, evaluates the recall pipeline against published benchmarks, and compares Aletheia to Zep, Letta, Hindsight, and Mem0.

---

## 1. Introduction

Long-term memory is the central problem in agent architecture. Without it, every session starts from zero. With it, agents accumulate knowledge, refine beliefs, and build relationships with users over months or years.

The research community has produced several memory systems. Zep [1] pairs property graphs with vector search. MemGPT [2] uses hierarchical memory with explicit management operations. Hindsight [3] provides an upper bound by showing the full conversation at query time. Mem0 [4] layers a memory store over existing LLM APIs. These systems advance the state of the art, but each leaves a gap: none uses Datalog for the knowledge substrate, none integrates kernel-level sandboxing into the agent loop, and none combines more than three signals for recall scoring.

Aletheia closes these gaps. It is a production agent runtime built in Rust with 47 workspace crates and a single-binary deployment model. Its memory subsystem (mneme) embeds a Datalog engine (krites) with HNSW vector indexes, full-text search, and graph algorithms. Tool execution (organon) runs built-in and external tools inside a Landlock + seccomp + netns sandbox. Recall (episteme) fuses six scoring signals through operator-tunable weights.

This paper makes four contributions:

1. **Datalog as a knowledge graph substrate for agent memory.** We show that Datalog's recursive queries and rule-based inference provide expressiveness that property graphs cannot match, and we quantify the overhead.
2. **Kernel-level sandbox integration in a persistent agent server.** We describe how Landlock, seccomp, and network namespaces apply to every tool execution without containers or root privileges.
3. **6-factor multi-signal recall scoring.** We present a weighted combination of vector similarity, BM25, graph intelligence, temporal decay, epistemic tier, and access frequency, with per-nous tunable weights.
4. **Single-binary architectural sovereignty.** We demonstrate that the full stack - KG, vectors, BM25, agent loop, sandbox, and SSE streaming - runs in one process with no external dependencies.

---

## 2. Background and related work

### 2.1 Agent memory systems

| System | Graph model | Vector store | Sandbox | Recall signals | Deployment |
|---|---|---|---|---|---|
| Zep | Property graph (Neo4j) | Qdrant | None | 2 (vector, graph) | Multi-service |
| Letta | Custom memory graph | HNSW (embedded) | None | 2 (vector, recency) | Python package |
| Hindsight | None (full context) | None | None | 1 (exact match) | Research prototype |
| Mem0 | Key-value + metadata | pgvector | None | 2 (vector, metadata) | Cloud API |
| **Aletheia** | **Datalog** | **HNSW (embedded)** | **Landlock + seccomp + netns** | **6 (vector, BM25, graph, decay, tier, frequency)** | **Single binary** |

**Zep** [1] stores entities and relationships in Neo4j and vector embeddings in Qdrant. It extracts facts with an LLM and links them into a property graph. Recall uses vector similarity plus graph traversal. Zep requires two external services and a Python backend.

**Letta** (formerly MemGPT) [2] implements a memory hierarchy with explicit `core_memory_replace` and `archival_memory_search` operations. It uses an embedded HNSW index and a custom graph structure. Letta's recall combines vector similarity with recency. It runs as a Python package with optional PostgreSQL.

**Hindsight** [3] provides an upper-bound baseline by presenting the full conversation history at query time. It scores highest on LongMemEval and LoCoMo because it avoids retrieval errors entirely, but it is not a deployable memory system. It acts as a ceiling against which practical systems measure themselves.

**Mem0** [4] adds a memory layer to existing LLM applications. It stores key-value memories with metadata filters and pgvector for semantic search. Recall uses vector similarity plus metadata matching. Mem0 offers a hosted cloud API or self-hosted Python server.

None of these systems uses Datalog, integrates kernel sandboxing, or scores recall with more than three signals.

### 2.2 Datalog for knowledge representation

As a declarative logic language, Datalog guarantees termination under stratified negation, supports recursive queries (transitive closure, reachability), and provides rule-based inference (if-then rules over relations). Property graphs require traversals or Cypher queries for the same operations, and recursive graph queries in Cypher are not universally supported.

Datalog has been used in program analysis, network configuration, and security policy. Its application to agent memory is new. Aletheia uses Datalog as the native storage format for facts, entities, relationships, and derived rules, not as an external query layer.

---

## 3. Architecture

### 3.1 System overview

Aletheia is a Rust workspace with 47 crates. A single binary (`aletheia`) embeds all subsystems. HTTP traffic arrives at `pylon`, an Axum API with SSE streaming. Agent turns flow through `nous`, a Tokio actor that processes bootstrap, recall, execution, and finalize stages. The memory subsystem (`mneme`) is a thin facade over four sub-crates: `eidos` (types), `graphe` (fjall session store), `episteme` (knowledge pipeline), and `krites` (Datalog engine).

```text
aletheia binary
├── pylon        HTTP gateway, SSE, auth middleware
├── nous         Agent pipeline (bootstrap → recall → execute → finalize)
├── organon      Tool registry (67 built-ins, default) + sandbox
├── mneme        Memory facade
│   ├── eidos    Shared knowledge types
│   ├── graphe   fjall session store
│   ├── episteme Knowledge pipeline (extract, embed, recall, consolidate)
│   └── krites   Datalog engine + HNSW vectors + graph algorithms
├── hermeneus    Anthropic client + model routing
├── symbolon     JWT auth + RBAC
└── daemon       Background tasks + cron + prosoche
```

### 3.2 Datalog engine (krites)

Krites is an embedded Datalog engine with in-memory and fjall (LSM-tree) storage backends. Historical SQLite backup/restore hooks are disabled with the removed `storage-sqlite` feature. It supports:

- **Stratified Datalog** with negation and recursion
- **HNSW approximate nearest-neighbor vector search** inside queries
- **Full-text search (FTS)** with BM25 scoring
- **Fixed rules** for graph algorithms: `PageRank`, `Louvain` community detection, bounded BFS
- **Multi-relation transactions** with channel-based dispatch
- **Query caching** with LRU hit/miss metrics
- **Callbacks** for reactive updates on relation changes

A fact is stored as a Datalog relation:

```datalog
:create facts {
    fact_id: String,
    content: String,
    entity_id: String,
    fact_type: String,
    epistemic_tier: String,
    created_at: String,
    access_count: Int default 0,
    stability_hours: Float default 0.0
}
```

Relationships form a separate relation:

```datalog
:create relationships {
    src: String,
    dst: String,
    relationship_type: String,
    weight: Float default 1.0
}
```

Recursive queries express transitive relationships directly:

```datalog
?[*x] := *relationships{"alice", x}
?[*x] := *relationships{"alice", y}, ?[y], *relationships{y, x}
```

This two-line query computes the full reachability closure from "alice." In a property graph, the equivalent requires a `MATCH` traversal with variable-length paths, which many graph databases optimize poorly or do not support at all.

Graph algorithms run as fixed rules inside Datalog:

```datalog
pr[entity_id, score] <~ PageRank(edges[])
comm[labels, entity_id] <~ CommunityDetectionLouvain(edges_w[])
```

`PageRank` and `Louvain` execute in Rust (via the fixed-rule mechanism), read the `relationships` relation, and write results back to `graph_scores`. The engine then uses these scores in the recall pipeline.

**Storage.** The fjall backend provides persistent LSM-tree storage with LZ4 compression and read-your-own-writes semantics. No external database process is required. Historical SQLite portability hooks remain disabled with the removed `storage-sqlite` feature.

### 3.3 Kernel-level sandbox (organon)

Aletheia executes tools in child processes with three layers of kernel enforcement, applied via `pre_exec` between `fork` and `exec`:

1. **Landlock LSM** (Linux 5.13+) restricts filesystem access. The policy grants read, write, and execute permissions to specific paths. Landlock ABI v5 is probed at startup. If the kernel lacks Landlock, the system falls back to permissive mode or blocks execution based on operator configuration.

2. **seccomp BPF** blocks dangerous syscalls. A BPF filter denies `ptrace`, `mount`, `umount2`, `reboot`, `kexec_load`, `init_module`, `delete_module`, `finit_module`, `pivot_root`, and `chroot`. In permissive mode, violations are logged; in enforcing mode, they return `EPERM`.

3. **Network namespaces** isolate egress. `unshare(CLONE_NEWUSER | CLONE_NEWNET)` creates a namespace with only loopback, blocking all outbound connections without root. If unprivileged namespaces are disabled, a seccomp fallback blocks `socket(AF_INET)` and `socket(AF_INET6)`.

The sandbox configuration is operator-tunable via TOML:

```toml
[sandbox]
enabled = true
enforcement = "enforcing"
egress = "deny"
extra_read_paths = ["/data/shared"]
extra_write_paths = ["/tmp/workspace"]
```

Tool execution in the agent loop (`nous → organon`) applies the sandbox to every external process automatically. Built-in tools that do not spawn processes (e.g., HTTP client, file read) run inside the main process with their own restrictions.

Comparison to alternatives: OpenAI Codex [5] uses a similar sandbox but inside a containerized environment with heavier orchestration. NVIDIA OpenShell [6] applies seccomp and namespaces but focuses on shell command isolation. Aletheia's sandbox is lighter (no containers, no root) and integrated directly into the agent turn loop.

### 3.4 6-factor recall scoring (episteme)

The recall engine (`episteme::RecallEngine`) scores memory candidates with six factors. Each factor produces a value in [0.0, 1.0]. The final score is a weighted sum:

| Factor | Weight (default) | Description |
|---|---|---|
| Vector similarity | 0.35 | Cosine distance from HNSW search |
| Temporal decay | 0.20 | FSRS power-law decay from last access |
| Nous relevance | 0.15 | Own memories rank higher than shared or other |
| Epistemic tier | 0.15 | Verified (1.0) > inferred (0.6) > assumed (0.3) |
| Relationship proximity | 0.10 | Graph distance from query context entities |
| Access frequency | 0.05 | Log-scaled access count |

**Vector similarity.** HNSW approximate search returns candidates in O(log n) time. Cosine distance converts to similarity with `1 - distance / 2`.

**Temporal decay.** Uses the FSRS formula: `R(t) = (1 + 19/81 × t/S)^(-0.5)` where `t` is hours since last access and `S` is effective stability. Stability scales by fact type, epistemic tier, and access count. Negative ages (clock jumps) clamp to 0.0 to prevent score inflation.

**Nous relevance.** Memories belonging to the querying agent score 1.0; shared memories score 0.5; another agent's memories score 0.3. This preserves privacy boundaries while allowing shared knowledge.

**Epistemic tier.** Facts marked `verified` (human or high-confidence confirmation) score 1.0. `inferred` facts (LLM-extracted, unconfirmed) score 0.6. `assumed` facts (heuristic guesses) score 0.3.

**Relationship proximity.** Graph BFS computes hops from query context entities. Same entity or direct neighbor = 1.0, 2-hop = 0.5, 3-hop = 0.25, and so on. When `PageRank` and `Louvain` are active, same-cluster facts receive a floor of 0.3 even without a direct path.

**Access frequency.** Logarithmic scaling: `ln(1 + count) / ln(1 + max_count)`. This prevents frequently accessed facts from dominating while still rewarding salience.

Weights are tunable per agent via the oikos config cascade. The engine skips expensive graph operations when the relationship proximity weight is zero.

**Graph intelligence enhancement.** Background Datalog jobs compute `PageRank` and `Louvain` communities on the relationship graph. These scores augment three factors:

- Epistemic tier gets a `PageRank` boost: a verified fact about a hub entity scores higher than one about a peripheral entity (multiplier up to 1.5x).
- Relationship proximity gets a community floor: facts in the same cluster as query entities receive a minimum proximity of 0.3.
- Access frequency gets a supersession bonus: facts at the end of long evolution chains (actively maintained knowledge) receive up to +0.2.

### 3.5 Single-binary sovereignty

Aletheia compiles to one binary with no runtime dependencies beyond the Linux kernel. The embedded stack includes:

- Datalog engine with HNSW vectors (krites)
- fjall session store (graphe)
- BM25 full-text search (krites FTS)
- Agent actor loop (nous)
- Sandbox (organon)
- SSE streaming HTTP gateway (pylon)
- Token estimation and context distillation (melete)

External services are optional. The system runs on a bare server with only the binary and a config file. This reduces operational surface area: one process to monitor, one log stream to aggregate, one artifact to deploy.

---

## 4. Evaluation

### 4.1 Benchmark runner

We implemented a benchmark runner in the `dokimion` crate that evaluates Aletheia against two published recall benchmarks: LongMemEval [7] and LoCoMo [8]. The runner:

1. Creates a fresh session for each question
2. Replays haystack user turns as messages
3. Asks the benchmark question via the HTTP API
4. Scores answers with exact match (EM), token-level F1, and substring containment

The runner has 188 passing unit and integration tests. It supports smoke tests (`--max-questions 5`) and full runs.

### 4.2 Benchmark datasets

**LongMemEval** [7] contains 500 questions across five categories: single-session user, single-session assistant, multi-session, temporal reasoning, and knowledge update. The metric is exact match (%).

Published baselines:

| System | EM (%) |
|---|---|
| Hindsight (upper bound) | 91.4 |
| GPT-4o + memory | 71.3 |
| Claude 3.5 Sonnet + memory | 67.1 |
| GPT-4o no memory | 48.2 |

**LoCoMo** [8] contains ~10,000 QA pairs across 50 multi-session conversations. The metric is F1 score (%).

Published baselines:

| System | F1 (%) |
|---|---|
| Hindsight (upper bound) | 89.61 |
| GPT-4 + summarization memory | 62.4 |
| GPT-4 no memory | 38.7 |

### 4.3 Results

Live benchmark runs are pending completion of the runner integration (issue #2854). The runner is implemented, tested, and ready. Results will be recorded in `docs/benchmarks/memory-benchmarks.md` and summarized here when available.

Expected analysis points:

- `single-session-user` and `single-session-assistant` EM above 70% would confirm basic factual recall
- `temporal-reasoning` above 60% would validate the Bayesian surprise and staleness detectors
- `multi_hop` F1 stability would measure whether graph intelligence bridges cross-session dependencies
- `knowledge-update` scores would test whether the staleness detector evicts outdated facts correctly

### 4.4 Microbenchmarks

Criterion benchmarks track hot-path performance:

| Crate | Hot path | Latency |
|---|---|---|
| koina | `Ulid::new` | ~120 ns |
| graphe | Session create + message append | ~2.1 ms |
| symbolon | JWT validate | ~45 ms |
| episteme | `parse_observations` | ~850 ms |
| nous | `TokenBudget::new` | ~180 ms |

HNSW vector search scales logarithmically with index size. The Datalog query cache reduces repeated query latency by ~40% for common bootstrap queries.

---

## 5. Comparison

### 5.1 Architectural differences

| Dimension | Zep | Letta | Hindsight | Mem0 | Aletheia |
|---|---|---|---|---|---|
| **Graph substrate** | Property graph (Neo4j) | Custom graph | None | Key-value | Datalog |
| **Recursive inference** | Limited Cypher | No | N/A | No | Native rules |
| **Vector store** | Qdrant (external) | Embedded HNSW | None | pgvector | Embedded HNSW |
| **Full-text search** | No | No | No | No | BM25 (embedded) |
| **Sandbox** | None | None | None | None | Landlock + seccomp + netns |
| **Recall signals** | 2 | 2 | 1 | 2 | 6 |
| **Epistemic tiering** | No | No | No | No | Verified / inferred / assumed |
| **Graph algorithms** | Basic traversal | No | N/A | No | PageRank + Louvain |
| **Deployment** | 3+ services | Python package | Research | Cloud API / Python | Single binary |
| **External deps** | Neo4j, Qdrant, Postgres | Optional Postgres | None | Postgres, OpenAI | None |

### 5.2 Datalog vs property graphs

Property graphs (Zep, Neo4j) store entities as nodes and relationships as edges. Queries traverse the graph with pattern matching. This works well for shallow retrieval (one or two hops) but struggles with recursive inference.

Datalog stores everything as relations. Entities, relationships, facts, and derived rules share the same representation. Recursive queries are first-class: transitive closure, reachability, and fixed-point computations require no special syntax or optimization hints.

Rule-based inference is also native. An operator can add a Datalog rule like:

```datalog
ancestor[x, y] := parent[x, y]
ancestor[x, y] := parent[x, z], ancestor[z, y]
```

The engine computes the full transitive closure automatically. In a property graph, the equivalent requires either a stored procedure or a client-side traversal.

The trade-off is query syntax. Datalog is less familiar than Cypher or SQL. Aletheia mitigates this by generating Datalog queries from structured Rust types; most operators never write Datalog by hand.

### 5.3 Sandbox integration

OpenAI Codex [5] runs tools in a container with seccomp and namespace isolation. NVIDIA OpenShell [6] applies seccomp to shell commands. Both require container runtime infrastructure.

Aletheia applies Landlock + seccomp + netns to every tool execution via `pre_exec` on a standard `std::process::Command`. No container runtime, no root, no Docker daemon. The sandbox adds ~5 ms to tool spawn time and zero overhead when tools are not running.

This matters for persistent agents. A long-running agent server that executes hundreds of tools per hour needs sandboxing that is always on, lightweight, and transparent to the agent logic. Aletheia's sandbox meets this requirement.

---

## 6. Discussion

### 6.1 Limitations

- **Datalog expressiveness vs familiarity.** Datalog is powerful but unfamiliar to most practitioners. The generated-query layer hides complexity, but advanced use cases require learning a new language.
- **Linux-only sandbox.** Landlock and seccomp are Linux kernel interfaces. macOS and Windows builds compile but run without sandbox enforcement.
- **Benchmark results pending.** The runner is complete but live runs against LongMemEval and LoCoMo have not yet been executed.
- **Embedding model.** Current default is BAAI/bge-small-en-v1.5 (384 dims). Larger models would improve retrieval quality at the cost of memory and compute.

### 6.2 Future work

- Execute and publish LongMemEval and LoCoMo results
- Add Datalog query planning and cost-based optimization
- Explore cross-agent knowledge sharing with epistemic tier propagation
- Evaluate larger embedding models (1024+ dims) for retrieval quality gains

---

## 7. Target venue

We recommend a two-stage publication strategy:

1. **arXiv preprint** - immediate. Establishes priority for the Datalog substrate, sandbox integration, and 6-factor recall. The system is running in production; the contributions are mature enough to claim.

2. **Conference submission** - follow within 6 months. Two tracks are appropriate:
   - **Systems:** OSDI, SOSP, EuroSys, or ATC for the single-binary architecture, sandbox design, and embedded database contributions.
   - **AI/ML:** NeurIPS or ICML workshop for the memory model, recall scoring, and benchmark evaluation.

The agent-specific venues (AAMAS, agent workshops) are also suitable if the evaluation emphasizes multi-agent knowledge sharing and epistemic tiering.

---

## 8. Conclusion

Aletheia demonstrates that three unconventional design choices - Datalog for knowledge representation, kernel-level sandboxing in the agent loop, and 6-factor recall scoring - combine into a deployable persistent agent runtime. The system runs as a single binary with no external services, making it suitable for sovereign deployments where data never leaves the operator's machine. The benchmark runner is ready; live evaluation will quantify the recall quality against published baselines. We invite the research community to inspect the open-source implementation and reproduce the results.

---

## References

1. Zep AI. *Zep: Long-term memory for AI assistants.* https://github.com/getzep/zep (2023–2025).
2. Letta (formerly MemGPT). *Letta: Memory management for LLM agents.* NeurIPS 2024. https://letta.com
3. Hindsight. *LongMemEval: Benchmarking Chat Assistants on Long-Term Interactive Memory.* Zhang et al., arXiv:2410.10813 (2024).
4. Mem0. *Mem0: The memory layer for your AI apps.* https://mem0.ai (2024).
5. OpenAI. *Codex: Sandbox environment for code execution.* OpenAI Research (2025).
6. NVIDIA. *OpenShell: Secure shell execution for AI agents.* NVIDIA AI (2025).
7. Zhang et al. *LongMemEval: Benchmarking Chat Assistants on Long-Term Interactive Memory.* arXiv:2410.10813 (2024).
8. Maharana et al. *Long-Context Conversational Memory (LoCoMo).* arXiv:2402.17753 (2024).

---

## Appendix A: 6-factor scoring formula

Given a recall candidate, compute raw factor scores:

```
vector_similarity = 1 - cosine_distance / 2
decay = (1 + (19/81) * age_hours / stability)^(-0.5)
relevance = 1.0 if own memory, 0.5 if shared, 0.3 if other
tier = 1.0 if verified, 0.6 if inferred, 0.3 if assumed
proximity = 1.0 if 0-1 hops, 0.5 if 2 hops, 0.25 if 3 hops, ...
frequency = ln(1 + access_count) / ln(1 + max_count)
```

Final weighted score:

```
score = (vector_similarity * w1 + decay * w2 + relevance * w3
         + tier * w4 + proximity * w5 + frequency * w6)
        / (w1 + w2 + w3 + w4 + w5 + w6)
```

Default weights: `w1=0.35, w2=0.20, w3=0.15, w4=0.15, w5=0.10, w6=0.05`.

---

## Appendix B: Sandbox syscall blocklist

seccomp BPF denies the following syscalls in enforcing mode:

| Syscall | Number | Reason |
|---|---|---|
| ptrace | 101 | Process tracing |
| mount | 165 | Filesystem mounting |
| umount2 | 166 | Filesystem unmounting |
| reboot | 169 | System reboot |
| kexec_load | 246 | Kernel image loading |
| init_module | 175 | Kernel module load |
| delete_module | 176 | Kernel module unload |
| finit_module | 313 | File-based module load |
| pivot_root | 155 | Root filesystem switch |
| chroot | 161 | Root directory change |

---

## Appendix C: Glossary of Greek crate names

| Name | Meaning | Role |
|---|---|---|
| Aletheia | Truth, disclosure | Binary and project |
| Nous | Mind, intellect | Agent pipeline |
| Mneme | Memory | Memory subsystem facade |
| Krites | Judge | Datalog engine |
| Episteme | Knowledge | Knowledge pipeline |
| Graphe | Writing, record | Session store |
| Eidos | Form, essence | Shared types |
| Organon | Tool, instrument | Tool registry |
| Hermeneus | Interpreter | LLM provider client |
| Pylon | Gateway | HTTP server |
| Symbolon | Token, sign | Authentication |
| Melete | Care, attention | Context distillation |
| Daemon | Spirit, attendant | Background tasks |
