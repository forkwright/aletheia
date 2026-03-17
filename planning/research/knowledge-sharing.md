# R716: Cross-agent knowledge sharing

## Question

Each nous has its own memory space. Facts are scoped by `nous_id`, sessions are per-agent, and behavioral patterns (instincts) are private. Cross-agent knowledge sharing would let agents access shared knowledge, learn from each other's experiences, and build collective understanding while preserving identity boundaries. What mechanisms should exist, what access controls are needed, and what should never be shared?

## Findings

### Current memory isolation model

**Fact scoping.** Every fact in the CozoDB knowledge store carries a `nous_id` field. When set to an agent's ID (e.g., `"syn"`), the fact is agent-scoped. When empty (`""`), the fact is shared. The recall engine's `score_relevance()` implements a soft visibility model:

| `nous_id` value | Relevance score | Meaning |
|-----------------|:---------------:|---------|
| Matches querying agent | 1.0 | Own memory, maximum boost |
| Empty string | 0.5 | Shared memory, moderate boost |
| Different agent | 0.3 | Other agent's memory, penalty |

This is a recall-time preference, not a storage-time restriction. All facts live in the same CozoDB instance. Any agent can technically retrieve any fact through vector or text search; the relevance factor just ranks own memories higher.

**Session isolation.** Session history in SQLite is scoped by `(nous_id, session_key)`. Each agent owns its sessions. The `SessionStore` queries filter by `nous_id`, so agent A cannot read agent B's conversation transcript through the normal API. This is code-level enforcement, not database-level (both agents' sessions live in the same SQLite file).

**Behavioral patterns.** The instinct system in `mneme/src/instinct.rs` records `ToolObservation` values tagged with `nous_id`. Behavioral patterns (tool success rates, context preferences) are aggregated per-agent and stored as `FactType::Preference` facts with the agent's `nous_id`. No mechanism exists to share or merge behavioral patterns across agents.

**Entity and relationship graph.** Entities and relationships in CozoDB are not scoped by `nous_id`. The entity graph is implicitly shared: if agent A extracts entity "Alice" and agent B extracts entity "Alice", they create (or update) the same node. Relationship edges are similarly global. Only the facts attached to entities carry agent scoping.

**Embeddings.** The HNSW vector index stores embeddings with a `nous_id` field, but vector search queries do not filter by it. All embeddings participate in nearest-neighbor search. The `nous_id` on embeddings affects only the relevance score of the associated fact.

**Summary of current isolation.**

| Layer | Scoping mechanism | Enforcement |
|-------|-------------------|-------------|
| Facts | `nous_id` field | Soft (recall scoring) |
| Sessions | `(nous_id, session_key)` | Code-level query filter |
| Entities | None (global) | Shared by design |
| Relationships | None (global) | Shared by design |
| Embeddings | `nous_id` field | Soft (recall scoring) |
| Instincts | `nous_id` in observations | Code-level aggregation |
| Cross-nous messages | Sender/receiver fields | Router delivery |

### Existing sharing mechanisms

Three mechanisms already enable some cross-agent knowledge flow:

**1. Shared facts (empty `nous_id`).** Any code path that inserts a fact with `nous_id = ""` makes it visible to all agents at 0.5 relevance weight. Currently, no extraction pipeline sets `nous_id` to empty; all extracted facts inherit the extracting agent's ID. Shared facts would need an explicit "publish" action.

**2. CrossNousRouter.** Point-to-point messaging between agents via `mpsc` channels. Supports fire-and-forget (`send`) and request-response (`ask` with timeout). Messages carry `from`, `to`, `content`, `expects_reply`, and delivery state tracking. The router maintains a ring-buffer audit log (default 1000 entries). This is an explicit, synchronous communication channel, not a knowledge-sharing mechanism, but it could serve as the transport for knowledge publication events.

**3. Entity/relationship graph.** Since entities and relationships are unscoped, agents implicitly share structural knowledge. Agent A extracting "Alice works-at Acme" and agent B extracting "Alice manages Bob" both contribute to the same graph. The graph intelligence layer (PageRank, community detection, relationship proximity scoring) operates on this shared structure.

### Designed sharing mechanisms

#### 1. Knowledge publication

**Concept.** An agent explicitly publishes a fact to the shared knowledge base. Publication changes the fact's `nous_id` from the agent's ID to empty, or creates a new shared copy with provenance metadata.

**Design.**

```
publish_fact(fact_id, publisher_nous_id) -> Result<SharedFactId>
  1. Validate fact exists and belongs to publisher
  2. Create shared copy: nous_id = "", source_nous_id = publisher
  3. Set provenance: published_by, published_at, original_fact_id
  4. Emit publication event to CrossNousRouter (broadcast)
  5. Return shared fact ID
```

**Trade-offs.**

| Approach | Pro | Con |
|----------|-----|-----|
| Copy-on-publish (new shared fact) | Original stays private, clean provenance | Duplication, drift between copies |
| Move-on-publish (clear `nous_id`) | No duplication | Original agent loses ownership boost |
| Link-on-publish (shared pointer) | No duplication, preserves ownership | Requires new relation type, complexity |

**Recommendation.** Copy-on-publish. The shared copy is a distinct fact with a `published_from` field pointing to the original. The original stays agent-scoped. Drift is acceptable because published knowledge is a snapshot; the publishing agent can republish if the fact evolves.

**Schema addition (Datalog).**

```
published_facts { shared_fact_id: String =>
    original_fact_id: String,
    published_by: String,
    published_at: String,
    review_status: String    -- pending | accepted | contested
}
```

#### 2. Knowledge subscription

**Concept.** An agent subscribes to topics, fact types, or entity categories. When another agent publishes knowledge matching a subscription, the subscriber receives a notification.

**Design.**

```
subscribe(nous_id, filter) -> SubscriptionId
  filter := {
    fact_types: [FactType],        -- e.g., [Skill, Relationship]
    entity_types: [String],        -- e.g., ["person", "project"]
    min_confidence: f64,           -- e.g., 0.8
    min_tier: EpistemicTier,       -- e.g., Verified
  }
```

Subscriptions are evaluated at publication time. When `publish_fact` runs, it checks all active subscriptions. Matching subscribers receive a `CrossNousMessage` with the published fact content. The subscriber's recall pipeline then includes the fact at shared relevance weight (0.5).

**Trade-offs.**

| Approach | Pro | Con |
|----------|-----|-----|
| Push (notify on publish) | Low latency, agent sees immediately | Noisy if many publications |
| Pull (query on recall) | No extra messages, demand-driven | Latency, agent unaware of new knowledge |
| Hybrid (push notification, pull content) | Balance of latency and noise | More complex |

**Recommendation.** Push notification with pull content. The CrossNousRouter delivers a lightweight notification ("new shared fact about entity X, type Skill"). The subscriber's recall pipeline picks up the actual fact content on next recall query. This avoids message bloat while ensuring agents know new knowledge exists.

#### 3. Federated queries

**Concept.** An agent queries across all agents' knowledge stores (or a subset) with access control. Returns results from multiple agents, ranked by the standard 6-factor recall engine.

**Design.** Federated queries do not require new infrastructure. The current recall engine already searches the entire CozoDB instance, which contains facts from all agents. The relevance factor (1.0/0.5/0.3) provides the access control. To make this more intentional:

```
recall_federated(query, options) -> Vec<RecallResult>
  options := {
    include_agents: Vec<NousId>,     -- whitelist (empty = all)
    exclude_agents: Vec<NousId>,     -- blacklist
    min_relevance_override: f64,     -- override 0.3 penalty
    require_tier: EpistemicTier,     -- minimum tier
  }
```

**Trade-offs.**

| Approach | Pro | Con |
|----------|-----|-----|
| Current model (soft scoring) | Already works, no changes needed | No opt-out, privacy is scoring-based |
| Explicit whitelist/blacklist | Agents control who sees their facts | Requires registration, maintenance |
| Per-fact visibility flags | Granular control | Schema change, extraction complexity |

**Recommendation.** Extend the existing model with per-fact visibility rather than building a separate federation layer. Add a `visibility` field to facts:

| Visibility | Meaning |
|------------|---------|
| `private` | Only the owning agent (current default behavior via `nous_id` scoring) |
| `shared` | All agents (current `nous_id = ""` behavior) |
| `published` | All agents, with provenance tracking |
| `restricted` | Named agents only (new) |

For `restricted` visibility, a `fact_access` relation maps fact IDs to authorized `nous_id` values.

#### 4. Shared knowledge base

**Concept.** A curated collection of facts that are always available to all agents. Distinct from ad-hoc publication: these are ground-truth facts that define the system's shared understanding.

**Design.** The shared knowledge base is the set of facts with `nous_id = ""` and `tier = Verified`. No new infrastructure needed. The distinction from published facts is curation: shared knowledge base facts are inserted by operators or verified by multiple agents, while published facts come from a single agent's extraction.

**Population methods.**
1. Operator insertion via HTTP API (current `POST /facts` endpoint)
2. Bootstrap files loaded at startup (TOML/YAML fact definitions)
3. Multi-agent verification: when N agents independently extract the same fact, promote to shared
4. Agent publication with review (publish -> other agents accept/contest)

**Multi-agent verification** is the most interesting. The extraction pipeline already detects when a new fact matches an existing one (via vector similarity). If three agents independently extract "Alice works at Acme" with confidence > 0.8, the system could auto-promote the fact to shared with `tier = Verified`.

**Verification query (Datalog).**

```datalog
?[content, count] :=
    *facts{content, nous_id, confidence, is_forgotten},
    nous_id != "",
    confidence >= 0.8,
    is_forgotten == false,
    count = count(nous_id)

:filter count >= 3
```

### Access control model

#### Visibility levels

| Level | Who can read | Who can write | Use case |
|-------|-------------|---------------|----------|
| Private | Owning agent only | Owning agent | Personal preferences, user secrets |
| Published | All agents | Publisher (immutable after publish) | Shared findings, learned patterns |
| Restricted | Named agents | Owning agent + authorized | Team knowledge, role-specific |
| Shared | All agents | Operators + verified pipeline | Ground truth, system config |

#### Provenance tracking

Every shared or published fact must carry provenance:

```
provenance { fact_id: String =>
    contributed_by: String,       -- nous_id of original extractor
    published_by: String,         -- nous_id that published (may differ)
    published_at: String,
    verification_count: Int,      -- how many agents confirmed
    contested_by: String,         -- comma-separated nous_ids
    contest_reason: String
}
```

Provenance enables: attribution (who contributed), trust assessment (how many agents agree), and conflict detection (who disagrees).

#### Conflict resolution

Agents may extract contradictory facts. The system already handles supersession (`superseded_by` field) and correction (`correct_fact` operation) within a single agent. Cross-agent conflicts need additional handling:

**Detection.** The extraction pipeline's contradiction detection (vector similarity + LLM classification) runs against all facts, not just the extracting agent's. When agent B extracts a fact that contradicts agent A's published fact, the system flags a conflict.

**Resolution strategies.**

| Strategy | When | Mechanism |
|----------|------|-----------|
| Confidence wins | Facts differ in confidence | Higher confidence fact prevails |
| Recency wins | Facts have temporal context | More recent fact prevails |
| Tier wins | Facts differ in epistemic tier | Verified > Inferred > Assumed |
| Consensus wins | Multiple agents involved | Majority position prevails |
| Operator resolves | None of the above | Flag for human review |

**Recommendation.** Composite scoring: `resolve_score = 0.4 * confidence + 0.3 * tier_score + 0.2 * recency + 0.1 * supporter_count`. Highest score wins. Ties go to operator review. The losing fact is not deleted; it gets `contested_by` provenance and a reduced confidence.

### The agora pattern

The agora (marketplace) crate already handles inbound message routing from external channels (Signal, Slack) to agents. Could knowledge sharing use this channel?

**Current agora architecture.** `MessageRouter` routes `InboundMessage` values to agents based on channel bindings (group, sender, channel default, global default). Each binding maps to a `(nous_id, session_key)` pair. The router is designed for external-to-agent communication, not agent-to-agent.

**Agent-to-agent via agora.** The `CrossNousRouter` in nous already handles agent-to-agent messaging. Adding knowledge-sharing events to CrossNousRouter is more natural than extending agora's message routing, because:

1. CrossNousRouter already has delivery tracking and audit logging
2. Agora's routing model (channel bindings) doesn't map to knowledge topics
3. CrossNousRouter's `ask` pattern supports request-response for verification queries

**Recommendation.** Use CrossNousRouter for knowledge-sharing events. Define message types:

| Message type | Payload | Response |
|--------------|---------|----------|
| `knowledge:published` | Shared fact ID + summary | None (notification) |
| `knowledge:verify` | Fact content + requester | Accept/Contest/Abstain |
| `knowledge:contest` | Fact ID + reason | None (notification) |
| `knowledge:query` | Query string + filters | Vec of matching facts |

These are structured messages sent through the existing CrossNousRouter infrastructure. No new transport needed.

### Privacy boundaries

**Must never be shared.**

| Category | Reason | Enforcement |
|----------|--------|-------------|
| User secrets (API keys, passwords) | Security breach | `SecretString` type, never persisted as facts |
| Session transcripts | User privacy, context leakage | SQLite scoping, no cross-agent session access |
| Tool parameters with credentials | Security | Instinct system strips secrets before observation |
| User PII above debug level | Compliance | Tracing redaction, fact content filtering |
| Behavioral patterns (instincts) | Agent autonomy | Per-agent aggregation, no sharing API |

**Should be shared cautiously.**

| Category | Risk | Mitigation |
|----------|------|------------|
| User preferences | Cross-user bleed in multi-user setups | Scope by user context, not just agent |
| Error patterns | Sensitive system internals | Generalize before sharing ("tool X fails on input type Y") |
| Conversation summaries | Privacy, context leakage | Share extracted facts, not summaries |

**Safe to share.**

| Category | Benefit |
|----------|---------|
| Entity graph (people, projects, orgs) | Collective understanding |
| Verified factual claims | Ground truth |
| Tool effectiveness patterns (anonymized) | Collective optimization |
| Domain knowledge (non-personal) | Shared expertise |

**Key principle.** Share extracted facts, not raw conversation content. The extraction pipeline is the privacy boundary: it transforms conversation into structured facts, stripping context that could leak private information. Published facts should be reviewed by the publishing agent for sensitivity before sharing.

### Effort estimate and phased approach

#### Phase 1: Publication and visibility (5-8 days)

**Scope.**
- Add `visibility` field to facts schema (default: `private`)
- Add `provenance` relation to CozoDB
- Implement `publish_fact` operation (copy-on-publish)
- Add `knowledge:published` message type to CrossNousRouter
- Expose publication via agent tool and HTTP API

**Changes.**
- `mneme/src/knowledge.rs`: Add `Visibility` enum, `Provenance` struct
- `mneme/src/knowledge_store/`: Schema migration, insert/query with visibility
- `nous/src/cross.rs`: Add knowledge message types
- `organon/`: New `publish_knowledge` tool for agents
- `pylon/`: HTTP endpoint for operator publication

**Risk.** Schema migration on existing CozoDB data. Mitigated by defaulting all existing facts to `private` visibility.

#### Phase 2: Subscription and notification (3-5 days)

**Scope.**
- Implement subscription registry (in-memory, persisted to SQLite)
- Subscription filter matching at publication time
- Push notifications via CrossNousRouter
- Agent tool for managing subscriptions

**Changes.**
- `mneme/src/subscription.rs`: New module for subscription management
- `nous/src/cross.rs`: Subscription-triggered notifications
- `organon/`: `subscribe_knowledge` and `unsubscribe_knowledge` tools

**Risk.** Notification volume with many agents and frequent publications. Mitigated by filter specificity and rate limiting.

#### Phase 3: Verification and conflict resolution (4-6 days)

**Scope.**
- Multi-agent verification protocol (request -> vote -> promote)
- Conflict detection in extraction pipeline (cross-agent)
- Conflict resolution scoring
- `knowledge:verify` and `knowledge:contest` message types

**Changes.**
- `mneme/src/verification.rs`: New module for verification protocol
- `mneme/src/extract/`: Cross-agent contradiction detection
- `nous/src/cross.rs`: Verification message handling
- `mneme/src/knowledge_store/`: Verification count tracking, promotion logic

**Risk.** Consensus deadlock with even number of agents. Mitigated by operator tiebreaker and timeout-based resolution.

#### Phase 4: Federated recall and restricted access (3-4 days)

**Scope.**
- Extend recall engine with visibility-aware filtering
- `restricted` visibility with per-fact access lists
- Recall options for cross-agent queries (include/exclude agents)

**Changes.**
- `mneme/src/recall.rs`: Visibility filtering in scoring
- `mneme/src/knowledge_store/`: `fact_access` relation for restricted facts
- `nous/src/recall.rs`: Recall options propagation

**Risk.** Performance impact of visibility checks on every recall query. Mitigated by indexing `visibility` field and short-circuiting for `private` (most common case).

**Total estimate: 15-23 days across 4 phases.**

## Recommendations

1. **Start with Phase 1 (publication).** The copy-on-publish model is the simplest extension that delivers value. Agents can share findings without new infrastructure beyond a schema addition and a tool.

2. **Use CrossNousRouter for all knowledge events.** Do not extend agora or build a new transport. The router already has delivery tracking and audit logging.

3. **Copy-on-publish over move-on-publish.** Preserving the original agent-scoped fact maintains agent autonomy and allows independent evolution of private vs. shared versions.

4. **Per-fact visibility over per-agent ACLs.** Visibility is a property of knowledge, not of agents. A fact about "project deadlines" might be shared while a fact about "user password preferences" stays private. The owning agent decides per fact.

5. **Share facts, not transcripts.** The extraction pipeline is the privacy boundary. Never expose raw session content to other agents. Published facts are sanitized, structured, and attribution-tagged.

6. **Multi-agent verification as the promotion path.** The strongest shared knowledge comes from independent confirmation. When 3+ agents extract the same fact, auto-promote to shared/verified. This builds collective confidence without central authority.

7. **Defer behavioral pattern sharing.** Instincts (tool usage patterns) are deeply tied to agent identity and user context. Sharing them risks homogenizing agent behavior. Revisit after the fact-sharing infrastructure matures.

## Gotchas

- **CozoDB schema migration.** Adding `visibility` to the `facts` relation requires rebuilding the relation in Datalog (CozoDB does not support ALTER). Existing facts need migration with default `visibility = "private"`. Test migration on a copy of production data first.

- **Recall performance.** Adding visibility filtering to the 6-factor recall engine adds a branch to every scored fact. The `private` case (most common) should short-circuit early. Benchmark recall latency before and after Phase 4.

- **Publication spam.** Without rate limiting, an agent could publish thousands of low-quality facts. Add per-agent publication rate limits (e.g., 100/hour) and minimum confidence thresholds for publication.

- **Cross-user contamination.** In multi-user deployments, agent A serving user X might publish facts that agent B serving user Y can see. The visibility model operates at the agent level, not the user level. Multi-user deployments need an additional `user_scope` dimension on facts, or separate knowledge store instances per user.

- **Verification protocol liveness.** If a verification request requires 3 confirmations but only 2 agents are active, the request hangs. Add a timeout (e.g., 24 hours) after which the fact is promoted with a `partially_verified` status.

- **Entity deduplication.** The entity graph is already shared, so cross-agent entity conflicts (agent A says "Alice" is a person, agent B says "Alice" is a project) can corrupt the graph. The extraction pipeline's entity resolution needs to consider `entity_type` as part of the dedup key, not just name.

- **Existing `nous_id = ""` semantics.** Today, nothing sets `nous_id` to empty in the extraction pipeline. If any code or operator has manually inserted facts with empty `nous_id`, those facts are already "shared" without provenance. A migration should audit existing empty-`nous_id` facts and backfill provenance.

## References

- `mneme/src/knowledge.rs` -- `Fact`, `FactType`, `EpistemicTier` definitions
- `mneme/src/knowledge_store/mod.rs` -- CozoDB schema, `insert_fact`, Datalog relations
- `mneme/src/recall.rs` -- `RecallEngine`, `score_relevance()`, 6-factor weights
- `mneme/src/instinct.rs` -- `ToolObservation`, `BehavioralPattern` (per-agent behavioral memory)
- `mneme/src/extract/mod.rs` -- `Extraction`, `ExtractedFact`, contradiction detection
- `nous/src/cross.rs` -- `CrossNousRouter`, `CrossNousMessage`, delivery tracking
- `nous/src/actor/mod.rs` -- `NousActor` message loop, cross-nous envelope handling
- `nous/src/manager.rs` -- `NousManager`, agent spawn with router registration
- `nous/src/spawn_svc.rs` -- Ephemeral sub-agent spawning (stateless, no knowledge access)
- `agora/src/router.rs` -- `MessageRouter`, channel binding resolution
- `koina/src/id.rs` -- `NousId`, `SessionId` newtype definitions
- R717 (active-forgetting.md) -- FSRS decay model, `forget_fact` mechanism, maintenance tasks
- R714 (causal-reasoning.md) -- Graph model extensions, relationship metadata patterns
