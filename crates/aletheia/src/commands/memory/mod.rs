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

pub(crate) async fn run(action: Action, url: &str, instance_root: Option<&PathBuf>) -> Result<()> {
    // WHY: the knowledge store uses an exclusive fjall lock; opening it while the
    // server holds the lock causes a confusing error. Detect a running server and
    // route 'check' through the HTTP API instead of direct store access.
    if let Ok(true) = is_server_running(url).await {
        match action {
            Action::Check { json } => return run_check_via_api(url, json).await,
            _ => {
                anyhow::bail!(
                    "The server at {url} is running and holds an exclusive lock on the knowledge store.\n  \
                     Stop the server first to use this subcommand, or use the REST API:\n  \
                     GET {url}/api/v1/knowledge/facts\n  \
                     GET {url}/api/v1/knowledge/entities"
                );
            }
        }
    }

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

/// Check if a server is running at `url` by hitting the health endpoint.
async fn is_server_running(url: &str) -> Result<bool> {
    let endpoint = format!("{url}/api/health");
    match reqwest::get(&endpoint).await {
        Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 503),
        Err(_) => Ok(false),
    }
}

/// Run the graph health check via the server's HTTP API.
async fn run_check_via_api(url: &str, json: bool) -> Result<()> {
    let endpoint = format!("{url}/api/v1/knowledge/check");
    let resp = reqwest::get(&endpoint)
        .await
        .map_err(|e| anyhow::anyhow!("failed to connect to {endpoint}: {e}"))?;

    if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        anyhow::bail!("knowledge store is not enabled on the running server");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse response: {e}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("=== Memory Graph Health Check (via server API) ===\n");
        if let Some(fc) = body.get("fact_count").and_then(serde_json::Value::as_u64) {
            println!("Facts:         {fc}");
        }
        if let Some(ec) = body.get("entity_count").and_then(serde_json::Value::as_u64) {
            println!("Entities:      {ec}");
        }
        if let Some(rc) = body
            .get("relationship_count")
            .and_then(serde_json::Value::as_u64)
        {
            println!("Relationships: {rc}");
        }
        let orphaned = body
            .get("orphaned_entity_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let dangling = body
            .get("dangling_edge_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if orphaned > 0 {
            println!("\nOrphaned entities: {orphaned}");
        } else {
            println!("Orphaned entities: 0");
        }
        if dangling > 0 {
            println!("Dangling edges: {dangling}");
        } else {
            println!("Dangling edges: 0");
        }
        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        println!("\nStatus: {status}");
    }

    Ok(())
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
        .and_then(aletheia_mneme::engine::DataValue::get_int)
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
        .map(|r| {
            let src = r.first().and_then(|v| v.get_str()).unwrap_or("?");
            let dst = r.get(1).and_then(|v| v.get_str()).unwrap_or("?");
            let rel = r.get(2).and_then(|v| v.get_str()).unwrap_or("?");
            format!("{src} --[{rel}]--> {dst}")
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
            // WHY: byte length checked above ensures ASCII prefix is safe; UTF-8 boundary handled by get
            let truncated = content.get(..197).unwrap_or(content);
            format!("{truncated}...")
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
            // WHY: len > 1 guarantees first() is Some
            let first_content = entries.first().map_or("", |(_, c)| c.as_str());
            let prefix = if first_content.len() > 60 {
                let truncated = first_content.get(..57).unwrap_or(first_content);
                format!("{truncated}...")
            } else {
                first_content.to_owned()
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
            let cnt = r
                .get(2)
                .and_then(aletheia_mneme::engine::DataValue::get_int)?;
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
            let cnt = r
                .get(1)
                .and_then(aletheia_mneme::engine::DataValue::get_int)?;
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
            let total = r
                .get(1)
                .and_then(aletheia_mneme::engine::DataValue::get_int)?;
            Some((name, total))
        })
        .collect())
}

#[cfg(all(test, feature = "recall"))]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length slices"
)]
mod tests;
