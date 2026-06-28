// kanon:ignore RUST/file-too-long — module contains tightly-coupled memory subcommand implementations; splitting would hurt cohesion
//! `aletheia memory`: knowledge graph inspection and maintenance.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use snafu::prelude::*;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Validate graph consistency and detect orphans
    Check {
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
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
    /// Find and merge duplicate entities (fact-merge not implemented; see #4165)
    Dedup {
        /// Nous agent ID to deduplicate
        #[arg(long)]
        nous_id: String,
        /// Preview duplicates without merging
        #[arg(long)]
        dry_run: bool,
    },
    /// List entity merge candidates that are queued for review.
    ///
    /// Drains the `pending_merges` queue populated by `memory dedup` —
    /// scores in `[0.70, 0.90)` that did not auto-merge (#4165 Path A).
    DedupPending {
        /// Nous agent ID whose review queue to list
        #[arg(long)]
        nous_id: String,
    },
    /// Approve a queued entity merge and execute it.
    ///
    /// Resolves a review-tier candidate by merging `merged_id` into
    /// `canonical_id` — edges are redirected, `fact_entities` are
    /// transferred, the merged name is preserved as an alias, and the
    /// pending-merge row is cleared. The operator picks which side
    /// survives by argument order (#4165 Path A).
    DedupApprove {
        /// Nous agent ID owning the pending merge
        #[arg(long)]
        nous_id: String,
        /// Entity ID that should survive the merge
        #[arg(long)]
        canonical_id: String,
        /// Entity ID that should be absorbed into the canonical entity
        #[arg(long)]
        merged_id: String,
    },
    /// Reject a queued entity merge and remove it from the review queue.
    DedupReject {
        /// Nous agent ID owning the pending merge
        #[arg(long)]
        nous_id: String,
        /// First entity in the pending pair (`entity_a`)
        #[arg(long)]
        entity_a: String,
        /// Second entity in the pending pair (`entity_b`)
        #[arg(long)]
        entity_b: String,
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
    /// Export the entity + relationship graph for visualization
    ExportGraph {
        /// Output format
        #[arg(long, value_enum, default_value = "dot")]
        format: ExportFormat,
        /// Filter to a specific nous agent's view
        #[arg(long)]
        nous_id: Option<String>,
        /// Filter to a specific memory scope
        #[arg(long)]
        scope: Option<mneme::knowledge::MemoryScope>,
        /// Output file path
        output_path: PathBuf,
    },
    /// Recompute fact embeddings for every store on disk.
    Reembed,
    /// Remove orphaned entities with no relationships and no fact links.
    Gc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ExportFormat {
    /// Graphviz DOT format
    Dot,
    /// `GraphML` XML format
    Graphml,
    /// JSON format
    Json,
}

pub(crate) async fn run(
    action: Action,
    url: &str,
    token: Option<&str>,
    instance_root: Option<&PathBuf>,
) -> Result<()> {
    validate_action(&action)?;

    // WHY: the knowledge store uses an exclusive fjall lock; opening it while the
    // server holds the lock causes a confusing error. Detect a running server and
    // route 'check' through the HTTP API instead of direct store access.
    if is_server_running(url).await? {
        match action {
            Action::Check { json } => return run_check_via_api(url, json, token).await,
            _ => {
                whatever!(
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
        let knowledge_path = oikos.knowledge_cohort_db("shared");
        if !knowledge_path.exists() && !oikos.knowledge_db().exists() {
            whatever!(
                "knowledge store not initialized at {}\n  \
                 The store is created lazily by the running server. Either:\n    \
                   1. Start the server once to bootstrap it:  aletheia\n    \
                   2. Or route this command through a running server with --url",
                knowledge_path.display()
            );
        }

        let loaded_config = taxis::loader::load_config(&oikos);
        let dedup_tuning = loaded_config
            .as_ref()
            .ok()
            .map_or(mneme::dedup::DedupTuning::DEFAULT, |cfg| {
                crate::knowledge_maintenance::tuning_from_behavior(&cfg.agents.defaults.behavior)
            });
        let recovery_config = knowledge_config_from_loaded(&loaded_config, false);

        match action {
            // WHY: reembed/gc iterate every cohort store themselves, so they
            // must not pre-open the shared store like the other actions.
            Action::Reembed => run_reembed(&oikos, &loaded_config),
            Action::Gc => run_gc(&oikos, &recovery_config),
            store_action => {
                let store = open_recovery_store(&knowledge_path, &recovery_config)?;
                run_store_action(&store, store_action, &dedup_tuning)
            }
        }
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (action, instance_root);
        whatever!(
            "memory subcommands require the 'recall' feature.\n  \
             Build with: cargo build --features recall"
        );
    }
}

/// Fail-fast on argument shapes that would silently produce empty or misleading
/// output: zero counts and empty/whitespace nous-ids. Same family as the
/// validators in `benchmark.rs`, `eval.rs`, `prompt_audit.rs`, and
/// `session_export.rs` (refs #4255 / #4259 / #4263 / #4265).
fn validate_action(action: &Action) -> Result<()> {
    match action {
        Action::Check { .. } | Action::Reembed | Action::Gc => Ok(()),
        Action::Sample { count, nous_id } => {
            if *count == 0 {
                whatever!("--count must be greater than 0");
            }
            if let Some(id) = nous_id
                && id.trim().is_empty()
            {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            Ok(())
        }
        Action::Dedup { nous_id, .. } | Action::DedupPending { nous_id } => {
            if nous_id.trim().is_empty() {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            Ok(())
        }
        Action::Patterns { nous_id, limit } => {
            if *limit == 0 {
                whatever!("--limit must be greater than 0");
            }
            if let Some(id) = nous_id
                && id.trim().is_empty()
            {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            Ok(())
        }
        Action::ExportGraph { nous_id, .. } => {
            if let Some(id) = nous_id
                && id.trim().is_empty()
            {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            Ok(())
        }
        Action::DedupApprove {
            nous_id,
            canonical_id,
            merged_id,
        } => {
            if nous_id.trim().is_empty() {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            if canonical_id.trim().is_empty() {
                whatever!("--canonical-id cannot be empty or whitespace");
            }
            if merged_id.trim().is_empty() {
                whatever!("--merged-id cannot be empty or whitespace");
            }
            if canonical_id.trim() == merged_id.trim() {
                whatever!("--canonical-id and --merged-id must differ");
            }
            Ok(())
        }
        Action::DedupReject {
            nous_id,
            entity_a,
            entity_b,
        } => {
            if nous_id.trim().is_empty() {
                whatever!("--nous-id cannot be empty or whitespace");
            }
            if entity_a.trim().is_empty() {
                whatever!("--entity-a cannot be empty or whitespace");
            }
            if entity_b.trim().is_empty() {
                whatever!("--entity-b cannot be empty or whitespace");
            }
            if entity_a.trim() == entity_b.trim() {
                whatever!("--entity-a and --entity-b must differ");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod validation_tests {
    use std::path::PathBuf;

    use super::{Action, ExportFormat, validate_action};

    #[test]
    fn check_always_passes() {
        validate_action(&Action::Check { json: false }).unwrap();
        validate_action(&Action::Check { json: true }).unwrap();
    }

    #[test]
    fn sample_rejects_zero_count() {
        let err = validate_action(&Action::Sample {
            count: 0,
            nous_id: None,
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("--count"),
            "error mentions --count: {err}"
        );
    }

    #[test]
    fn sample_rejects_empty_nous_id() {
        for empty in ["", "   ", "\t", "\n"] {
            let err = validate_action(&Action::Sample {
                count: 10,
                nous_id: Some(empty.to_owned()),
            })
            .unwrap_err();
            assert!(
                err.to_string().contains("--nous-id"),
                "error mentions --nous-id for {empty:?}: {err}"
            );
        }
    }

    #[test]
    fn sample_accepts_well_formed_args() {
        validate_action(&Action::Sample {
            count: 10,
            nous_id: None,
        })
        .unwrap();
        validate_action(&Action::Sample {
            count: 1,
            nous_id: Some("alice".to_owned()),
        })
        .unwrap();
    }

    #[test]
    fn dedup_rejects_empty_nous_id() {
        for empty in ["", "  "] {
            let err = validate_action(&Action::Dedup {
                nous_id: empty.to_owned(),
                dry_run: false,
            })
            .unwrap_err();
            assert!(
                err.to_string().contains("--nous-id"),
                "error mentions --nous-id for {empty:?}: {err}"
            );
        }
    }

    #[test]
    fn dedup_accepts_well_formed_args() {
        validate_action(&Action::Dedup {
            nous_id: "alice".to_owned(),
            dry_run: true,
        })
        .unwrap();
    }

    #[test]
    fn patterns_rejects_zero_limit() {
        let err = validate_action(&Action::Patterns {
            nous_id: None,
            limit: 0,
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("--limit"),
            "error mentions --limit: {err}"
        );
    }

    #[test]
    fn patterns_rejects_empty_nous_id() {
        let err = validate_action(&Action::Patterns {
            nous_id: Some(" ".to_owned()),
            limit: 20,
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("--nous-id"),
            "error mentions --nous-id: {err}"
        );
    }

    #[test]
    fn export_graph_rejects_empty_nous_id() {
        let err = validate_action(&Action::ExportGraph {
            format: ExportFormat::Dot,
            nous_id: Some(String::new()),
            scope: None,
            output_path: PathBuf::from("/tmp/out.dot"),
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("--nous-id"),
            "error mentions --nous-id: {err}"
        );
    }

    #[test]
    fn export_graph_accepts_well_formed_args() {
        validate_action(&Action::ExportGraph {
            format: ExportFormat::Json,
            nous_id: None,
            scope: None,
            output_path: PathBuf::from("/tmp/out.json"),
        })
        .unwrap();
        validate_action(&Action::ExportGraph {
            format: ExportFormat::Graphml,
            nous_id: Some("alice".to_owned()),
            scope: None,
            output_path: PathBuf::from("/tmp/out.graphml"),
        })
        .unwrap();
    }

    #[tokio::test]
    async fn is_server_running_rejects_empty_url() {
        let err = super::is_server_running("").await.unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn is_server_running_rejects_malformed_url() {
        let err = super::is_server_running("not-a-url").await.unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn is_server_running_returns_false_for_unreachable_well_formed_url() {
        let res = super::is_server_running("http://127.0.0.1:1")
            .await
            .unwrap();
        assert!(!res, "expected false when no listener; got {res}");
    }
}

/// Check if a server is running at `url` by hitting the health endpoint.
///
/// Rejects malformed URLs up-front so a parse failure does not silently coerce
/// to "server not running" and let the caller fall through to direct knowledge
/// store access with garbage in `--url`.
async fn is_server_running(url: &str) -> Result<bool> {
    if let Err(e) = reqwest::Url::parse(url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", url);
    }
    let endpoint = format!("{url}/api/health");
    match reqwest::get(&endpoint).await {
        Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 503),
        Err(_) => Ok(false),
    }
}

/// Run the graph health check via the server's HTTP API.
async fn run_check_via_api(url: &str, json: bool, token: Option<&str>) -> Result<()> {
    let endpoint = format!("{url}/api/v1/knowledge/check");
    let mut request = reqwest::Client::new().get(&endpoint);
    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {t}"));
    }
    let resp = request
        .send()
        .await
        .whatever_context("failed to connect to server")?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        whatever!("authentication failed: API token required or invalid");
    }

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        whatever!("authorization failed: token lacks required permissions");
    }

    if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        whatever!("knowledge store is not enabled on the running server");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .whatever_context("failed to parse response")?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).whatever_context("failed to serialize JSON")?
        );
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

// --- recovery ---

/// Dispatch the store-backed memory actions against an opened shared store.
#[cfg(feature = "recall")]
fn run_store_action(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    action: Action,
    dedup_tuning: &mneme::dedup::DedupTuning,
) -> Result<()> {
    match action {
        Action::Check { json } => run_check(store, json),
        Action::Sample { count, nous_id } => run_sample(store, count, nous_id.as_deref()),
        Action::Dedup { nous_id, dry_run } => run_dedup(store, &nous_id, dry_run, dedup_tuning),
        Action::Patterns { nous_id, limit } => run_patterns(store, nous_id.as_deref(), limit),
        Action::ExportGraph {
            format,
            nous_id,
            scope,
            output_path,
        } => run_export_graph(store, format, nous_id.as_deref(), scope, &output_path),
        Action::DedupPending { nous_id } => run_dedup_pending(store, &nous_id),
        Action::DedupApprove {
            nous_id,
            canonical_id,
            merged_id,
        } => run_dedup_approve(store, &nous_id, &canonical_id, &merged_id),
        Action::DedupReject {
            nous_id,
            entity_a,
            entity_b,
        } => run_dedup_reject(store, &nous_id, &entity_a, &entity_b),
        Action::Reembed | Action::Gc => {
            whatever!("BUG: reembed/gc must dispatch before the shared store opens")
        }
    }
}

#[cfg(feature = "recall")]
#[derive(Debug, Clone)]
struct RecoveryKnowledgeConfig {
    dim: usize,
    embedding_model: String,
    allow_assumed_embedding_meta: bool,
}

#[cfg(feature = "recall")]
fn open_recovery_store(
    path: &Path,
    config: &RecoveryKnowledgeConfig,
) -> Result<std::sync::Arc<mneme::knowledge_store::KnowledgeStore>> {
    let config = mneme::knowledge_store::KnowledgeConfig {
        dim: config.dim,
        embedding_model: config.embedding_model.clone(),
        allow_assumed_embedding_meta: config.allow_assumed_embedding_meta,
        ..Default::default()
    };
    // NOTE: lock contention surfaces from the typed
    // `krites::storage::StorageError::Locked` with operator guidance baked
    // into its display; no string inspection is needed here.
    mneme::knowledge_store::KnowledgeStore::open_fjall(path, config).map_err(|err| {
        crate::error::Error::msg(format!(
            "failed to open knowledge store at {}: {err}",
            path.display()
        ))
    })
}

#[cfg(feature = "recall")]
fn knowledge_config_from_loaded(
    loaded: &std::result::Result<taxis::config::AletheiaConfig, taxis::error::Error>,
    allow_assumed_embedding_meta: bool,
) -> RecoveryKnowledgeConfig {
    let Some(config) = loaded.as_ref().ok() else {
        let default = mneme::knowledge_store::KnowledgeConfig::default();
        return RecoveryKnowledgeConfig {
            dim: default.dim,
            embedding_model: default.embedding_model,
            allow_assumed_embedding_meta,
        };
    };
    let embedding_config = config.embedding.to_embedding_config();
    RecoveryKnowledgeConfig {
        dim: config.embedding.dimension,
        embedding_model: embedding_config.effective_model_name(),
        allow_assumed_embedding_meta,
    }
}

#[cfg(feature = "recall")]
fn recovery_store_paths(oikos: &taxis::oikos::Oikos) -> Result<Vec<PathBuf>> {
    let root = oikos.knowledge_db();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut stores = Vec::new();
    for entry in
        std::fs::read_dir(&root).whatever_context("failed to enumerate knowledge store cohorts")?
    {
        let entry = entry.whatever_context("failed to inspect knowledge store cohort")?;
        let path = entry.path();
        if path.is_dir() {
            stores.push(path);
        }
    }
    stores.sort();
    Ok(stores)
}

#[cfg(feature = "recall")]
fn run_reembed(
    oikos: &taxis::oikos::Oikos,
    loaded_config: &std::result::Result<taxis::config::AletheiaConfig, taxis::error::Error>,
) -> Result<()> {
    let config = match loaded_config {
        Ok(cfg) => cfg,
        Err(err) => {
            whatever!("failed to load instance config for memory reembed: {err}");
        }
    };
    let embedding_config = crate::embedding_config::runtime_embedding_config(&config.embedding)
        .with_whatever_context(|error| format!("invalid embedding config: {error}"))?;
    let provider = mneme::embedding::create_provider(&embedding_config)
        .whatever_context("failed to create configured embedding provider")?;

    let stores = recovery_store_paths(oikos)?;
    if stores.is_empty() {
        whatever!(
            "no knowledge stores found under {}\n  \
             Start the server or run a migration/import first.",
            oikos.knowledge_db().display()
        );
    }

    let store_count = stores.len();
    let mut total = 0usize;
    let recovery_config = knowledge_config_from_loaded(loaded_config, true);
    for store_path in stores {
        let store = open_recovery_store(&store_path, &recovery_config)?;
        let written = store
            .reembed_all(provider.as_ref())
            .with_whatever_context(|err| {
                format!(
                    "failed to re-embed facts in {}: {err}",
                    store_path.display()
                )
            })?;
        println!("reembed: {} -> {} facts", store_path.display(), written);
        total = total.saturating_add(written);
    }

    println!("Re-embedded {total} facts across {store_count} store(s).");
    Ok(())
}

#[cfg(feature = "recall")]
fn run_gc(oikos: &taxis::oikos::Oikos, config: &RecoveryKnowledgeConfig) -> Result<()> {
    let stores = recovery_store_paths(oikos)?;
    if stores.is_empty() {
        whatever!(
            "no knowledge stores found under {}\n  \
             Start the server or run a migration/import first.",
            oikos.knowledge_db().display()
        );
    }

    let mut total = 0usize;
    for store_path in &stores {
        let store = open_recovery_store(store_path, config)?;
        let removed = store
            .remove_orphaned_entities()
            .with_whatever_context(|err| {
                format!(
                    "failed to remove orphaned entities in {}: {err}",
                    store_path.display()
                )
            })?;
        println!(
            "gc: {} -> {} orphaned entities removed",
            store_path.display(),
            removed
        );
        total = total.saturating_add(removed);
    }

    println!(
        "Removed {total} orphaned entities across {} store(s).",
        stores.len()
    );
    Ok(())
}

// --- check ---

#[cfg(feature = "recall")]
fn run_check(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    json: bool,
) -> Result<()> {
    let report = build_check_report(store)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).whatever_context("failed to serialize report")?
        );
    } else {
        println!("=== Memory Graph Validation Check ===\n");
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
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
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
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
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
        .whatever_context("query failed")?;
    let col = result.headers.first().map_or("count(k)", String::as_str);
    let count = result.get_i64(0, col).unwrap_or(0);
    Ok(usize::try_from(count).unwrap_or(0))
}

#[cfg(feature = "recall")]
fn find_orphaned_entities(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<String>> {
    store
        .orphaned_entity_ids()
        .whatever_context("orphan query failed")
}

#[cfg(feature = "recall")]
fn find_dangling_edges(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
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
        .whatever_context("dangling edge query failed")?;
    Ok((0..result.row_count())
        .map(|i| {
            let src = result
                .get_string(i, "src")
                .unwrap_or_else(|| "?".to_owned());
            let dst = result
                .get_string(i, "dst")
                .unwrap_or_else(|| "?".to_owned());
            let rel = result
                .get_string(i, "relation")
                .unwrap_or_else(|| "?".to_owned());
            format!("{src} --[{rel}]--> {dst}")
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_orphaned_embeddings(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
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
        .whatever_context("orphaned embedding query failed")?;
    Ok((0..result.row_count())
        .filter_map(|i| result.get_string(i, "id"))
        .collect())
}

// --- sample ---

#[cfg(feature = "recall")]
fn run_sample(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    count: u16,
    nous_id: Option<&str>,
) -> Result<()> {
    let limit = i64::from(count).saturating_mul(5).max(100);
    let facts = match nous_id {
        Some(id) => {
            let now = mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
            store
                .query_facts(id, &now, limit)
                .whatever_context("query failed")?
        }
        None => store
            .list_all_facts(limit)
            .whatever_context("query failed")?,
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
    items.sample(&mut rng, count).cloned().collect()
}

// --- dedup ---

#[cfg(feature = "recall")]
fn run_dedup(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
    dry_run: bool,
    tuning: &mneme::dedup::DedupTuning,
) -> Result<()> {
    println!("Scanning for duplicate entities (nous: {nous_id})...");

    let candidates = store
        .find_duplicate_entities_with_tuning(nous_id, tuning)
        .whatever_context("duplicate scan failed")?;

    let pending = store
        .get_pending_merges(nous_id)
        .whatever_context("pending merge query failed")?;

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
            .run_entity_dedup_with_tuning(nous_id, tuning)
            .whatever_context("dedup execution failed")?;
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

/// List entity merge candidates queued for review.
///
/// Drains [`pending_merges`] populated by `memory dedup` runs — composite
/// scores in `[0.70, 0.90)` that did not auto-merge. Without this listing
/// step the queue was a write-only "roach motel" (#4165 B), since
/// `approve_merge` had no callers anywhere in the codebase.
#[cfg(feature = "recall")]
fn run_dedup_pending(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
) -> Result<()> {
    let pending = store
        .get_pending_merges(nous_id)
        .whatever_context("pending merge query failed")?;

    if pending.is_empty() {
        println!("No pending entity merges for nous {nous_id}.");
        return Ok(());
    }

    println!(
        "Pending entity merges for nous {nous_id} ({}):",
        pending.len()
    );
    for p in &pending {
        println!(
            "  {} <-> {} (score: {:.2}, name_sim: {:.2}, embed_sim: {:.2}, type_match: {}, alias_overlap: {})",
            p.entity_a,
            p.entity_b,
            p.merge_score,
            p.name_similarity,
            p.embed_similarity,
            p.type_match,
            p.alias_overlap,
        );
        println!("    names: {:?} <-> {:?}", p.name_a, p.name_b);
        println!(
            "    approve: aletheia memory dedup-approve --nous-id {nous_id} \\
                     --canonical-id <choose> --merged-id <other>"
        );
    }

    Ok(())
}

/// Approve a queued entity merge.
///
/// Executes [`approve_merge`](mneme::knowledge_store::KnowledgeStore::approve_merge)
/// — redirects relationships, transfers `fact_entities`, preserves the merged
/// name as an alias, deletes the merged entity, and removes the pending
/// row. Operator chooses which side survives via argument order. The
/// merge is *not* gated by the auto-merge threshold: this is the explicit
/// approval path for review-tier candidates the auto-merge math rejects
/// by design (#4165 Path A).
#[cfg(feature = "recall")]
fn run_dedup_approve(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
    canonical_id: &str,
    merged_id: &str,
) -> Result<()> {
    let canonical = mneme::id::EntityId::new(canonical_id)
        .whatever_context("--canonical-id is not a valid entity id")?;
    let merged = mneme::id::EntityId::new(merged_id)
        .whatever_context("--merged-id is not a valid entity id")?;

    let record = store
        .approve_merge(&canonical, &merged)
        .whatever_context("approve_merge failed")?;

    println!(
        "Approved merge for nous {nous_id}: {canonical} absorbed {merged_name} ({facts} facts, {edges} edges).",
        canonical = record.canonical_entity_id,
        merged_name = record.merged_entity_name,
        facts = record.facts_transferred,
        edges = record.relationships_redirected,
    );

    Ok(())
}

/// Reject a queued entity merge by removing it from the review queue.
///
/// Tries both `(a, b)` and `(b, a)` orderings since `pending_merges` may
/// store either; the underlying call swallows the second ordering's
/// failure as a debug log if there is nothing to remove.
#[cfg(feature = "recall")]
fn run_dedup_reject(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
    entity_a: &str,
    entity_b: &str,
) -> Result<()> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;

    let mut params = BTreeMap::new();
    params.insert("entity_a".to_owned(), DataValue::Str(entity_a.into()));
    params.insert("entity_b".to_owned(), DataValue::Str(entity_b.into()));
    let script = r"?[entity_a, entity_b] <- [[$entity_a, $entity_b]]
                   :rm pending_merges{entity_a, entity_b}";
    // WHY: pending_merges may store either (a,b) or (b,a) order; swallow
    // the second ordering's not-found error since at most one row matches.
    let _ = store
        .run_mut_query(script, params)
        .whatever_context("pending_merges remove failed")?;
    let mut params2 = BTreeMap::new();
    params2.insert("entity_a".to_owned(), DataValue::Str(entity_b.into()));
    params2.insert("entity_b".to_owned(), DataValue::Str(entity_a.into()));
    let _ = store.run_mut_query(script, params2);

    println!("Rejected pending merge for nous {nous_id}: {entity_a} <-> {entity_b}.");
    Ok(())
}

/// Find facts with identical content (by SHA-256 hash).
#[cfg(feature = "recall")]
fn find_content_hash_duplicates(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    use std::collections::HashMap;

    use sha2::{Digest, Sha256};

    let now = mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
    let facts = store
        .query_facts(nous_id, &now, 10_000)
        .whatever_context("fact query failed")?;

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
    dupes.sort_by_key(|x| std::cmp::Reverse(x.1.len()));

    Ok(dupes)
}

// --- patterns ---

#[cfg(feature = "recall")]
fn run_patterns(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: Option<&str>,
    limit: u16,
) -> Result<()> {
    match nous_id {
        Some(nid) => println!("=== Knowledge Graph Patterns (nous: {nid}) ===\n"),
        None => println!("=== Knowledge Graph Patterns ===\n"),
    }

    let cooccurrence = find_entity_cooccurrence(store, nous_id, i64::from(limit))?;
    if cooccurrence.is_empty() {
        println!("Entity co-occurrence: no patterns found");
    } else {
        println!("Entity co-occurrence (shared facts):");
        for (entity_a, entity_b, count) in &cooccurrence {
            println!("  {entity_a} <-> {entity_b}: {count} shared facts");
        }
    }

    println!();

    let chains = find_relationship_chains(store, nous_id, i64::from(limit))?;
    if chains.is_empty() {
        println!("Relationship chains: no patterns found");
    } else {
        println!("Common relationship types:");
        for (relation, count) in &chains {
            println!("  {relation}: {count} edges");
        }
    }

    println!();

    let hubs = find_hub_entities(store, nous_id, i64::from(limit))?;
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
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: Option<&str>,
    limit: i64,
) -> Result<Vec<(String, String, i64)>> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;
    let mut params = BTreeMap::new();
    let nous_join = match nous_id {
        Some(nid) => {
            params.insert("nid".to_owned(), DataValue::Str(nid.into()));
            ",\n            *facts{id: fid, nous_id: $nid}"
        }
        None => "",
    };
    let script = format!(
        r"
        cooccur[ea, eb, count(fid)] :=
            *fact_entities{{fact_id: fid, entity_id: ea}},
            *fact_entities{{fact_id: fid, entity_id: eb}}{nous_join},
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
        .run_query(&script, params)
        .whatever_context("co-occurrence query failed")?;
    Ok((0..result.row_count())
        .filter_map(|i| {
            let a = result.get_string(i, "name_a")?;
            let b = result.get_string(i, "name_b")?;
            let cnt = result.get_i64(i, "cnt")?;
            Some((a, b, cnt))
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_relationship_chains(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: Option<&str>,
    limit: i64,
) -> Result<Vec<(String, i64)>> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;
    let mut params = BTreeMap::new();
    let body = match nous_id {
        Some(nid) => {
            params.insert("nid".to_owned(), DataValue::Str(nid.into()));
            r"
        in_nous[eid] :=
            *fact_entities{fact_id: fid, entity_id: eid},
            *facts{id: fid, nous_id: $nid}

        ?[relation, count(src)] :=
            *relationships{src, relation},
            in_nous[src]
        "
        }
        None => {
            r"
        ?[relation, count(src)] :=
            *relationships{src, relation}
        "
        }
    };
    let script = format!("{body}\n        :sort -count(src)\n        :limit {limit}\n        ");
    let result = store
        .run_query(&script, params)
        .whatever_context("relationship chain query failed")?;
    Ok((0..result.row_count())
        .filter_map(|i| {
            let rel = result.get_string(i, "relation")?;
            let cnt = result.get_i64(i, "count(src)")?;
            Some((rel, cnt))
        })
        .collect())
}

#[cfg(feature = "recall")]
fn find_hub_entities(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: Option<&str>,
    limit: i64,
) -> Result<Vec<(String, i64)>> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;
    let mut params = BTreeMap::new();
    let degree_rules = match nous_id {
        Some(nid) => {
            params.insert("nid".to_owned(), DataValue::Str(nid.into()));
            r"
        in_nous[eid] :=
            *fact_entities{fact_id: fid, entity_id: eid},
            *facts{id: fid, nous_id: $nid}

        degree[eid, count(other)] :=
            *relationships{src: eid, dst: other},
            in_nous[eid]
        degree[eid, count(other)] :=
            *relationships{src: other, dst: eid},
            in_nous[eid]
        "
        }
        None => {
            r"
        degree[eid, count(other)] :=
            *relationships{src: eid, dst: other}
        degree[eid, count(other)] :=
            *relationships{src: other, dst: eid}
        "
        }
    };
    let script = format!(
        r"{degree_rules}
        ?[name, total] :=
            degree[eid, cnt],
            total = cnt,
            *entities{{id: eid, name}}

        :sort -total
        :limit {limit}
        "
    );
    let result = store
        .run_query(&script, params)
        .whatever_context("hub entity query failed")?;
    Ok((0..result.row_count())
        .filter_map(|i| {
            let name = result.get_string(i, "name")?;
            let total = result.get_i64(i, "total")?;
            Some((name, total))
        })
        .collect())
}

// --- export-graph ---

#[cfg(feature = "recall")]
fn run_export_graph(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    format: ExportFormat,
    nous_id: Option<&str>,
    scope: Option<mneme::knowledge::MemoryScope>,
    output_path: &std::path::Path,
) -> Result<()> {
    let is_operator = is_operator();

    let file =
        std::fs::File::create(output_path).whatever_context("failed to create output file")?;
    let mut writer = std::io::BufWriter::new(file);

    // Determine if we need to load facts for sensitivity coloring or filtering.
    // WHY: scope and sensitivity are not yet columns in the Datalog facts schema
    // (#3413). When that migration lands, these filters can push down to the
    // Datalog query layer.
    let need_fact_load = !is_operator || scope.is_some() || matches!(format, ExportFormat::Dot);

    let (entities, relationships, entity_sensitivities) = if need_fact_load {
        let (visible_ids, sensitivities) = load_filtered_facts(store, nous_id, scope, is_operator)?;
        let all_entities = query_entities(store)?;
        let entities: Vec<_> = all_entities
            .into_iter()
            .filter(|e| visible_ids.contains(e.id.as_str()))
            .collect();
        let all_relationships = query_relationships(store)?;
        let relationships: Vec<_> = all_relationships
            .into_iter()
            .filter(|r| {
                visible_ids.contains(r.src.as_str()) && visible_ids.contains(r.dst.as_str())
            })
            .collect();
        (entities, relationships, Some(sensitivities))
    } else if let Some(nid) = nous_id {
        let entities = query_entities_filtered(store, nid)?;
        let relationships = query_relationships_filtered(store, nid)?;
        (entities, relationships, None)
    } else {
        let entities = query_entities(store)?;
        let relationships = query_relationships(store)?;
        (entities, relationships, None)
    };

    let entity_count = entities.len();
    let edge_count = relationships.len();

    match format {
        ExportFormat::Dot => {
            let empty = std::collections::HashMap::new();
            let sens_map = entity_sensitivities.as_ref().unwrap_or(&empty);
            export_dot(&mut writer, &entities, &relationships, sens_map)?;
        }
        ExportFormat::Graphml => {
            export_graphml(&mut writer, &entities, &relationships)?;
        }
        ExportFormat::Json => {
            export_json(&mut writer, &entities, &relationships)?;
        }
    }

    // Print summary
    let mut dist = std::collections::BTreeMap::new();
    if let Some(ref sens) = entity_sensitivities {
        for sensitivity in sens.values() {
            *dist.entry(*sensitivity).or_insert(0usize) += 1;
        }
    } else {
        dist.insert(mneme::knowledge::FactSensitivity::Public, entity_count);
    }

    println!("=== Memory Graph Export ===");
    println!("Format:        {format:?}");
    println!("Entities:      {entity_count}");
    println!("Relationships: {edge_count}");
    println!("Output:        {}", output_path.display());
    println!();
    println!("Sensitivity distribution:");
    for (sens, label) in [
        (mneme::knowledge::FactSensitivity::Public, "public"),
        (mneme::knowledge::FactSensitivity::Internal, "internal"),
        (
            mneme::knowledge::FactSensitivity::Confidential,
            "confidential",
        ),
    ] {
        let count = dist.get(&sens).copied().unwrap_or(0);
        println!("  {label}: {count}");
    }

    Ok(())
}

/// Determine whether the current process is running as an operator.
///
/// Interactive terminal sessions are treated as operators. Non-interactive
/// scripts can set `ALETHEIA_OPERATOR=1` to export all sensitivities.
#[cfg(feature = "recall")]
fn is_operator() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
        || std::env::var("ALETHEIA_OPERATOR").is_ok_and(|v| v == "1" || v == "true")
}

/// Load facts, apply scope / sensitivity filters, and return:
/// 1. The set of entity IDs linked to visible facts.
/// 2. A map of entity ID -> max sensitivity among its linked facts.
#[cfg(feature = "recall")]
fn load_filtered_facts(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: Option<&str>,
    scope: Option<mneme::knowledge::MemoryScope>,
    is_operator: bool,
) -> Result<(
    std::collections::HashSet<String>,
    std::collections::HashMap<String, mneme::knowledge::FactSensitivity>,
)> {
    use std::collections::{HashMap, HashSet};

    use mneme::knowledge::FactSensitivity;

    let facts = match nous_id {
        Some(id) => {
            let now = mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
            store
                .query_facts(id, &now, 1_000_000)
                .whatever_context("fact query failed")?
        }
        None => store
            .list_all_facts(1_000_000)
            .whatever_context("list_all_facts failed")?,
    };

    let fact_entities = load_fact_entities(store)?;
    let mut visible: HashSet<String> = HashSet::new();
    let mut sensitivities: HashMap<String, FactSensitivity> = HashMap::new();

    for fact in &facts {
        // Scope filter
        if let Some(filter_scope) = scope
            && fact.scope != Some(filter_scope)
        {
            continue;
        }

        // Sensitivity filter for non-operators
        if !is_operator && fact.sensitivity != FactSensitivity::Public {
            continue;
        }

        if let Some(entities) = fact_entities.get(fact.id.as_str()) {
            for eid in entities {
                visible.insert(eid.clone());
                let current = sensitivities
                    .get(eid)
                    .copied()
                    .unwrap_or(FactSensitivity::Public);
                if fact.sensitivity > current {
                    sensitivities.insert(eid.clone(), fact.sensitivity);
                }
            }
        }
    }

    Ok((visible, sensitivities))
}

/// Load the full `fact_entities` relation as a map of `fact_id` -> `[entity_id]`.
#[cfg(feature = "recall")]
fn load_fact_entities(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    use std::collections::{BTreeMap, HashMap};

    let script = r"?[fact_id, entity_id] := *fact_entities{fact_id, entity_id}";
    let result = store
        .run_query(script, BTreeMap::new())
        .whatever_context("fact_entities query failed")?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..result.row_count() {
        if let (Some(fid), Some(eid)) = (
            result.get_string(i, "fact_id"),
            result.get_string(i, "entity_id"),
        ) {
            map.entry(fid).or_default().push(eid);
        }
    }
    Ok(map)
}

#[cfg(feature = "recall")]
fn query_entities(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<mneme::knowledge::Entity>> {
    let result = store
        .list_entities()
        .whatever_context("entity query failed")?;
    Ok(result)
}

#[cfg(feature = "recall")]
fn validate_nous_id(nous_id: &str) -> Result<()> {
    if nous_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        whatever!(
            "invalid nous_id '{nous_id}': only alphanumeric characters, hyphens, and underscores are allowed"
        );
    }
}

#[cfg(feature = "recall")]
fn query_entities_filtered(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
) -> Result<Vec<mneme::knowledge::Entity>> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;

    validate_nous_id(nous_id)?;

    // WHY: bind nous_id as a CozoDB parameter so special characters cannot
    // escape the query string and alter the Datalog semantics.
    let mut params = BTreeMap::new();
    params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
    let script = r"
        ?[id, name, entity_type, aliases, created_at, updated_at] :=
            *entities{id, name, entity_type, aliases, created_at, updated_at},
            *fact_entities{fact_id, entity_id: id},
            *facts{fact_id, nous_id: $nous_id}
        :order name
    ";
    let result = store
        .run_query(script, params)
        .whatever_context("filtered entity query failed")?;
    parse_entity_rows(&result)
}

#[cfg(feature = "recall")]
fn query_relationships(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<Vec<mneme::knowledge::Relationship>> {
    use std::collections::BTreeMap;

    let script = r"?[src, dst, relation, weight, created_at] := *relationships{src, dst, relation, weight, created_at}";
    let result = store
        .run_query(script, BTreeMap::new())
        .whatever_context("relationship query failed")?;
    parse_relationship_rows(&result)
}

#[cfg(feature = "recall")]
fn query_relationships_filtered(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    nous_id: &str,
) -> Result<Vec<mneme::knowledge::Relationship>> {
    use mneme::engine::DataValue;
    use std::collections::BTreeMap;

    validate_nous_id(nous_id)?;

    let mut params = BTreeMap::new();
    params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
    let script = r"
        ?[src, dst, relation, weight, created_at] :=
            *relationships{src, dst, relation, weight, created_at},
            *fact_entities{fact_id: fid1, entity_id: src},
            *facts{fid1, nous_id: $nous_id},
            *fact_entities{fact_id: fid2, entity_id: dst},
            *facts{fid2, nous_id: $nous_id}
    ";
    let result = store
        .run_query(script, params)
        .whatever_context("filtered relationship query failed")?;
    parse_relationship_rows(&result)
}

#[cfg(feature = "recall")]
fn query_get_string(result: &mneme::knowledge_store::QueryResult, row: usize, col: &str) -> String {
    // kanon:ignore RUST/no-result-unwrap-or-default — query result parser helper; missing column yields empty default handled downstream
    result.get_string(row, col).unwrap_or_default()
}

#[cfg(feature = "recall")]
fn parse_entity_rows(
    result: &mneme::knowledge_store::QueryResult,
) -> Result<Vec<mneme::knowledge::Entity>> {
    let mut entities = Vec::with_capacity(result.row_count());
    for i in 0..result.row_count() {
        let id_str = query_get_string(result, i, "id");
        let name = query_get_string(result, i, "name");
        let entity_type = query_get_string(result, i, "entity_type");
        let aliases_str = query_get_string(result, i, "aliases");
        let aliases = if aliases_str.is_empty() {
            Vec::new()
        } else {
            aliases_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect()
        };
        let created_at =
            mneme::knowledge::parse_timestamp(&query_get_string(result, i, "created_at"))
                .unwrap_or_else(jiff::Timestamp::now);
        let updated_at =
            mneme::knowledge::parse_timestamp(&query_get_string(result, i, "updated_at"))
                .unwrap_or_else(jiff::Timestamp::now);

        let id =
            mneme::id::EntityId::new(&id_str).whatever_context("invalid entity id in store")?;
        entities.push(mneme::knowledge::Entity {
            id,
            name,
            entity_type,
            aliases,
            created_at,
            updated_at,
        });
    }
    Ok(entities)
}

#[cfg(feature = "recall")]
fn parse_relationship_rows(
    result: &mneme::knowledge_store::QueryResult,
) -> Result<Vec<mneme::knowledge::Relationship>> {
    use snafu::ResultExt;

    let mut relationships = Vec::with_capacity(result.row_count());
    for i in 0..result.row_count() {
        let src_str = query_get_string(result, i, "src");
        let dst_str = query_get_string(result, i, "dst");
        let relation = query_get_string(result, i, "relation");
        let weight = result.get_f64(i, "weight").unwrap_or(1.0);
        let created_at =
            mneme::knowledge::parse_timestamp(&query_get_string(result, i, "created_at"))
                .unwrap_or_else(jiff::Timestamp::now);

        let src = mneme::id::EntityId::new(&src_str)
            .whatever_context("invalid relationship src id in store")?;
        let dst = mneme::id::EntityId::new(&dst_str)
            .whatever_context("invalid relationship dst id in store")?;
        relationships.push(mneme::knowledge::Relationship {
            src,
            dst,
            relation,
            weight,
            created_at,
        });
    }
    Ok(relationships)
}

#[cfg(feature = "recall")]
fn export_dot(
    writer: &mut dyn std::io::Write,
    entities: &[mneme::knowledge::Entity],
    relationships: &[mneme::knowledge::Relationship],
    entity_sensitivities: &std::collections::HashMap<String, mneme::knowledge::FactSensitivity>,
) -> Result<()> {
    writeln!(writer, "digraph G {{").whatever_context("failed to write dot header")?;
    writeln!(
        writer,
        "  node [shape=box, style=filled, fontname=\"Helvetica\"];"
    )
    .whatever_context("failed to write dot node style")?;

    for entity in entities {
        let sens = entity_sensitivities
            .get(entity.id.as_str())
            .copied()
            .unwrap_or(mneme::knowledge::FactSensitivity::Public);
        let color = sensitivity_dot_color(sens);
        let label = format!(
            "{}\\n({})",
            dot_escape(&entity.name),
            dot_escape(&entity.entity_type)
        );
        writeln!(
            writer,
            "  \"{}\" [label=\"{}\", fillcolor=\"{}\"];",
            dot_escape(entity.id.as_str()),
            label,
            color
        )
        .whatever_context("failed to write dot node")?;
    }

    for rel in relationships {
        writeln!(
            writer,
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            dot_escape(rel.src.as_str()),
            dot_escape(rel.dst.as_str()),
            dot_escape(&rel.relation)
        )
        .whatever_context("failed to write dot edge")?;
    }

    writeln!(writer, "}}").whatever_context("failed to write dot footer")?;
    Ok(())
}

#[cfg(feature = "recall")]
fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(feature = "recall")]
fn sensitivity_dot_color(sensitivity: mneme::knowledge::FactSensitivity) -> &'static str {
    match sensitivity {
        mneme::knowledge::FactSensitivity::Public => "#90EE90", // green
        mneme::knowledge::FactSensitivity::Internal => "#FFD700", // gold
        mneme::knowledge::FactSensitivity::Confidential => "#FF6B6B", // red
    }
}

/// `GraphML` XML namespace identifier.
///
/// WHY: standardised W3C `GraphML` identifier URI, not an endpoint — every
/// `GraphML` document must embed this verbatim. Keeping it in a single-line
/// constant with a `// WHY:` marker lets `SECURITY/insecure-transport`'s
/// skip-pattern bypass the literal without an ad-hoc ignore.
#[cfg(feature = "recall")]
const GRAPHML_NS: &str = "http://graphml.graphdrawing.org/xmlns"; // WHY: standardised GraphML identifier URI, not an endpoint

#[cfg(feature = "recall")]
fn export_graphml(
    writer: &mut dyn std::io::Write,
    entities: &[mneme::knowledge::Entity],
    relationships: &[mneme::knowledge::Relationship],
) -> Result<()> {
    writeln!(writer, r#"<?xml version="1.0" encoding="UTF-8"?>"#)
        .whatever_context("failed to write graphml header")?;
    writeln!(writer, r#"<graphml xmlns="{GRAPHML_NS}">"#)
        .whatever_context("failed to write graphml root")?;

    // Keys for node data
    writeln!(
        writer,
        r#"  <key id="d0" for="node" attr.name="label" attr.type="string"/>"#
    )
    .whatever_context("failed to write graphml key")?;
    writeln!(
        writer,
        r#"  <key id="d1" for="node" attr.name="entity_type" attr.type="string"/>"#
    )
    .whatever_context("failed to write graphml key")?;
    writeln!(
        writer,
        r#"  <key id="d2" for="edge" attr.name="relation" attr.type="string"/>"#
    )
    .whatever_context("failed to write graphml key")?;
    writeln!(
        writer,
        r#"  <key id="d3" for="edge" attr.name="weight" attr.type="double"/>"#
    )
    .whatever_context("failed to write graphml key")?;

    writeln!(writer, r#"  <graph id="G" edgedefault="directed">"#)
        .whatever_context("failed to write graphml graph start")?;

    for entity in entities {
        let id_escaped = xml_escape(entity.id.as_str());
        let name_escaped = xml_escape(&entity.name);
        let type_escaped = xml_escape(&entity.entity_type);
        writeln!(
            writer,
            r#"    <node id="{id_escaped}">
      <data key="d0">{name_escaped}</data>
      <data key="d1">{type_escaped}</data>
    </node>"#
        )
        .whatever_context("failed to write graphml node")?;
    }

    for rel in relationships {
        let src_escaped = xml_escape(rel.src.as_str());
        let dst_escaped = xml_escape(rel.dst.as_str());
        let rel_escaped = xml_escape(&rel.relation);
        writeln!(
            writer,
            r#"    <edge source="{src_escaped}" target="{dst_escaped}">
      <data key="d2">{rel_escaped}</data>
      <data key="d3">{}</data>
    </edge>"#,
            rel.weight
        )
        .whatever_context("failed to write graphml edge")?;
    }

    writeln!(writer, "  </graph>").whatever_context("failed to write graphml graph end")?;
    writeln!(writer, "</graphml>").whatever_context("failed to write graphml root end")?;
    Ok(())
}

#[cfg(feature = "recall")]
fn xml_escape(s: &str) -> String {
    quick_xml::escape::escape(s).to_string()
}

#[cfg(feature = "recall")]
fn export_json(
    writer: &mut dyn std::io::Write,
    entities: &[mneme::knowledge::Entity],
    relationships: &[mneme::knowledge::Relationship],
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct JsonEntity<'a> {
        id: &'a str,
        name: &'a str,
        entity_type: &'a str,
        aliases: &'a [String],
    }

    #[derive(serde::Serialize)]
    struct JsonRelationship<'a> {
        src: &'a str,
        dst: &'a str,
        relation: &'a str,
        weight: f64,
    }

    writer
        .write_all(b"{\n  \"entities\": [")
        .whatever_context("failed to write json start")?;
    for (i, entity) in entities.iter().enumerate() {
        if i > 0 {
            writer.write_all(b",").whatever_context("json comma")?;
        }
        writer
            .write_all(b"\n    ")
            .whatever_context("json indent")?;
        let je = JsonEntity {
            id: entity.id.as_str(),
            name: &entity.name,
            entity_type: &entity.entity_type,
            aliases: &entity.aliases,
        };
        serde_json::to_writer(&mut *writer, &je)
            .whatever_context("failed to serialize json entity")?;
    }
    writer
        .write_all(b"\n  ],\n  \"relationships\": [")
        .whatever_context("failed to write json relationships start")?;
    for (i, rel) in relationships.iter().enumerate() {
        if i > 0 {
            writer.write_all(b",").whatever_context("json comma")?;
        }
        writer
            .write_all(b"\n    ")
            .whatever_context("json indent")?;
        let jr = JsonRelationship {
            src: rel.src.as_str(),
            dst: rel.dst.as_str(),
            relation: &rel.relation,
            weight: rel.weight,
        };
        serde_json::to_writer(&mut *writer, &jr)
            .whatever_context("failed to serialize json relationship")?;
    }
    writer
        .write_all(b"\n  ]\n}\n")
        .whatever_context("failed to write json end")?;
    Ok(())
}

#[cfg(all(test, feature = "recall"))]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length slices"
)]
mod tests;
