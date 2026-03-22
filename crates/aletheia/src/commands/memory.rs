//! `aletheia memory`: knowledge graph inspection and maintenance.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Validate graph consistency and detect orphans
    Check {
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Trigger a consolidation pass manually
    Consolidate {
        /// Nous agent ID to consolidate
        #[arg(long)]
        nous_id: String,
        /// Preview without mutating
        #[arg(long)]
        dry_run: bool,
    },
    /// Sample random memories for quality review
    Sample {
        /// Number of memories to sample
        #[arg(long, default_value_t = 10)]
        count: u16,
        /// Filter to a specific nous agent
        #[arg(long)]
        nous_id: Option<String>,
    },
    /// Find and merge duplicate entities/facts
    Dedup {
        /// Nous agent ID to deduplicate
        #[arg(long)]
        nous_id: String,
        /// Preview duplicates without merging
        #[arg(long)]
        dry_run: bool,
    },
    /// Extract recurring patterns from the knowledge graph
    Patterns {
        /// Nous agent ID to analyze
        #[arg(long)]
        nous_id: Option<String>,
        /// Maximum patterns to display
        #[arg(long, default_value_t = 20)]
        limit: u16,
    },
}

pub(crate) fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        let oikos = super::resolve_oikos(instance_root)?;
        let knowledge_path = oikos.knowledge_db();
        if !knowledge_path.exists() {
            anyhow::bail!(
                "knowledge store not found at {}\n  \
                 Has this instance been initialized with recall enabled?",
                knowledge_path.display()
            );
        }

        let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
        let store =
            aletheia_mneme::knowledge_store::KnowledgeStore::open_fjall(&knowledge_path, config)
                .map_err(|e| anyhow::anyhow!("failed to open knowledge store: {e}"))?;

        match action {
            Action::Check { json } => run_check(&store, json),
            Action::Consolidate { nous_id, dry_run } => run_consolidate(&store, &nous_id, dry_run),
            Action::Sample { count, nous_id } => run_sample(&store, count, nous_id.as_deref()),
            Action::Dedup { nous_id, dry_run } => run_dedup(&store, &nous_id, dry_run),
            Action::Patterns { nous_id, limit } => run_patterns(&store, nous_id.as_deref(), limit),
        }
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (action, instance_root);
        anyhow::bail!(
            "memory subcommands require the 'recall' feature.\n  \
             Build with: cargo build --features recall"
        );
    }
}

// --- check ---

#[cfg(feature = "recall")]
fn run_check(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    json: bool,
) -> Result<()> {
    let report = build_check_report(store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("=== Memory Graph Sanity Check ===\n");
        println!("Facts:         {}", report.fact_count);
        println!("Entities:      {}", report.entity_count);
        println!("Relationships: {}", report.relationship_count);
        println!("Embeddings:    {}", report.embedding_count);
        println!();

        if report.orphaned_entity_count > 0 {
            println!(
                "Orphaned entities (no relationships): {}",
                report.orphaned_entity_count
            );
            for id in &report.orphaned_entity_ids {
                println!("  {id}");
            }
        } else {
            println!("Orphaned entities: 0");
        }

        if report.dangling_edge_count > 0 {
            println!(
                "\nDangling edges (missing endpoint): {}",
                report.dangling_edge_count
            );
            for edge in &report.dangling_edges {
                println!("  {edge}");
            }
        } else {
            println!("Dangling edges: 0");
        }

        if report.orphaned_embedding_count > 0 {
            println!(
                "\nOrphaned embeddings (no source fact): {}",
                report.orphaned_embedding_count
            );
            for id in &report.orphaned_embedding_ids {
                println!("  {id}");
            }
        } else {
            println!("Orphaned embeddings: 0");
        }

        let healthy = report.orphaned_entity_count == 0
            && report.dangling_edge_count == 0
            && report.orphaned_embedding_count == 0;
        println!(
            "\nStatus: {}",
            if healthy { "healthy" } else { "issues found" }
        );
    }

    Ok(())
}

#[cfg(feature = "recall")]
#[derive(serde::Serialize)]
struct CheckReport {
    fact_count: usize,
    entity_count: usize,
    relationship_count: usize,
    embedding_count: usize,
    orphaned_entity_count: usize,
    orphaned_entity_ids: Vec<String>,
    dangling_edge_count: usize,
    dangling_edges: Vec<String>,
    orphaned_embedding_count: usize,
    orphaned_embedding_ids: Vec<String>,
}

#[cfg(feature = "recall")]
fn build_check_report(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
) -> Result<CheckReport> {
    let fact_count = count_relation(store, "facts")?;
    let entity_count = count_relation(store, "entities")?;
    let relationship_count = count_relation(store, "relationships")?;
    let embedding_count = count_relation(store, "embeddings")?;

    let orphaned = find_orphaned_entities(store)?;
    let dangling = find_dangling_edges(store)?;
    let orphaned_embeddings = find_orphaned_embeddings(store)?;

    Ok(CheckReport {
        fact_count,
        entity_count,
        relationship_count,
        embedding_count,
        orphaned_entity_count: orphaned.len(),
        orphaned_entity_ids: orphaned,
        dangling_edge_count: dangling.len(),
        dangling_edges: dangling,
        orphaned_embedding_count: orphaned_embeddings.len(),
        orphaned_embedding_ids: orphaned_embeddings,
    })
}

#[cfg(feature = "recall")]
fn count_relation(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    relation: &str,
) -> Result<usize> {
    use std::collections::BTreeMap;
    // WHY: each relation has different key fields; count rows via a universal pattern
    let key_field = match relation {
        "relationships" => "src",
        "fact_entities" => "fact_id",
        _ => "id",
    };
    let script = format!("row[{key_field}] := *{relation}{{{key_field}}} \n?[count(k)] := row[k]");
    let result = store
        .run_query(&script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("query failed: {e}"))?;
    let count = result
        .rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.get_int())
        .unwrap_or(0);
    Ok(usize::try_from(count).unwrap_or(0))
}

#[cfg(feature = "recall")]
fn find_orphaned_entities(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<String>> {
    use std::collections::BTreeMap;
    let script = r"
        ?[id] :=
            *entities{id},
            not *relationships{src: id},
            not *relationships{dst: id},
            not *fact_entities{entity_id: id}
    ";
    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("orphan query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| r.first().and_then(|v| v.get_str()).map(String::from))
        .collect())
}

#[cfg(feature = "recall")]
fn find_dangling_edges(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<String>> {
    use std::collections::BTreeMap;
    let script = r"
        ?[src, dst, relation] :=
            *relationships{src, dst, relation},
            not *entities{id: src}

        ?[src, dst, relation] :=
            *relationships{src, dst, relation},
            not *entities{id: dst}
    ";
    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("dangling edge query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let src = r.first().and_then(|v| v.get_str()).unwrap_or("?");
            let dst = r.get(1).and_then(|v| v.get_str()).unwrap_or("?");
            let rel = r.get(2).and_then(|v| v.get_str()).unwrap_or("?");
            Some(format!("{src} --[{rel}]--> {dst}"))
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_orphaned_embeddings(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<String>> {
    use std::collections::BTreeMap;
    let script = r"
        ?[id] :=
            *embeddings{id, source_type, source_id},
            source_type == 'fact',
            not *facts{id: source_id}
    ";
    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("orphaned embedding query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| r.first().and_then(|v| v.get_str()).map(String::from))
        .collect())
}

// --- consolidate ---

#[cfg(feature = "recall")]
fn run_consolidate(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
    dry_run: bool,
) -> Result<()> {
    use aletheia_mneme::consolidation::ConsolidationConfig;

    let config = ConsolidationConfig::default();

    println!("Scanning for consolidation candidates (nous: {nous_id})...");

    let entity_candidates = store
        .find_entity_overflow_candidates(nous_id, &config)
        .map_err(|e| anyhow::anyhow!("entity overflow scan failed: {e}"))?;
    let community_candidates = store
        .find_community_overflow_candidates(nous_id, &config)
        .map_err(|e| anyhow::anyhow!("community overflow scan failed: {e}"))?;

    let total = entity_candidates.len() + community_candidates.len();
    if total == 0 {
        println!("No consolidation candidates found.");
        println!(
            "  Entity threshold: {} facts, Community threshold: {} facts, Age gate: {} days",
            config.entity_fact_threshold, config.community_fact_threshold, config.min_age_days
        );
        return Ok(());
    }

    println!("Found {total} candidates:");
    for c in entity_candidates.iter().chain(community_candidates.iter()) {
        println!("  {} — {} facts", c.trigger.trigger_type(), c.fact_count);
    }

    if dry_run {
        println!("\n--dry-run: no mutations applied.");
    } else {
        println!(
            "\nConsolidation requires an LLM provider. Use the server's maintenance \
             pipeline for automated consolidation, or re-run with --dry-run to preview."
        );
    }

    Ok(())
}

// --- sample ---

#[cfg(feature = "recall")]
fn run_sample(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    count: u16,
    nous_id: Option<&str>,
) -> Result<()> {
    let limit = i64::from(count).saturating_mul(5).max(100);
    let facts = match nous_id {
        Some(id) => {
            let now = aletheia_mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
            store
                .query_facts(id, &now, limit)
                .map_err(|e| anyhow::anyhow!("query failed: {e}"))?
        }
        None => store
            .list_all_facts(limit)
            .map_err(|e| anyhow::anyhow!("query failed: {e}"))?,
    };

    if facts.is_empty() {
        println!("No facts found.");
        return Ok(());
    }

    let selected = sample_random(&facts, usize::from(count));

    println!(
        "=== Memory Sample ({} of {} total) ===\n",
        selected.len(),
        facts.len()
    );
    for (i, fact) in selected.iter().enumerate() {
        println!("--- [{}/{}] {} ---", i + 1, selected.len(), fact.id);
        println!("  Type:       {}", fact.fact_type);
        println!("  Confidence: {:.2}", fact.provenance.confidence);
        println!("  Tier:       {}", fact.provenance.tier);
        println!("  Recorded:   {}", fact.temporal.recorded_at);
        println!("  Accesses:   {}", fact.access.access_count);
        if let Some(ref session) = fact.provenance.source_session_id {
            println!("  Session:    {session}");
        }
        let content = &fact.content;
        let display = if content.len() > 200 {
            format!("{}...", &content[..197])
        } else {
            content.clone()
        };
        println!("  Content:    {display}");
        println!();
    }

    Ok(())
}

/// Select `count` random elements from `items` using Fisher-Yates on indices.
#[cfg(feature = "recall")]
fn sample_random<T: Clone>(items: &[T], count: usize) -> Vec<T> {
    use rand::prelude::IndexedRandom;
    let mut rng = rand::rng();
    if count >= items.len() {
        return items.to_vec();
    }
    items.choose_multiple(&mut rng, count).cloned().collect()
}

// --- dedup ---

#[cfg(feature = "recall")]
fn run_dedup(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
    dry_run: bool,
) -> Result<()> {
    println!("Scanning for duplicate entities (nous: {nous_id})...");

    let candidates = store
        .find_duplicate_entities(nous_id)
        .map_err(|e| anyhow::anyhow!("duplicate scan failed: {e}"))?;

    let pending = store
        .get_pending_merges(nous_id)
        .map_err(|e| anyhow::anyhow!("pending merge query failed: {e}"))?;

    let hash_dupes = find_content_hash_duplicates(store, nous_id)?;

    if candidates.is_empty() && pending.is_empty() && hash_dupes.is_empty() {
        println!("No duplicates found.");
        return Ok(());
    }

    if !candidates.is_empty() {
        println!("\nEntity merge candidates ({}):", candidates.len());
        for c in &candidates {
            println!(
                "  {} <-> {} (score: {:.2}, name_sim: {:.2}, type_match: {})",
                c.name_a, c.name_b, c.merge_score, c.name_similarity, c.type_match
            );
        }
    }

    if !pending.is_empty() {
        println!("\nPending merges awaiting review ({}):", pending.len());
        for p in &pending {
            println!(
                "  {} <-> {} (score: {:.2})",
                p.name_a, p.name_b, p.merge_score
            );
        }
    }

    if !hash_dupes.is_empty() {
        println!("\nExact content duplicates ({}):", hash_dupes.len());
        for (content_prefix, ids) in &hash_dupes {
            println!(
                "  \"{content_prefix}\" — {} copies: {}",
                ids.len(),
                ids.join(", ")
            );
        }
    }

    if dry_run {
        println!("\n--dry-run: no mutations applied.");
    } else {
        let records = store
            .run_entity_dedup(nous_id)
            .map_err(|e| anyhow::anyhow!("dedup execution failed: {e}"))?;
        if records.is_empty() {
            println!("\nNo auto-merges met the threshold. Candidates stored for review.");
        } else {
            println!("\nExecuted {} merges:", records.len());
            for r in &records {
                println!(
                    "  {} absorbed {} ({} facts, {} edges)",
                    r.canonical_entity_id,
                    r.merged_entity_name,
                    r.facts_transferred,
                    r.relationships_redirected
                );
            }
        }
    }

    Ok(())
}

/// Find facts with identical content (by SHA-256 hash).
#[cfg(feature = "recall")]
fn find_content_hash_duplicates(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    use std::collections::HashMap;

    use sha2::{Digest, Sha256};

    let now = aletheia_mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
    let facts = store
        .query_facts(nous_id, &now, 10_000)
        .map_err(|e| anyhow::anyhow!("fact query failed: {e}"))?;

    let mut hash_map: HashMap<[u8; 32], Vec<(String, String)>> = HashMap::new();
    for fact in &facts {
        let hash: [u8; 32] = Sha256::digest(fact.content.as_bytes()).into();
        hash_map
            .entry(hash)
            .or_default()
            .push((fact.id.to_string(), fact.content.clone()));
    }

    let mut dupes = Vec::new();
    for (_hash, entries) in hash_map {
        if entries.len() > 1 {
            let prefix = if entries[0].1.len() > 60 {
                format!("{}...", &entries[0].1[..57])
            } else {
                entries[0].1.clone()
            };
            let ids: Vec<String> = entries.into_iter().map(|(id, _)| id).collect();
            dupes.push((prefix, ids));
        }
    }
    dupes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    Ok(dupes)
}

// --- patterns ---

#[cfg(feature = "recall")]
fn run_patterns(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    _nous_id: Option<&str>,
    limit: u16,
) -> Result<()> {
    println!("=== Knowledge Graph Patterns ===\n");

    let cooccurrence = find_entity_cooccurrence(store, i64::from(limit))?;
    if cooccurrence.is_empty() {
        println!("Entity co-occurrence: no patterns found");
    } else {
        println!("Entity co-occurrence (shared facts):");
        for (entity_a, entity_b, count) in &cooccurrence {
            println!("  {entity_a} <-> {entity_b}: {count} shared facts");
        }
    }

    println!();

    let chains = find_relationship_chains(store, i64::from(limit))?;
    if chains.is_empty() {
        println!("Relationship chains: no patterns found");
    } else {
        println!("Common relationship types:");
        for (relation, count) in &chains {
            println!("  {relation}: {count} edges");
        }
    }

    println!();

    let hubs = find_hub_entities(store, i64::from(limit))?;
    if hubs.is_empty() {
        println!("Hub entities: no patterns found");
    } else {
        println!("Hub entities (most connected):");
        for (name, degree) in &hubs {
            println!("  {name}: {degree} connections");
        }
    }

    Ok(())
}

#[cfg(feature = "recall")]
fn find_entity_cooccurrence(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    limit: i64,
) -> Result<Vec<(String, String, i64)>> {
    use std::collections::BTreeMap;
    let script = format!(
        r"
        cooccur[ea, eb, count(fid)] :=
            *fact_entities{{fact_id: fid, entity_id: ea}},
            *fact_entities{{fact_id: fid, entity_id: eb}},
            ea < eb

        ?[name_a, name_b, cnt] :=
            cooccur[ea, eb, cnt],
            *entities{{id: ea, name: name_a}},
            *entities{{id: eb, name: name_b}},
            cnt >= 2

        :sort -cnt
        :limit {limit}
        "
    );
    let result = store
        .run_query(&script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("co-occurrence query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let a = r.first().and_then(|v| v.get_str())?.to_owned();
            let b = r.get(1).and_then(|v| v.get_str())?.to_owned();
            let cnt = r.get(2).and_then(|v| v.get_int())?;
            Some((a, b, cnt))
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_relationship_chains(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    limit: i64,
) -> Result<Vec<(String, i64)>> {
    use std::collections::BTreeMap;
    let script = format!(
        r"
        ?[relation, count(src)] :=
            *relationships{{src, relation}}

        :sort -count(src)
        :limit {limit}
        "
    );
    let result = store
        .run_query(&script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("relationship chain query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let rel = r.first().and_then(|v| v.get_str())?.to_owned();
            let cnt = r.get(1).and_then(|v| v.get_int())?;
            Some((rel, cnt))
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_hub_entities(
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    limit: i64,
) -> Result<Vec<(String, i64)>> {
    use std::collections::BTreeMap;
    let script = format!(
        r"
        degree[eid, count(other)] :=
            *relationships{{src: eid, dst: other}}
        degree[eid, count(other)] :=
            *relationships{{src: other, dst: eid}}

        ?[name, total] :=
            degree[eid, cnt],
            total = cnt,
            *entities{{id: eid, name}}

        :sort -total
        :limit {limit}
        "
    );
    let result = store
        .run_query(&script, BTreeMap::new())
        .map_err(|e| anyhow::anyhow!("hub entity query failed: {e}"))?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| {
            let name = r.first().and_then(|v| v.get_str())?.to_owned();
            let total = r.get(1).and_then(|v| v.get_int())?;
            Some((name, total))
        })
        .collect())
}

#[cfg(all(test, feature = "recall"))]
mod tests {
    use std::sync::Arc;

    use aletheia_mneme::id::{EntityId, FactId};
    use aletheia_mneme::knowledge::{
        Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
        far_future,
    };
    use aletheia_mneme::knowledge_store::KnowledgeStore;

    fn test_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem().expect("failed to open in-memory store")
    }

    fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
        let now = jiff::Timestamp::now();
        Fact {
            id: FactId::new(id).expect("valid id"),
            nous_id: nous_id.to_owned(),
            fact_type: "observation".to_owned(),
            content: content.to_owned(),
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: 0.8,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: 168.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
        let now = jiff::Timestamp::now();
        Entity {
            id: EntityId::new(id).expect("valid id"),
            name: name.to_owned(),
            entity_type: entity_type.to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn check_reports_empty_store_as_healthy() {
        let store = test_store();
        let report = super::build_check_report(&store).expect("check should succeed");
        assert_eq!(report.fact_count, 0, "empty store has no facts");
        assert_eq!(report.entity_count, 0, "empty store has no entities");
        assert_eq!(report.orphaned_entity_count, 0, "no orphans in empty store");
        assert_eq!(
            report.dangling_edge_count, 0,
            "no dangling edges in empty store"
        );
    }

    #[test]
    fn check_detects_orphaned_entity() {
        let store = test_store();
        let entity = make_entity("ent-001", "Alice", "person");
        store
            .insert_entity(&entity)
            .expect("insert entity should succeed");

        let report = super::build_check_report(&store).expect("check should succeed");
        assert_eq!(report.entity_count, 1, "one entity inserted");
        assert_eq!(
            report.orphaned_entity_count, 1,
            "entity with no relationships is orphaned"
        );
        assert_eq!(report.orphaned_entity_ids, vec!["ent-001"]);
    }

    #[test]
    fn check_entity_with_relationship_not_orphaned() {
        let store = test_store();
        let e1 = make_entity("ent-a", "Alice", "person");
        let e2 = make_entity("ent-b", "Bob", "person");
        store.insert_entity(&e1).expect("insert e1");
        store.insert_entity(&e2).expect("insert e2");

        let rel = aletheia_mneme::knowledge::Relationship {
            src: EntityId::new("ent-a").expect("valid id"),
            dst: EntityId::new("ent-b").expect("valid id"),
            relation: "knows".to_owned(),
            weight: 1.0,
            created_at: jiff::Timestamp::now(),
        };
        store
            .insert_relationship(&rel)
            .expect("insert relationship");

        let report = super::build_check_report(&store).expect("check should succeed");
        assert_eq!(report.entity_count, 2, "two entities");
        assert_eq!(
            report.orphaned_entity_count, 0,
            "entities with relationships are not orphaned"
        );
    }

    #[test]
    fn dedup_finds_content_hash_duplicates() {
        let store = test_store();
        let f1 = make_fact("fact-001", "nous-1", "Alice likes Rust");
        let f2 = make_fact("fact-002", "nous-1", "Alice likes Rust");
        let f3 = make_fact("fact-003", "nous-1", "Bob prefers Go");
        store.insert_fact(&f1).expect("insert f1");
        store.insert_fact(&f2).expect("insert f2");
        store.insert_fact(&f3).expect("insert f3");

        let dupes = super::find_content_hash_duplicates(&store, "nous-1")
            .expect("dedup scan should succeed");
        assert_eq!(dupes.len(), 1, "one set of duplicates");
        assert_eq!(dupes[0].1.len(), 2, "two copies of the duplicate");
    }

    #[test]
    fn dedup_no_false_positives_on_unique_content() {
        let store = test_store();
        let f1 = make_fact("fact-a", "nous-1", "fact one");
        let f2 = make_fact("fact-b", "nous-1", "fact two");
        store.insert_fact(&f1).expect("insert f1");
        store.insert_fact(&f2).expect("insert f2");

        let dupes = super::find_content_hash_duplicates(&store, "nous-1")
            .expect("dedup scan should succeed");
        assert!(dupes.is_empty(), "unique facts produce no duplicates");
    }

    #[test]
    fn sample_returns_requested_count() {
        let items: Vec<i32> = (0..100).collect();
        let sampled = super::sample_random(&items, 10);
        assert_eq!(sampled.len(), 10, "sample returns exactly requested count");
    }

    #[test]
    fn sample_clamps_to_available() {
        let items: Vec<i32> = (0..5).collect();
        let sampled = super::sample_random(&items, 20);
        assert_eq!(sampled.len(), 5, "sample clamps to available items");
    }

    #[test]
    fn count_relation_returns_zero_for_empty() {
        let store = test_store();
        let count =
            super::count_relation(&store, "facts").expect("count should succeed on empty store");
        assert_eq!(count, 0, "empty relation has zero rows");
    }

    #[test]
    fn count_relation_after_insert() {
        let store = test_store();
        store
            .insert_fact(&make_fact("f1", "n1", "content"))
            .expect("insert");
        let count = super::count_relation(&store, "facts").expect("count should succeed");
        assert!(count >= 1, "at least one fact after insert");
    }
}
