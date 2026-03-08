# Pattern Detection for Skill Auto-Capture

Research document for Aletheia's post-session skill extraction pipeline.

## 1. Executive Summary

**Recommended approach: hybrid two-phase pipeline with human-in-the-loop.**

Phase 1 (cheap, rule-based) scores each completed session against structural heuristics — minimum tool-call count, success signal, cross-session recurrence. Phase 2 (LLM-based) analyzes qualifying traces to extract abstract skill definitions in SKILL.md format.

The critical insight from the literature: **curated skills substantially improve agent success rates, while self-generated skills can degrade them** (SoK: Agentic Skills, 2026). Quality gating is more important than capture volume. Start with a conservative capture threshold (Rule of Three — only promote patterns seen in 3+ successful sessions) and add sophistication iteratively.

Aletheia's existing architecture is well-positioned for this. The `TurnResult` structure already captures tool call sequences with names, inputs, results, timing, and error status. The `ExtractionEngine` in mneme provides the pattern for LLM-based post-session analysis. The `KnowledgeStore` can persist skill facts with confidence scores and access tracking. The hook system supports event-driven triggers at session boundaries.

**Migration path:** v1 heuristics-only → v2 add LLM validation → v3 add embedding-based retrieval → v4 add clustering for discovery.

---

## 2. Literature Review

### 2.1 Process Mining

Process mining discovers structured workflow models from event logs. The event log schema (case_id = session_id, activity = tool_call, timestamp) maps directly to agent execution traces.

**Foundational work:**

- **van der Aalst, "Workflow Mining" (IEEE TKDE, 2004)** — Established the field. Showed that Petri nets and process trees can be automatically discovered from event logs. Defined the four quality criteria for discovered models: fitness (does it replay the log?), precision (does it allow only observed behavior?), generalization (does it extend beyond the log?), simplicity. These criteria directly apply to evaluating candidate skill patterns.

- **Augusto et al., "Automated Discovery of Process Models: Review and Benchmark" (IEEE TKDE, 2019)** — Most rigorous comparison of discovery algorithms. **Inductive Miner** and **Split Miner** consistently produced the best fitness-precision tradeoff. Alpha Miner performed worst on noisy real-world data. Applicability: **high** — directly answers "which algorithm?" for our use case.

- **Leemans et al., "Discovering Block-Structured Process Models" (Petri Nets, 2013)** — Introduced the Inductive Miner. Divide-and-conquer approach produces process trees with sequence, choice, parallel, and loop operators. **Guarantees soundness** — the output is always a valid executable workflow. Process tree output maps naturally to parameterized skill templates.

- **Tax et al., "Mining Local Process Models" (Information Systems, 2018)** — Instead of mining one global model, mine small reusable fragments (Local Process Models / LPMs). This is the process mining equivalent of "skills" — small composable workflow fragments. Applicability: **high** — handles the variability of LLM agent traces better than global models, and works with smaller datasets.

**Applicability to agent traces:** Agent traces map cleanly to the process mining event log format. Inductive Miner handles non-determinism via noise thresholds. Variable-length traces are handled natively. The main limitation is data volume — process mining typically assumes hundreds to thousands of traces. LPMs are better suited for smaller datasets (dozens of sessions).

### 2.2 Sequential Pattern Mining

Sequential pattern mining discovers frequently occurring ordered subsequences in sequence databases.

- **Pei et al., "PrefixSpan" (ICDE, 2001)** — Projection-based mining without candidate generation. Recursively projects the database by frequent prefixes. Best overall performance in time and memory vs. GSP and SPADE. For sequences of 5-50 tool calls with moderate dataset sizes, PrefixSpan is the clear winner. Applicability: **high**.

- **Zaki, "SPADE" (Machine Learning, 2001)** — Vertical data representation with equivalence classes. Only three database scans. Viable but slower and more memory-hungry than PrefixSpan. Applicability: **medium**.

- **Srikant & Agrawal, "GSP" (EDBT, 1996)** — Extended Apriori to sequences. Pioneering but generates many candidates. Only relevant if time-gap constraints between tool calls are needed (GSP supports min/max gap natively). Applicability: **low**.

| Algorithm | Time | Memory | Strengths | Weaknesses |
|-----------|------|--------|-----------|------------|
| PrefixSpan | Best | Best | No candidate generation | Complex to implement from scratch |
| SPADE | Good | Moderate | Only 3 DB scans | ID-lists can grow large |
| GSP | Worst | Worst | Native time constraints | Candidate explosion |

**Threshold selection:** Fixed minimum support is problematic — too high misses rare patterns, too low generates noise. A practical starting point: minimum support = 3 sessions, minimum pattern length = 3 tool calls. The top-k alternative (find the k most frequent patterns of minimum length L) avoids the threshold selection problem entirely.

### 2.3 LLM Agent Skill Learning

This is the most directly relevant literature — systems that build skill libraries from agent execution experience.

- **Wang et al., "Voyager" (NeurIPS, 2023)** — First LLM-powered agent with a growing skill library. Three components: automatic curriculum, skill library stored as executable code indexed by docstring embeddings, iterative self-verification. Complex skills compose simpler ones. Only verified-successful skills are stored. 3.3x more unique items, 15.3x faster milestones vs. prior SOTA. Applicability: **high** — closest prior art to Aletheia's needs.

- **"SoK: Agentic Skills — Beyond Tool Use in LLM Agents" (arXiv, Feb 2026)** — Most comprehensive survey. Maps the full skill lifecycle: discovery → practice → distillation → storage → composition → evaluation → update. Critical finding: **curated skills improve agent success, self-generated skills may degrade it**. Quality control on auto-mined skills is essential. Applicability: **high** — the reference framework.

- **"Agent Skills for LLMs: Architecture, Acquisition, Security" (arXiv, 2026)** — Defines three acquisition modalities: human authoring (most reliable), LLM-generated (fast, quality-variable), experience-distilled (Aletheia's approach). Applicability: **high**.

- **"SICA: A Self-Improving Coding Agent" (ICLR Workshop, 2025)** — Agent that edits its own codebase. Uses callgraph + event stream analysis to detect repeated work. Autonomously discovered diff-based editing tools and AST-based symbol locators. Demonstrates that pattern detection from execution traces is viable. Applicability: **high** — closest architectural analog.

- **"SkillRL: Evolving Agents via Recursive Skill-Augmented RL" (arXiv, 2026)** — Experience-based skill distillation from both successes and failures. Hierarchical SkillBank with General Skills (universal) and Task-Specific Skills (category-level). 15.3% improvement over baselines. 10-20% token compression vs. raw trajectory storage. Applicability: **high**.

- **"CASCADE" (arXiv, Dec 2025)** — Cumulative skill creation for scientific research agents. 93.3% success rate on 116-task benchmark. Demonstrates skill sharing across agents. Applicability: **medium-high**.

- **Xiang et al., "AFlow" (ICLR 2025 Oral)** — Reformulates workflow optimization as search over code-represented workflows using MCTS. Introduces "operators" — reusable workflow fragments. 5.7% average improvement over SOTA at 4.55% inference cost. Applicability: **medium** — the operator concept (reusable fragments as code) is relevant.

### 2.4 Program Abstraction from Traces

- **"Syren" (PLDI, 2025)** — Synthesizes programs from traces of side-effecting functions. Start with a correct-but-specific program, apply proven-sound rewrites to generalize. The key insight: generalization is a rewrite problem, not a synthesis problem. Directly applicable to going from "this specific tool sequence" to "this abstract skill pattern."

- **Plotkin, "Anti-unification" (1970)** — Computes the least general generalization (LGG) of two expressions. For two concrete workflow instances, anti-unification identifies common structure while replacing differing elements with variables. Extended in modern work to nominal, higher-order, and constraint-based settings (Cerna & Kutsia, IJCAI 2023).

---

## 3. Approach Comparison

| Criterion | Rule-based | LLM-based | Hybrid | Embedding Clustering |
|-----------|-----------|-----------|--------|---------------------|
| **Accuracy** | Low-medium. Catches structural patterns, misses semantic similarity | High. Understands intent, can abstract | High. Rules filter, LLM validates | Medium-high. Discovers unexpected patterns |
| **Cost per session** | Negligible (microseconds) | $0.01-0.05 per session (2-5K tokens) | $0.001-0.01 (rules reject 80-95%) | Embedding cost + periodic clustering |
| **Implementation complexity** | Low. Pattern matching on token sequences | Medium. Prompt engineering + output parsing | Medium-high. Two systems to build | High. Embedding pipeline + clustering + extraction |
| **Minimum data needed** | Works immediately | Works immediately (single session) | Works immediately | Hundreds of traces for meaningful clusters |
| **False positive rate** | Medium-high (structural matches without semantic understanding) | Low (model understands context) | Low (double-filtered) | Medium (cluster boundaries imprecise) |
| **False negative rate** | High (novel patterns missed) | Low (model generalizes) | Low-medium | Low (discovers patterns rules wouldn't find) |
| **Handles novel patterns** | No — only predefined templates | Yes — semantic understanding | Yes — via LLM phase | Yes — clustering reveals structure |
| **Maintenance burden** | High — rules need updating | Low — model adapts | Medium | Medium — retraining/reclustering needed |

**Verdict:** Hybrid is the clear winner for Aletheia. Rules provide cheap, immediate filtering. LLM provides quality abstraction for the candidates that pass. The cost reduction (80-95% fewer LLM calls) makes it economically viable to analyze sessions at scale.

---

## 4. Recommended Architecture

### Phase 1: Heuristic Filter (v1, immediate)

Post-session, score the trace against structural criteria:

```
fn is_skill_candidate(trace: &SessionTrace) -> bool {
    let distinct_tools = trace.tool_calls.iter().map(|t| &t.name).collect::<HashSet<_>>().len();
    let has_success = trace.final_status == Success;
    let min_complexity = trace.tool_calls.len() >= 5;
    let diverse_tools = distinct_tools >= 3;
    let has_pattern = contains_known_pattern(&trace.tool_calls);

    has_success && min_complexity && diverse_tools && (has_pattern || recurrence_count(trace) >= 3)
}
```

Known patterns to detect:
- **Diagnostic pattern**: `Read → Grep/Search → Read → Analyze` (information gathering)
- **Fix pattern**: `Read → Edit → Bash(test) → [Edit → Bash(test)]*` (iterative fix)
- **Refactor pattern**: `Grep → Read* → Edit* → Bash(test)` (multi-file change)
- **Research pattern**: `WebSearch → WebFetch → Read → Write` (research and document)

Store candidate traces with a `candidate_count` — increment each time the same tool-call signature appears. Promote to Phase 2 when count reaches 3 (Rule of Three).

### Phase 2: LLM Validation + Extraction (v2)

When a candidate reaches the promotion threshold, send the trace(s) to an LLM:

1. Present the 3+ concrete traces that matched
2. Ask: "Is this a reusable skill pattern, or coincidental similarity?"
3. If yes: generate a SKILL.md with parameterized steps
4. Human reviews and accepts/rejects

Use a smaller model (Haiku) for the binary classification step. Use a larger model (Sonnet/Opus) only for skill definition generation on accepted candidates.

### Phase 3: Embedding-Based Retrieval (v3)

Once 20+ skills exist, embed skill descriptions for retrieval:

- At session start, embed the task description
- Find top-k similar skills by cosine similarity
- Load relevant skills into context
- Track whether loaded skills were followed (quality signal)

Use `fastembed-rs` or sentence-transformers for embeddings. Voyager's pattern: index by docstring embedding, retrieve by task + environment state.

### Phase 4: Clustering for Discovery (v4)

Once hundreds of traces accumulate:

- Embed all execution traces (act2vec/trace2vec approach)
- Cluster with HDBSCAN
- Extract representative patterns from each cluster
- Surface clusters that don't match existing skills as candidates

This discovers patterns that the heuristic rules wouldn't catch.

### Integration with Aletheia Architecture

```
Pipeline Finalize
       │
       ▼
  TurnResult { tool_calls, signals, usage }
       │
       ▼
  ┌─────────────────┐
  │ Heuristic Filter │ ← Phase 1: cheap structural scoring
  └────────┬────────┘
           │ (candidates only)
           ▼
  ┌─────────────────────┐
  │ Candidate Tracker    │ ← Count recurrences, store trace refs
  │ (KnowledgeStore fact)│
  └────────┬────────────┘
           │ (count >= 3)
           ▼
  ┌─────────────────┐
  │ LLM Extractor    │ ← Phase 2: validate + generate SKILL.md
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Human Review     │ ← Accept / reject / edit
  └────────┬────────┘
           │
           ▼
  instance/shared/skills/<skill-name>/SKILL.md
```

**Natural integration points:**
- **Post-finalize hook** — trigger heuristic scoring after each turn is persisted
- **ExtractionEngine extension** — add skill-specific extraction prompts alongside entity/relationship extraction
- **KnowledgeStore** — persist candidate patterns as facts with `fact_type: "skill_candidate"`, confidence, and access tracking
- **Hook system** — use `session:end` event to trigger cross-turn analysis

---

## 5. Reusability Heuristics

Concrete criteria for "is this pattern worth capturing?":

### Must-Pass Gates (all required)

| Gate | Criterion | Rationale |
|------|-----------|-----------|
| **Success** | Session ended without error; tests/build passed if applicable | Failed patterns teach wrong approaches |
| **Complexity** | ≥5 tool calls with ≥3 distinct tool types | Trivial sequences aren't worth the context budget |
| **Recurrence** | Pattern seen in ≥3 independent sessions | Rule of Three — prevents premature abstraction |
| **Novelty** | No existing skill matches within 0.85 embedding similarity | Avoid duplicates |

### Scoring Signals (weighted sum for ranking)

| Signal | Weight | Measurement |
|--------|--------|-------------|
| Cross-session frequency | 0.3 | Count of matching sessions |
| Task-type diversity | 0.2 | Number of distinct task categories where pattern appeared |
| Step count | 0.1 | Higher complexity = more value in capturing |
| Backtrack ratio | -0.2 | High ratio of retries/reverts = fragile pattern |
| Tool error rate | -0.2 | High error rate during pattern = not robust |

### Anti-Patterns to Reject

- **Single-file edits** — read one file, edit it, done. Too simple.
- **Debugging spirals** — long sequences of failed attempts. Not reusable.
- **Configuration-specific** — patterns that depend on a specific project structure.
- **One-off research** — web searches for a specific topic. Not generalizable.

---

## 6. Quality Framework

### Lifecycle Stages

```
Candidate → Promoted → Active → Proven → (Stale → Retired)
```

| Stage | Entry Criteria | Exit Criteria |
|-------|---------------|---------------|
| **Candidate** | Heuristic filter passes | Recurrence ≥ 3, LLM validates |
| **Promoted** | LLM confirms reusable pattern | Human accepts |
| **Active** | Human-accepted SKILL.md persisted | 5+ successful uses OR 28 days without use |
| **Proven** | ≥5 sessions used this skill successfully | Confidence drops below threshold |
| **Stale** | 28 days without use OR codebase drift invalidates references | Manual review → retire or refresh |
| **Retired** | Confirmed no longer useful | Archived, not deleted |

### Quality Signals

**Usage tracking (per skill):**
- `loaded_count` — how often the skill was loaded into agent context
- `followed_count` — how often the agent's execution matched the skill's pattern
- `success_after_load` — task success rate when skill was in context
- `ignored_count` — loaded but not followed (3 consecutive ignores → flag for review)

**Outcome-based evaluation:**
- Compare success rates: sessions with skill loaded vs. sessions on similar tasks without
- If `success_after_load` < baseline success rate → skill may be harmful
- If `followed_count / loaded_count` < 0.3 → skill isn't relevant to what it's being retrieved for

**Agent self-assessment:**
- After completing a task where a skill was loaded, ask: "Did the loaded skill help? (yes/no/partially)"
- Weight this lower than objective signals (test pass/fail, build success)

### Decay Strategy

Adopt the GitHub Copilot model: **28-day TTL with renewal on successful use.**

- Every skill starts with a 28-day countdown
- Each successful use resets the countdown
- Skills used regularly never expire
- Unused skills auto-retire after 28 days
- Seasonal skills (release management, quarterly tasks) may need manual exemption

For Aletheia specifically, the `stability_hours` field in the KnowledgeStore fact model maps directly to this — set initial stability to 672 hours (28 days), refresh on use.

### Feedback Loop

```
Skill loaded → Agent executes → Outcome recorded
                                       │
                    ┌──────────────────┬┘
                    ▼                  ▼
              Success?            Followed?
              yes → refresh TTL   yes → signal relevance
              no  → decrement     no  → signal irrelevance
                    confidence          (3x → flag)
```

### Contextual Bandit for Selection

When multiple skills could apply to a task, use Thompson Sampling:

- **Arms** = candidate skills
- **Context** = task type, crate being modified, tool sequence so far
- **Reward** = task success after skill loaded

Thompson Sampling naturally handles exploration (new skills get tried) and exploitation (proven skills get preferred). Model uncertainty via Beta distributions — a skill with 2/2 successes isn't necessarily preferred over one with 50/60.

---

## 7. Implementation Notes

### Data Model Extension

Minimal additions to existing mneme types:

```rust
// New fact_type values for KnowledgeStore
const SKILL_CANDIDATE: &str = "skill_candidate";
const SKILL_ACTIVE: &str = "skill_active";

// Skill candidate tracking
struct SkillCandidate {
    tool_signature: Vec<String>,    // ordered tool names
    session_refs: Vec<SessionId>,   // sessions where pattern appeared
    recurrence_count: u32,
    first_seen: Timestamp,
    last_seen: Timestamp,
}

// Skill quality tracking
struct SkillMetrics {
    loaded_count: u32,
    followed_count: u32,
    success_count: u32,
    ignored_count: u32,
    last_used: Option<Timestamp>,
    confidence: f64,
}
```

### Tool Signature Hashing

To detect recurrence, hash the tool-call sequence into a comparable signature:

1. Extract ordered tool names: `["Read", "Grep", "Read", "Edit", "Bash"]`
2. Collapse consecutive duplicates: `["Read", "Grep", "Read", "Edit", "Bash"]`
3. Hash with a fuzzy matching tolerance (allow ±1 tool insertions/deletions)

For fuzzy matching, use sequence alignment (Needleman-Wunsch via the `seal` crate) with a similarity threshold of 0.8.

### Post-Session Analysis Pipeline

```rust
// Pseudocode for the post-session hook
async fn analyze_session(session: &CompletedSession) -> Option<SkillCandidate> {
    let trace = extract_tool_trace(session);

    // Phase 1: Heuristic filter
    if !passes_gates(&trace) {
        return None;
    }

    let signature = compute_signature(&trace);

    // Check recurrence
    let existing = knowledge_store.find_candidate(&signature).await;
    match existing {
        Some(mut candidate) => {
            candidate.recurrence_count += 1;
            candidate.session_refs.push(session.id);
            candidate.last_seen = now();

            if candidate.recurrence_count >= 3 {
                // Phase 2: Queue for LLM extraction
                queue_for_extraction(candidate).await;
            }

            knowledge_store.update_candidate(candidate).await;
            Some(candidate)
        }
        None => {
            let candidate = SkillCandidate::new(signature, session.id);
            knowledge_store.insert_candidate(candidate).await;
            Some(candidate)
        }
    }
}
```

### Skill Definition Format

Use the established SKILL.md format with YAML frontmatter:

```yaml
---
name: diagnose-and-fix-lint-errors
description: Diagnose and fix clippy lint errors in a Rust crate
tools: [Read, Grep, Edit, Bash]
triggers:
  - clippy warning
  - lint error
  - cargo clippy
confidence: 0.85
source: auto-captured
sessions: [session_abc, session_def, session_ghi]
---

## Steps

1. Run `cargo clippy --workspace` to identify all warnings
2. For each warning:
   a. Read the source file at the reported location
   b. Understand the lint rule and suggested fix
   c. Apply the fix using targeted edits
   d. Re-run clippy on the affected crate to verify
3. Run full workspace clippy to confirm zero warnings
4. Run tests for affected crates

## Parameters

- `${crate}` — the crate with lint errors (or `--workspace` for all)
- `${severity}` — warning level to target (default: all)

## Anti-patterns

- Do not suppress lints with `#[allow]` — use `#[expect]` with reason
- Do not batch-fix unrelated lints in a single edit — one logical fix per edit
```

### Deduplication at Storage Time

Before persisting a new skill:

1. Embed the skill description
2. Query existing skills for cosine similarity > 0.85
3. If match found:
   - If structural similarity (tool sequence Jaccard) > 0.7 → treat as version update
   - If only semantic similarity → flag for human review as potential duplicate
4. If no match → store as new skill

### Rust Crate Integration

The natural home for this is a new module within mneme (memory-related) or as a new crate if the scope warrants it:

- `crates/mneme/src/skills.rs` — skill candidate tracking, quality metrics, signature hashing
- `crates/mneme/src/skills/extract.rs` — LLM-based skill definition generation (extends ExtractionEngine pattern)
- `crates/mneme/src/skills/heuristics.rs` — rule-based filtering and pattern detection

Use the existing `ExtractionEngine` as the architectural template — it already does LLM-based extraction with structured output parsing and KnowledgeStore persistence.

---

## 8. Existing Tool Assessment

### Can Use Directly

| Tool | Language | What it provides | Integration path |
|------|----------|-----------------|------------------|
| **rust-rule-miner** | Rust | Sequential pattern mining (A→B→C), association rules, configurable time gaps | Direct dependency. Use for detecting recurring tool-call subsequences across sessions. |
| **seal** | Rust | Sequence alignment (Needleman-Wunsch, Smith-Waterman) | Direct dependency. Use for fuzzy comparison of tool-call sequences. |
| **fastembed-rs** | Rust | Fast embedding computation | Direct dependency for Phase 3. Embed skill descriptions for retrieval. |

### Can Use as Reference / Sidecar

| Tool | Language | What it provides | Integration path |
|------|----------|-----------------|------------------|
| **PM4Py** | Python | Process discovery algorithms (Inductive Miner, Heuristic Miner) | Python sidecar or subprocess for Phase 4 exploration. Not needed for v1. |
| **SPMF** | Java | 55+ pattern mining algorithms | Reference implementation. Use to validate rust-rule-miner results if needed. |
| **sentence-transformers** | Python | High-quality text embeddings | If fastembed-rs proves insufficient, use as Python sidecar for embedding. |

### Must Build

| Component | Why not available | Complexity |
|-----------|------------------|------------|
| **Heuristic filter** | Domain-specific to Aletheia's tool vocabulary | Low — straightforward pattern matching |
| **Candidate tracker** | Uses Aletheia's KnowledgeStore | Low — extends existing fact model |
| **LLM skill extractor** | Requires Aletheia-specific prompts | Medium — extends ExtractionEngine pattern |
| **Quality metrics tracker** | Domain-specific lifecycle management | Medium — new data model + hooks |
| **Skill retrieval** | Requires integration with pipeline's Recall stage | Medium — embedding index + context loading |

### Not Needed

| Tool | Why skip |
|------|----------|
| **ProM** | Java, desktop-oriented, too heavyweight |
| **Apache Airflow** | Workflow execution, not discovery |
| **Temporal** | Same — execution framework, not pattern mining |
| **Full process mining suite** | Overkill for v1. Agent trace volumes are too low to justify the infrastructure. PrefixSpan via rust-rule-miner covers the sequential mining need. |

---

## 9. Source Index

### Process Mining

| Source | Year | Relevance |
|--------|------|-----------|
| van der Aalst, "Workflow Mining" (IEEE TKDE) | 2004 | Foundational — event log schema maps to agent traces |
| Augusto et al., "Automated Discovery: Review and Benchmark" (IEEE TKDE) | 2019 | Algorithm comparison — Inductive Miner wins |
| Leemans et al., "Discovering Block-Structured Process Models" | 2013 | Inductive Miner algorithm — guaranteed-sound output |
| Tax et al., "Mining Local Process Models" (Information Systems) | 2018 | LPMs — small reusable workflow fragments = skills |
| van der Aalst, "Process Mining: Data Science in Action" (Springer) | 2016 | Textbook — quality framework (fitness, precision, generalization, simplicity) |

### Sequential Pattern Mining

| Source | Year | Relevance |
|--------|------|-----------|
| Pei et al., "PrefixSpan" (ICDE) | 2001 | Best algorithm for moderate datasets — no candidate generation |
| Zaki, "SPADE" (Machine Learning) | 2001 | Alternative — vertical representation, 3 DB scans |
| Srikant & Agrawal, "GSP" (EDBT) | 1996 | Historical — only if time-gap constraints needed |

### LLM Agent Skill Learning

| Source | Year | Relevance |
|--------|------|-----------|
| Wang et al., "Voyager" (NeurIPS) | 2023 | Gold standard — skill library with verification + embedding retrieval |
| "SoK: Agentic Skills" (arXiv 2602.20867) | 2026 | Comprehensive survey — lifecycle model, quality warnings |
| "Agent Skills for LLMs" (arXiv 2602.12430) | 2026 | Acquisition modalities — experience distillation is our approach |
| "SICA: Self-Improving Coding Agent" (ICLR Workshop) | 2025 | Closest architectural analog — pattern detection from tool event streams |
| "SkillRL" (arXiv) | 2026 | Hierarchical skill bank — distillation from successes and failures |
| "CASCADE" (arXiv 2512.23880) | 2025 | Cumulative skill creation — cross-agent skill sharing |
| Xiang et al., "AFlow" (ICLR Oral) | 2025 | Workflow optimization via MCTS — operator concept |
| "SAGE" (arXiv) | 2025 | Skill accumulation during RL — 26% fewer interaction steps |
| "SEAgent" (arXiv 2508.04700) | 2025 | Self-evolving agent — skill learning for novel software environments |
| "EXIF" (arXiv 2506.04287) | 2025 | Exploration-first skill discovery for language agents |

### Program Abstraction

| Source | Year | Relevance |
|--------|------|-----------|
| "Syren" (PLDI) | 2025 | Trace-to-program generalization via proven rewrites |
| Cerna & Kutsia, "Anti-unification Survey" (IJCAI) | 2023 | LGG for merging specific instances into abstract templates |
| Raza, "PBE using LGG" (AAAI) | 2014 | Programming by example via generalization |
| De Koninck et al., "act2vec, trace2vec" (BPM) | 2018 | Embedding approaches for execution traces |

### Production Systems

| Source | Year | Relevance |
|--------|------|-----------|
| GitHub, "Building an Agentic Memory System for Copilot" (blog) | 2026 | 28-day TTL, citation validation, 7% PR merge rate improvement |
| Anthropic, "Equipping Agents with Skills" (engineering blog) | 2025 | SKILL.md format, routing via YAML frontmatter |
| Vercel, "AGENTS.md Outperforms Skills" (blog) | 2025 | Compressed docs = full docs in performance; curated > auto-generated |

### Tools and Libraries

| Tool | Language | License | URL |
|------|----------|---------|-----|
| PM4Py | Python | GPL-3.0 | github.com/process-intelligence-solutions/pm4py |
| SPMF | Java | GPL-3.0 | philippe-fournier-viger.com/spmf |
| rust-rule-miner | Rust | — | crates.io/crates/rust-rule-miner |
| Rust4PM | Rust | Apache-2.0 | github.com/aarkue/rust4pm |
| seal | Rust | — | github.com/regexident/rust-seal |
| fastembed-rs | Rust | — | crates.io/crates/fastembed |
| sentence-transformers | Python | Apache-2.0 | sbert.net |
